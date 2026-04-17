//! Redaction for event log, transcripts, and UI streams. See PRD §12.

use once_cell::sync::Lazy;
use regex::Regex;

static PATTERNS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    vec![
        (Regex::new(r"sk-[A-Za-z0-9_\-]{20,}").unwrap(), "<REDACTED:api-key>"),
        (Regex::new(r"ghp_[A-Za-z0-9]{20,}").unwrap(), "<REDACTED:github-pat>"),
        (Regex::new(r"ghu_[A-Za-z0-9]{20,}").unwrap(), "<REDACTED:github-user>"),
        (Regex::new(r"ghs_[A-Za-z0-9]{20,}").unwrap(), "<REDACTED:github-server>"),
        (Regex::new(r"xoxb-[A-Za-z0-9\-]+").unwrap(), "<REDACTED:slack-bot>"),
        (Regex::new(r"xoxp-[A-Za-z0-9\-]+").unwrap(), "<REDACTED:slack-user>"),
        (Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(), "<REDACTED:aws-access-key>"),
        (
            Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[^-]*-----END [A-Z ]*PRIVATE KEY-----")
                .unwrap(),
            "<REDACTED:private-key>",
        ),
        (Regex::new(r"Bearer\s+[A-Za-z0-9._\-]{20,}").unwrap(), "Bearer <REDACTED>"),
    ]
});

/// Apply all known secret patterns and return a redacted copy.
pub fn redact(input: &str) -> String {
    let mut out = input.to_string();
    for (re, repl) in PATTERNS.iter() {
        out = re.replace_all(&out, *repl).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_patterns() {
        let src = "token=sk-abcdefghijklmnopqrstuv and ghp_abcdefghijklmnopqrstuvwxyz01";
        let r = redact(src);
        assert!(r.contains("REDACTED:api-key"));
        assert!(r.contains("REDACTED:github-pat"));
        assert!(!r.contains("sk-abcdefghijklmnopqrstuv"));
    }

    #[test]
    fn preserves_harmless() {
        let src = "normal log line with no secrets";
        assert_eq!(redact(src), src);
    }
}
