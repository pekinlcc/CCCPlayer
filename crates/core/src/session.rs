//! Session metadata persisted as `.cccplayer/session.json`. See PRD §8.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::state::{Phase, SessionState};

/// Current schema version for `session.json`. See PRD §8 forward-compat rule.
pub const SCHEMA_VERSION: u32 = 1;

/// Tokens used by a single agent, accumulated across the session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChildProcessRecord {
    pub pid: i32,
    /// `CLOCK_BOOTTIME` value when the process started, used together with pid
    /// to detect stale records across reboots and pid reuse. See PRD §16.1.
    pub start_boottime_ns: u128,
    pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub schema_version: u32,
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: SessionState,
    pub phase: Phase,
    pub round: u32,
    pub workdir: PathBuf,
    /// Live child processes we care to clean up on next launch if we crash.
    pub children: Vec<ChildProcessRecord>,
    /// Seconds-of-no-heartbeat threshold currently in effect.
    pub stall_threshold_secs: u64,
}

impl SessionMeta {
    pub fn new(id: String, workdir: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION,
            id,
            created_at: now,
            updated_at: now,
            state: SessionState::Created,
            phase: Phase::Idle,
            round: 0,
            workdir,
            children: Vec::new(),
            stall_threshold_secs: 600,
        }
    }

    pub fn bump(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// Wraps the `.cccplayer/` directory for a Session. Owns the flock.
pub struct Session {
    workdir: PathBuf,
    dir: PathBuf,
    _lock: std::fs::File,
}

impl Session {
    /// Open or create the `.cccplayer/` directory for `workdir` and take an
    /// exclusive flock. Fails if another instance already holds it. Fails if
    /// the persisted `session.json` has a `schema_version` greater than what
    /// this binary supports. See PRD §8.
    pub fn open(workdir: &Path) -> Result<Self> {
        let workdir = workdir
            .canonicalize()
            .with_context(|| format!("canonicalize {}", workdir.display()))?;
        let dir = workdir.join(".cccplayer");
        std::fs::create_dir_all(&dir).context("create .cccplayer")?;
        std::fs::create_dir_all(dir.join("snapshots")).ok();
        std::fs::create_dir_all(dir.join("transcripts")).ok();

        let lock_path = dir.join("session.lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .context("open session.lock")?;
        lock.try_lock_exclusive().map_err(|e| {
            anyhow::anyhow!(
                "another CCCPlayer instance is already using {}: {e}",
                workdir.display()
            )
        })?;
        // Write our pid+start for diagnostics; the kernel flock is the
        // authoritative mutex.
        let pid = std::process::id();
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let _ = std::fs::write(
            &lock_path,
            format!("{{\"pid\":{pid},\"start_ns\":{start}}}\n"),
        );

        let meta_path = dir.join("session.json");
        if meta_path.exists() {
            let text = std::fs::read_to_string(&meta_path)?;
            // Peek just the schema version first so we give a clean error
            // message on forward-incompat files.
            let peek: serde_json::Value = serde_json::from_str(&text)
                .context("session.json is not valid JSON")?;
            let ver = peek
                .get("schema_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if ver > SCHEMA_VERSION as u64 {
                bail!(
                    "session.json was written by a newer CCCPlayer (schema {ver} > {max}); \
                     please upgrade",
                    max = SCHEMA_VERSION
                );
            }
        }

        Ok(Self {
            workdir,
            dir,
            _lock: lock,
        })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn meta_path(&self) -> PathBuf {
        self.dir.join("session.json")
    }

    pub fn events_log_path(&self) -> PathBuf {
        self.dir.join("events.log")
    }

    pub fn usage_path(&self) -> PathBuf {
        self.dir.join("usage.json")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.dir.join("snapshots")
    }

    pub fn transcripts_dir(&self) -> PathBuf {
        self.dir.join("transcripts")
    }

    pub fn goal_path(&self) -> PathBuf {
        self.workdir.join("GOAL.md")
    }

    pub fn prd_path(&self) -> PathBuf {
        self.workdir.join("PRD.md")
    }

    /// Returns the next unused review version number (= max existing + 1, or 1).
    pub fn next_review_version(&self) -> Result<u32> {
        let re = regex::Regex::new(r"^codex_review_v(\d+)\.md$").unwrap();
        let mut max: u32 = 0;
        for e in std::fs::read_dir(&self.workdir)? {
            let e = e?;
            if let Some(name) = e.file_name().to_str() {
                if let Some(caps) = re.captures(name) {
                    if let Ok(n) = caps.get(1).unwrap().as_str().parse::<u32>() {
                        max = max.max(n);
                    }
                }
            }
        }
        Ok(max + 1)
    }
}
