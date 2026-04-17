//! Workdir safety checks. See PRD §16.14 (blacklist / warn / soft-warn) and §8
//! (`.gitignore` auto-append).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Hard-denied workdir roots. See PRD §16.14-A.
const BLACKLIST: &[&str] = &[
    "/",
    "/Users",
    "/System",
    "/Library",
    "/Applications",
    "/private",
    "/var",
    "/etc",
    "/bin",
    "/sbin",
    "/tmp",
    "/usr",
    "/opt",
    "/dev",
    "/cores",
    "/Volumes",
];

/// Names that, when the workdir resolves to the user's home dir or one of
/// these subpaths, require extra confirmation.
const HOME_WARN_SUBPATHS: &[&str] = &[
    "", // home dir itself
    "Documents",
    "Desktop",
    "Downloads",
    "Library",
];

/// Glob-like filename patterns that trigger soft warnings when present under
/// the workdir.
const SENSITIVE_FILE_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    "credentials",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
];

/// Extensions that trigger soft warnings.
const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key"];

/// Directory names (anywhere under workdir) that trigger soft warnings.
const SENSITIVE_DIRS: &[&str] = &[".ssh", ".aws", ".gnupg"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum SafetyVerdict {
    /// Workdir is fine; proceed.
    Ok,
    /// Workdir overlaps a user-data path that likely contains valuable files.
    /// UI must require the user to re-type the absolute path to confirm.
    StrongWarn { reason: String },
    /// Informational; UI shows a dismissible banner, no re-type required.
    SoftWarn { reasons: Vec<String> },
    /// Workdir is a system path and selection must be refused.
    Blocked { reason: String },
}

/// Classify a candidate workdir per PRD §16.14.
///
/// On non-macOS systems we still apply the logic but use `$HOME` to determine
/// the home-directory-class rules.
pub fn classify(path: &Path) -> Result<SafetyVerdict> {
    let canon = path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))?;

    // Hard blacklist.
    for deny in BLACKLIST {
        let deny_path = PathBuf::from(deny);
        if canon == deny_path {
            return Ok(SafetyVerdict::Blocked {
                reason: format!("{} is a system path; CCCPlayer refuses to run here", deny),
            });
        }
    }

    // Overlap with our own application data directory.
    if let Some(app_support) = app_support_dir() {
        if canon.starts_with(&app_support) {
            return Ok(SafetyVerdict::StrongWarn {
                reason: "Workdir overlaps CCCPlayer's application data directory"
                    .to_string(),
            });
        }
    }

    // Home-directory-class.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let home_canon = home.canonicalize().unwrap_or(home);
        for sub in HOME_WARN_SUBPATHS {
            let candidate = if sub.is_empty() {
                home_canon.clone()
            } else {
                home_canon.join(sub)
            };
            if canon == candidate {
                return Ok(SafetyVerdict::StrongWarn {
                    reason: format!(
                        "Workdir is {} — an area that typically contains personal files; \
                         CCCPlayer will let agents freely modify its contents",
                        canon.display()
                    ),
                });
            }
        }
    }

    // Soft warnings.
    let mut reasons = Vec::new();

    // Size / file count probe (cap iteration so we don't walk gigantic trees).
    let (file_count, total_bytes) = sample_tree(&canon);
    if file_count > 100_000 {
        reasons.push(format!(
            "workdir contains >100k files ({file_count}); may not be what you intended"
        ));
    }
    if total_bytes > 10 * 1024 * 1024 * 1024 {
        reasons.push(format!(
            "workdir is larger than 10 GiB (~{} GiB); may not be what you intended",
            total_bytes / (1024 * 1024 * 1024)
        ));
    }

    if let Some(sensitive) = scan_sensitive(&canon) {
        reasons.push(format!(
            "sensitive file(s) detected under workdir (e.g. {sensitive})"
        ));
    }

    if canon.join(".git").exists() && has_uncommitted_changes(&canon) {
        reasons.push("workdir has uncommitted git changes; agents may overwrite them".to_string());
    }

    if reasons.is_empty() {
        Ok(SafetyVerdict::Ok)
    } else {
        Ok(SafetyVerdict::SoftWarn { reasons })
    }
}

/// Append `.cccplayer/` to `.gitignore` if the workdir is a git repo and the
/// pattern isn't already listed. See PRD §8.
pub fn maybe_append_gitignore(workdir: &Path) -> Result<bool> {
    if !workdir.join(".git").exists() {
        return Ok(false);
    }
    let gi = workdir.join(".gitignore");
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    if existing
        .lines()
        .any(|l| l.trim() == ".cccplayer/" || l.trim() == ".cccplayer")
    {
        return Ok(false);
    }
    let mut new = existing;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(".cccplayer/\n");
    crate::persistence::atomic_write(&gi, new.as_bytes())?;
    Ok(true)
}

fn app_support_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("CCCPlayer"),
    )
}

fn sample_tree(root: &Path) -> (u64, u64) {
    // Cap the walk so very large directories return quickly.
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .take(200_000)
    {
        if entry.file_type().is_file() {
            files += 1;
            if let Ok(m) = entry.metadata() {
                bytes += m.len();
            }
        }
    }
    (files, bytes)
}

fn scan_sensitive(root: &Path) -> Option<String> {
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
        .take(100_000)
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().is_dir() && SENSITIVE_DIRS.iter().any(|d| *d == name) {
            return Some(name);
        }
        if entry.file_type().is_file() {
            if SENSITIVE_FILE_NAMES.iter().any(|n| *n == name) {
                return Some(name);
            }
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if SENSITIVE_EXTENSIONS.iter().any(|e| *e == ext) {
                    return Some(format!("*.{ext}"));
                }
            }
        }
    }
    None
}

fn has_uncommitted_changes(workdir: &Path) -> bool {
    // Best-effort; we don't fail if git isn't present.
    std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blacklist_root() {
        assert!(matches!(
            classify(&PathBuf::from("/")).unwrap(),
            SafetyVerdict::Blocked { .. }
        ));
    }

    #[test]
    fn tmp_blacklisted() {
        let v = classify(&PathBuf::from("/tmp")).unwrap();
        assert!(matches!(v, SafetyVerdict::Blocked { .. }), "got {v:?}");
    }

    #[test]
    fn random_tmp_subdir_ok_or_softwarn() {
        let dir = tempfile::tempdir().unwrap();
        // Not blacklisted, not home-warn — must be Ok or SoftWarn.
        let v = classify(dir.path()).unwrap();
        assert!(matches!(
            v,
            SafetyVerdict::Ok | SafetyVerdict::SoftWarn { .. }
        ));
    }

    #[test]
    fn gitignore_append_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        assert!(maybe_append_gitignore(dir.path()).unwrap());
        assert!(!maybe_append_gitignore(dir.path()).unwrap());
        let text = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(text.matches(".cccplayer/").count(), 1);
    }
}
