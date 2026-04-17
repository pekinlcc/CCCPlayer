//! Shared helpers for the fake CLIs. See PRD §19 item 3.

use std::path::Path;

/// Emit a stream-json line (one JSON object per line) the way the real
/// `claude --output-format stream-json` CLI would. Here we just use our own
/// shape; the real harness parses it loosely.
pub fn emit(kind: &str, summary: &str) {
    let ts = chrono::Utc::now().to_rfc3339();
    let line = serde_json::json!({
        "type": kind,
        "summary": summary,
        "at": ts,
    });
    println!("{line}");
}

pub fn emit_tokens(input: u64, output: u64) {
    let line = serde_json::json!({
        "type": "usage",
        "input_tokens": input,
        "output_tokens": output,
    });
    println!("{line}");
}

/// Atomically write `content` to `path` (same protocol every agent is expected
/// to follow per §16.5).
pub fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|s| s.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
