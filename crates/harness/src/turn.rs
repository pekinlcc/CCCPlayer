//! Turn-level types and output classifier. See PRD §16.8.

use cccplayer_core::events::{Agent, TurnOutcome};
use cccplayer_core::state::Phase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnResult {
    pub agent: Agent,
    pub phase: Phase,
    pub outcome: TurnOutcome,
    /// Final exit code from the CLI, if the process did exit.
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// For `TurnOutcome::RateLimited`, the wall-clock time the CLI said
    /// to retry at (if one was parseable from the error message). Used
    /// to schedule an auto-resume. `None` means "try again later, we
    /// don't know when". Added v1.3.
    #[serde(default)]
    pub retry_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Patterns that indicate the CLI refused to carry out the task. See §16.8
/// `refused` row.
pub fn is_refusal(text: &str) -> bool {
    let t = text.to_lowercase();
    const NEEDLES: &[&str] = &[
        "i can't help",
        "i can't assist",
        "i won't",
        "i cannot help",
        "unable to help",
        "i refuse",
        "i'm unable to",
        "i am unable to",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}

/// Patterns that indicate an authentication or authorization failure. See
/// §16.8 `auth_failed` row.
///
/// **v1.7.4 tightening.** The earlier needle list included `"unauthorized"`,
/// `"unauthenticated"`, `"not logged in"`, `"please login"`, `"invalid api
/// key"`, `"api key not found"` — all of which are short English phrases
/// that appear in ordinary prose whenever an agent discusses auth error
/// scenarios while writing user-facing code (Hermes Linux's setup wizard
/// triggered a false auth_failed by reasoning "the service could fail due
/// to network issues or an invalid API key"). Restricted v1.7.4 to
/// phrases that **only appear in actual CLI / API error bodies**:
///
/// * HTTP status strings are specific enough.
/// * The exact `` `please run `claude login` `` / `codex login` wordings
///   are what the CLIs print on auth failure — verbatim.
/// * The underscored JSON error-type fields (`authentication_error`,
///   `invalid_api_key`, `invalid_authentication`) come straight from the
///   vendor API error response schemas and never appear in English prose.
pub fn is_auth_failure(stderr: &str) -> bool {
    let t = stderr.to_lowercase();
    const NEEDLES: &[&str] = &[
        // HTTP layer — the reason-phrase form survives HTTP version noise
        // (`HTTP/1.1 401 Unauthorized`, `HTTP/2 401 Unauthorized`).
        "401 unauthorized",
        "status 401",
        // Claude / Codex CLI exact error wordings.
        "please run `claude login`",
        "please run `codex login`",
        // Anthropic API error-body `type` field.
        "authentication_error",
        // OpenAI API error-body `code` / `type` fields. Both are
        // underscored tokens that only appear in JSON error bodies.
        "invalid_api_key",
        "invalid_authentication",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}

#[cfg(test)]
mod tests {
    use super::is_auth_failure;

    // ----- v1.7.4 regressions: real errors must still fire --------------------

    #[test]
    fn auth_fires_on_http_401() {
        assert!(is_auth_failure("HTTP/1.1 401 Unauthorized"));
        assert!(is_auth_failure("got status 401 from the API"));
    }

    #[test]
    fn auth_fires_on_claude_cli_login_message() {
        assert!(is_auth_failure(
            "error: not authenticated — please run `claude login` and try again"
        ));
    }

    #[test]
    fn auth_fires_on_codex_cli_login_message() {
        assert!(is_auth_failure(
            "please run `codex login` to authenticate"
        ));
    }

    #[test]
    fn auth_fires_on_anthropic_api_error_body() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"..."}}"#;
        assert!(is_auth_failure(body));
    }

    #[test]
    fn auth_fires_on_openai_invalid_api_key_code() {
        let body = r#"{"error":{"code":"invalid_api_key","message":"..."}}"#;
        assert!(is_auth_failure(body));
    }

    #[test]
    fn auth_fires_on_openai_invalid_authentication_type() {
        let body = r#"{"error":{"type":"invalid_authentication","message":"..."}}"#;
        assert!(is_auth_failure(body));
    }

    // ----- v1.7.4 regressions: PRD prose must NOT false-positive --------------

    #[test]
    fn auth_does_not_fire_on_prd_prose_about_auth_errors() {
        // Verbatim paraphrase of the Hermes Linux v1.7.3 session thinking
        // that triggered the original false auth_failed. The classifier
        // was scanning Claude's content (not CLI stderr) and flipped the
        // turn to AuthFailed because the thinking said "invalid API key".
        let prose = "the service either failed to start due to network \
                     issues or an invalid API key, or the 60-second timeout \
                     wasn't enough for it to reach active status";
        assert!(
            !is_auth_failure(prose),
            "PRD prose about auth-error scenarios must not false-positive"
        );
    }

    #[test]
    fn auth_does_not_fire_on_ui_help_text() {
        // Common UI-help wordings that agents write into setup wizards.
        assert!(!is_auth_failure("Please login to continue using the service."));
        assert!(!is_auth_failure("User is not logged in; redirecting to /login."));
        assert!(!is_auth_failure("Check that your API key is not empty."));
    }

    #[test]
    fn auth_does_not_fire_on_generic_unauthorized_word() {
        // "Unauthorized" appears in OAuth comments, 403 discussions, code
        // docs, HTTP RFC quotations — never enough signal on its own to
        // mark a whole turn as auth-failed.
        assert!(!is_auth_failure(
            "OAuth returns 403 Forbidden for unauthorized scopes; 401 is reserved for missing credentials."
        ));
    }
}
