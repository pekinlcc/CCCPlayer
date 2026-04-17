//! Parsers for CLI output artifacts. See PRD §16.8 "各阶段产出物预期".

use cccplayer_core::state::Verdict;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Reassemble the model's plain-text reply from a Claude Code
/// `--output-format stream-json --verbose` stdout transcript.
///
/// Each line is a protocol event envelope. The actual text the model wrote
/// lives in:
///   * `{"type":"assistant","message":{"content":[{"type":"text","text":"…"},
///     …]}}` — one per reply turn
///   * `{"type":"result","result":"…"}` — the final combined reply (present
///     in single-shot / `-p` invocations)
///
/// Everything else (tool_use, tool_result, system, thinking, rate_limit_event)
/// is discarded. Lines that don't parse as JSON are silently skipped so this
/// stays tolerant of preamble/trailing noise.
///
/// Fallback: if no recognized envelopes are present, return the input
/// unchanged. This keeps the helper transparent for plain-text output (the
/// fake CLI used in integration tests, or real runs without `stream-json`).
pub fn extract_claude_text(stdout: &str) -> String {
    let mut out = String::new();
    let mut saw_envelope = false;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        match v.get("type").and_then(|x| x.as_str()) {
            Some("assistant") => {
                if let Some(content) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    saw_envelope = true;
                    for part in content {
                        if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                out.push_str(t);
                                out.push('\n');
                            }
                        }
                    }
                }
            }
            Some("result") => {
                if let Some(t) = v.get("result").and_then(|t| t.as_str()) {
                    saw_envelope = true;
                    out.push_str(t);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    if saw_envelope {
        out
    } else {
        stdout.to_string()
    }
}

/// A very tolerant JSON extractor: finds the first fenced ```json block (or
/// bare JSON object) and parses it. See PRD §16.3.
pub fn extract_json_block(text: &str) -> Option<serde_json::Value> {
    // Try fenced code block first.
    let fenced = Regex::new(r"(?s)```(?:json)?\s*(\{.*?\})\s*```").unwrap();
    if let Some(cap) = fenced.captures(text) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cap[1]) {
            return Some(v);
        }
    }
    // Fall back to any JSON-looking object.
    if let Some(start) = text.find('{') {
        let mut depth = 0i32;
        for (i, c) in text[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &text[start..start + i + 1];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(slice) {
                            return Some(v);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalCheckOutput {
    pub done: bool,
    pub missing: Vec<String>,
    pub next_state: String,
    pub rationale: String,
}

pub fn parse_goal_check(text: &str) -> Option<GoalCheckOutput> {
    let v = extract_json_block(text)?;
    // Accept either snake_case or camelCase; tolerate missing optional fields.
    let done = v.get("done").and_then(|x| x.as_bool())?;
    let missing = v
        .get("missing")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let next_state = v
        .get("next_state")
        .and_then(|x| x.as_str())
        .unwrap_or(if done { "DONE" } else { "IMPLEMENTING" })
        .to_string();
    let rationale = v
        .get("rationale")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Some(GoalCheckOutput {
        done,
        missing,
        next_state,
        rationale,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewParse {
    pub verdict: Verdict,
    pub blocking: Vec<String>,
    pub non_blocking: Vec<String>,
}

/// Parse the trailing `## Verdict` section of a `codex_review_v{N}.md`.
/// See PRD §10 and §17.4.
pub fn parse_review(markdown: &str) -> Option<ReviewParse> {
    // Find last "## Verdict" section.
    let verdict_idx = markdown.rfind("## Verdict")?;
    let section = &markdown[verdict_idx..];
    let status_re = Regex::new(r"(?mi)^\s*-\s*status\s*:\s*(\w+)").unwrap();
    let status = status_re.captures(section).and_then(|c| c.get(1))?;
    let verdict = match status.as_str() {
        "approved" => Verdict::Approved,
        "changes_requested" => Verdict::ChangesRequested,
        "blocked" => Verdict::Blocked,
        _ => return None,
    };

    // Parse the two sub-lists. We look for `- blocking:` / `- non_blocking:`
    // and take subsequent `  - <item>` lines until a dedent.
    let (blocking, non_blocking) = parse_bullet_lists(section);
    // Invariant: approved iff blocking empty.
    if matches!(verdict, Verdict::Approved) && !blocking.is_empty() {
        // The review text contradicts itself; treat as changes_requested
        // (strict per §17.4 "approved is only correct when blocking is empty").
        return Some(ReviewParse {
            verdict: Verdict::ChangesRequested,
            blocking,
            non_blocking,
        });
    }
    Some(ReviewParse {
        verdict,
        blocking,
        non_blocking,
    })
}

fn parse_bullet_lists(section: &str) -> (Vec<String>, Vec<String>) {
    let mut blocking = Vec::new();
    let mut non_blocking = Vec::new();
    let mut cur: Option<&mut Vec<String>> = None;
    for line in section.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- blocking:") || trimmed.starts_with("-blocking:") {
            cur = Some(&mut blocking);
            continue;
        }
        if trimmed.starts_with("- non_blocking:")
            || trimmed.starts_with("- non-blocking:")
            || trimmed.starts_with("-non_blocking:")
        {
            cur = Some(&mut non_blocking);
            continue;
        }
        if let Some(list) = cur.as_deref_mut() {
            // Accept either "  - item" (indented bullet) or "- item" immediately
            // after the header.
            if let Some(rest) = trimmed.strip_prefix("- ") {
                // But if the dedent brought us back to a new `- status:` type
                // directive, stop.
                if rest.contains(':')
                    && (rest.starts_with("status")
                        || rest.starts_with("blocking")
                        || rest.starts_with("non_blocking"))
                {
                    // new directive — stop accumulating into current list
                    cur = None;
                    continue;
                }
                list.push(rest.trim().to_string());
            } else if !trimmed.is_empty() && !line.starts_with(' ') {
                // reached non-list content at root level — stop
                cur = None;
            }
        }
    }
    (blocking, non_blocking)
}

/// Required top-level headings for a well-formed `PRD.md`. See §17.1.
pub const REQUIRED_PRD_HEADINGS: &[&str] = &[
    "Goal",
    "Current state",
    "Scope",
    "Non-goals",
    "Design",
    "Milestones",
    "Open Questions",
];

pub fn prd_is_well_formed(markdown: &str) -> bool {
    // Collect ordered list of top-level h1/h2 headings. We tolerate numbered
    // prefixes ("## 1. Goal") and extra whitespace.
    let re = Regex::new(r"(?m)^#{1,2}\s*(?:\d+\.\s*)?(.+)$").unwrap();
    let headings: Vec<String> = re
        .captures_iter(markdown)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_lowercase()))
        .collect();
    REQUIRED_PRD_HEADINGS
        .iter()
        .all(|h| headings.iter().any(|x| x.starts_with(&h.to_lowercase())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_check_roundtrip() {
        let text = "before\n```json\n{\"done\": true, \"missing\": [], \
                    \"next_state\": \"DONE\", \"rationale\": \"all set\"}\n```\nafter";
        let g = parse_goal_check(text).unwrap();
        assert!(g.done);
        assert_eq!(g.next_state, "DONE");
    }

    #[test]
    fn goal_check_unfenced_json_still_parses() {
        let text = "{\"done\": false, \"missing\": [\"a\",\"b\"], \
                    \"next_state\": \"REFINING\", \"rationale\": \"x\"}";
        let g = parse_goal_check(text).unwrap();
        assert!(!g.done);
        assert_eq!(g.missing.len(), 2);
    }

    #[test]
    fn extract_claude_text_from_assistant_and_result() {
        let stream = concat!(
            r#"{"type":"system","subtype":"hook_started"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"},{"type":"tool_use","name":"Read"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"..."}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":" world"}]}}"#,
            "\n",
            r#"{"type":"result","result":"```json\n{\"done\": false, \"missing\": [\"x\"]}\n```"}"#,
            "\n",
        );
        let text = extract_claude_text(stream);
        // Assistant texts concatenated.
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
        // Result.result included as-is (the fenced JSON survives).
        assert!(text.contains("```json"));
        assert!(text.contains("\"done\""));
        // tool_use / tool_result / system / user payloads are dropped.
        assert!(!text.contains("hook_started"));
    }

    #[test]
    fn extract_claude_text_falls_back_to_raw_on_plain_text() {
        // Fake CLI (and any non-stream-json output) emits plain text with a
        // ```json fence. The extractor must not swallow that.
        let plain = "```json\n{\"done\": true}\n```\n";
        assert_eq!(extract_claude_text(plain), plain);
    }

    #[test]
    fn goal_check_survives_claude_stream_json_wrapping() {
        let stream = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"result","result":"```json\n{\n  \"done\": false,\n  \"missing\": [\"TranscriptScanner\"],\n  \"next_state\": \"IMPLEMENTING\",\n  \"rationale\": \"M2 absent\"\n}\n```"}"#,
            "\n",
        );
        let text = extract_claude_text(stream);
        let g = parse_goal_check(&text).expect("goal-check JSON should be recoverable");
        assert!(!g.done);
        assert_eq!(g.missing, vec!["TranscriptScanner"]);
        assert_eq!(g.next_state, "IMPLEMENTING");
    }

    #[test]
    fn parse_review_approved() {
        let md = "# Codex Review v1\n\n## Findings\n\n## Verdict\n\
                  - status: approved\n\
                  - blocking:\n\
                  - non_blocking:\n";
        let r = parse_review(md).unwrap();
        assert_eq!(r.verdict, Verdict::Approved);
        assert!(r.blocking.is_empty());
    }

    #[test]
    fn parse_review_changes_requested() {
        let md = "## Verdict\n\
                  - status: changes_requested\n\
                  - blocking:\n  - missing tests\n  - no README\n\
                  - non_blocking:\n  - use clap\n";
        let r = parse_review(md).unwrap();
        assert_eq!(r.verdict, Verdict::ChangesRequested);
        assert_eq!(r.blocking.len(), 2);
        assert_eq!(r.non_blocking.len(), 1);
    }

    #[test]
    fn contradictory_review_forces_changes_requested() {
        let md = "## Verdict\n\
                  - status: approved\n\
                  - blocking:\n  - still missing X\n\
                  - non_blocking:\n";
        let r = parse_review(md).unwrap();
        assert_eq!(r.verdict, Verdict::ChangesRequested);
    }

    #[test]
    fn prd_well_formed_accepts_numbered() {
        let md = "# PRD\n\n\
                  ## 1. Goal\nx\n## 2. Current state\n-\n\
                  ## 3. Scope\n-\n## 4. Non-goals\n-\n\
                  ## 5. Design\n-\n## 6. Milestones\n-\n\
                  ## 7. Open Questions\n-\n";
        assert!(prd_is_well_formed(md));
    }

    #[test]
    fn prd_malformed_missing_section() {
        let md = "# PRD\n## Goal\nx\n## Design\n-";
        assert!(!prd_is_well_formed(md));
    }
}
