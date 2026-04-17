//! Snapshot subsystem: per-Round tar+zstd archives under `.cccplayer/snapshots/`.
//! See PRD §8 (layout, `round-00`), §16.5 (atomic write / rollback).

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Directories excluded from snapshots by default. See PRD §8 snapshot excludes.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".cccplayer", // never snapshot our own state
    ".git",       // user's git history is not our concern
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    "dist",
    "build",
    ".tox",
    ".gradle",
    ".idea",
    ".vscode",
    ".DS_Store",
];

/// Create a tar.zst snapshot of `workdir` into `out`, excluding well-known
/// regenerable directories. Writes atomically: tmp file + fsync + rename.
pub fn create(workdir: &Path, out: &Path, excludes: &[&str]) -> Result<u64> {
    if !out
        .extension()
        .map(|e| e == "zst")
        .unwrap_or(false)
    {
        bail!("snapshot path must end in .zst");
    }
    let tmp = out.with_extension("zst.tmp");
    if let Some(parent) = tmp.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Estimate disk space — we refuse if there's less free than 1.2× the walked
    // size. Per PRD §8 "磁盘空间预检".
    let estimate = estimate_size(workdir, excludes);
    if let Some(free) = free_space(out) {
        if free < (estimate * 12 / 10) {
            bail!(
                "insufficient free space to create snapshot: {} bytes estimated, {} bytes free",
                estimate,
                free
            );
        }
    }

    {
        let out_file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let buf = BufWriter::new(out_file);
        let enc = zstd::Encoder::new(buf, 3)?.auto_finish();
        let mut builder = tar::Builder::new(enc);
        builder.follow_symlinks(false);

        for entry in walkdir::WalkDir::new(workdir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded(e.path(), workdir, excludes))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path == workdir {
                continue;
            }
            let rel = path
                .strip_prefix(workdir)
                .context("strip_prefix in snapshot")?;
            if entry.file_type().is_dir() {
                builder.append_dir(rel, path)?;
            } else if entry.file_type().is_file() {
                let mut f = File::open(path)?;
                builder.append_file(rel, &mut f)?;
            } else if entry.file_type().is_symlink() {
                // Best-effort: record the link itself.
                if let Ok(target) = std::fs::read_link(path) {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_mode(0o777);
                    header.set_cksum();
                    builder.append_link(&mut header, rel, target)?;
                }
            }
        }
        builder.finish()?;
    }
    // fsync before rename per §8.
    let f = File::open(&tmp)?;
    f.sync_all().ok();
    drop(f);
    std::fs::rename(&tmp, out)?;
    let sz = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    Ok(sz)
}

/// Restore `workdir` from a snapshot archive. Overwrites existing files.
pub fn restore(archive: &Path, workdir: &Path) -> Result<()> {
    let f = File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let dec = zstd::Decoder::new(BufReader::new(f))?;
    let mut ar = tar::Archive::new(dec);
    ar.set_preserve_permissions(true);
    // Unpack into workdir. We intentionally do NOT wipe the directory first —
    // callers that want a clean restore should remove the target subtree
    // first.
    ar.unpack(workdir)?;
    Ok(())
}

fn is_excluded(path: &Path, root: &Path, excludes: &[&str]) -> bool {
    if let Ok(rel) = path.strip_prefix(root) {
        for comp in rel.components() {
            let name = comp.as_os_str().to_string_lossy();
            if excludes.iter().any(|e| *e == name) {
                return true;
            }
        }
    }
    false
}

fn estimate_size(workdir: &Path, excludes: &[&str]) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(workdir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path(), workdir, excludes))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn free_space(path: &Path) -> Option<u64> {
    // Best-effort; returns None if statvfs isn't available.
    let target = path.parent().unwrap_or(path);
    fs2::available_space(target).ok()
}

/// Convenience: the default path for a round snapshot.
pub fn snapshot_path(snapshots_dir: &Path, round: u32) -> PathBuf {
    snapshots_dir.join(format!("round-{round:02}.tar.zst"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_small_tree() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("hello.txt"), b"hi").unwrap();
        std::fs::create_dir_all(src.path().join("node_modules/foo")).unwrap();
        std::fs::write(src.path().join("node_modules/foo/bar.js"), b"junk").unwrap();

        let out = src.path().join("out.tar.zst");
        create(src.path(), &out, DEFAULT_EXCLUDES).unwrap();

        let dest = tempfile::tempdir().unwrap();
        restore(&out, dest.path()).unwrap();
        assert!(dest.path().join("hello.txt").exists());
        // Excluded:
        assert!(!dest.path().join("node_modules/foo/bar.js").exists());
    }
}
