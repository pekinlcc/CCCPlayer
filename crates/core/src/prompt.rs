//! Prompt templates. See PRD §17.
//!
//! Defaults are shipped inside the binary via `include_str!`. User overrides
//! live under `~/Library/Application Support/CCCPlayer/prompts/` and are
//! discovered at runtime (see [`PromptSet::load`]).

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Rendered prompt sent to a CLI. The `{workdir}`, `{round}`, `{review_n}`
/// etc. placeholders are already substituted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedPrompt(pub String);

#[derive(Debug, Clone)]
pub struct PromptSet {
    pub common: String,
    pub planning: String,
    pub implementing: String,
    pub refining: String,
    pub reviewing: String,
    pub goal_check: String,
}

impl PromptSet {
    /// Built-in defaults, compiled into the binary.
    pub fn defaults() -> Self {
        Self {
            common: include_str!("../../../prompts/common.md").to_string(),
            planning: include_str!("../../../prompts/planning.md").to_string(),
            implementing: include_str!("../../../prompts/implementing.md").to_string(),
            refining: include_str!("../../../prompts/refining.md").to_string(),
            reviewing: include_str!("../../../prompts/reviewing.md").to_string(),
            goal_check: include_str!("../../../prompts/goal-check.md").to_string(),
        }
    }

    /// Load user overrides from `dir`, falling back to defaults. Any file not
    /// present uses the default.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut set = Self::defaults();
        if let Some(s) = maybe_read(&dir.join("common.md"))? {
            set.common = s;
        }
        if let Some(s) = maybe_read(&dir.join("planning.md"))? {
            set.planning = s;
        }
        if let Some(s) = maybe_read(&dir.join("implementing.md"))? {
            set.implementing = s;
        }
        if let Some(s) = maybe_read(&dir.join("refining.md"))? {
            set.refining = s;
        }
        if let Some(s) = maybe_read(&dir.join("reviewing.md"))? {
            set.reviewing = s;
        }
        if let Some(s) = maybe_read(&dir.join("goal-check.md"))? {
            set.goal_check = s;
        }
        Ok(set)
    }
}

fn maybe_read(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(path)?))
}

/// Template variables accepted by the prompt renderer.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    pub workdir: PathBuf,
    pub round: u32,
    /// Highest existing codex review version, if any.
    pub latest_review_version: Option<u32>,
    /// Next version to write (reviewing only).
    pub next_review_version: Option<u32>,
}

/// Substitute `{workdir}`, `{round}`, `{N}` and `{N+1}` placeholders. Also
/// prepends the "common" constraints (§17.0) after the first heading, which
/// is the pattern every template expects.
pub fn render(template: &str, common: &str, ctx: &RenderContext) -> RenderedPrompt {
    let mut out = String::with_capacity(template.len() + common.len() + 32);
    out.push_str(template);
    // Append common constraints at the end — simplest & most robust vs trying
    // to splice into an arbitrary template structure.
    out.push_str("\n\n---\n\n");
    out.push_str(common);
    let out = out
        .replace("{workdir}", ctx.workdir.to_string_lossy().as_ref())
        .replace("{round}", &ctx.round.to_string())
        .replace(
            "{N}",
            &ctx.latest_review_version
                .map(|n| n.to_string())
                .unwrap_or_else(|| "1".to_string()),
        )
        .replace(
            "{N+1}",
            &ctx.next_review_version
                .map(|n| n.to_string())
                .unwrap_or_else(|| "1".to_string()),
        );
    RenderedPrompt(out)
}
