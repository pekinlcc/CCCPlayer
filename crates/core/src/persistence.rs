//! Atomic write helpers and event log append. See PRD §8, §16.5.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::events::Event;
use crate::session::{Session, SessionMeta};

/// Write `bytes` to `path` atomically: write to `path.tmp`, fsync, rename.
/// See PRD §8 "关键文件落盘 fsync" and §16.5.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent).ok();
    // Use an extension-based sibling so the rename is atomic on the same fs.
    let tmp = path.with_extension(format!(
        "{}.cccplayer.tmp",
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
    ));
    {
        let mut f = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?; // fsync per §8
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn save_session_meta(session: &Session, meta: &SessionMeta) -> Result<()> {
    let json = serde_json::to_vec_pretty(meta)?;
    atomic_write(&session.meta_path(), &json)
}

pub fn load_session_meta(session: &Session) -> Result<Option<SessionMeta>> {
    let path = session.meta_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&text)?))
}

/// Append a single [`Event`] as one JSON line to `events.log` and fsync.
///
/// Callers must serialize their access to this function (the state-machine
/// reducer is the single writer per PRD §16.13), so no internal mutex is
/// taken.
pub fn append_event(session: &Session, event: &Event) -> Result<()> {
    let path = session.events_log_path();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    f.write_all(&line)?;
    f.sync_data()?; // fsync per §8
    Ok(())
}

/// Compute mtime (ns) + size + sha256 of a file for external-edit detection
/// per PRD §16.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub mtime_ns: i128,
    pub size: u64,
    pub sha256: String,
}

pub fn fingerprint(path: &Path) -> Result<Option<FileFingerprint>> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mtime = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mtime_ns = mtime.as_nanos() as i128;
    let size = meta.len();
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = hex::encode(hasher.finalize());
    Ok(Some(FileFingerprint {
        mtime_ns,
        size,
        sha256,
    }))
}
