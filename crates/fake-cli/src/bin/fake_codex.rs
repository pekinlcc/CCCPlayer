//! fake-codex: deterministic stand-in for the `codex` CLI.

use std::env;
use std::path::PathBuf;

use cccplayer_fake_cli as lib;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("fake-codex 0.1.0 (stub)");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help") {
        println!("fake-codex — test fixture");
        return Ok(());
    }

    let phase = args
        .windows(2)
        .find(|w| w[0] == "--phase")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "reviewing".to_string());

    let workdir = args
        .windows(2)
        .find(|w| w[0] == "--workdir")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| env::current_dir().unwrap());

    match phase.as_str() {
        "reviewing" => do_reviewing(&workdir)?,
        "goal-check" => do_goal_check(&workdir)?,
        other => anyhow::bail!("unknown phase {other}"),
    }
    Ok(())
}

fn requires_refining(workdir: &std::path::Path) -> bool {
    // Honor an env var OR a workdir-local marker file. The file-based path is
    // robust against parallel-test env-var pollution.
    let force_env = std::env::var("CCCPLAYER_FAKE_FORCE_CHANGES").is_ok();
    let force_file = workdir.join(".fake-force-changes").exists();
    (force_env || force_file) && !workdir.join(".fake-refined").exists()
}

fn do_reviewing(workdir: &std::path::Path) -> anyhow::Result<()> {
    lib::emit("agent_started", "reviewing");
    let re = regex::Regex::new(r"^codex_review_v(\d+)\.md$").unwrap();
    let mut max_n = 0u32;
    if let Ok(rd) = std::fs::read_dir(workdir) {
        for e in rd.flatten() {
            if let Some(n) = e.file_name().to_str().and_then(|s| {
                re.captures(s)
                    .and_then(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
            }) {
                max_n = max_n.max(n);
            }
        }
    }
    let next = max_n + 1;

    // Approve iff hello.txt exists (fake-claude's test marker) AND we aren't
    // being asked to force a first-round changes_requested verdict.
    let approved = workdir.join("hello.txt").exists() && !requires_refining(workdir);
    let verdict = if approved { "approved" } else { "changes_requested" };
    let blocking = if approved { "" } else { "  - produce hello.txt\n" };

    let body = format!(
        "# Codex Review v{next}\n\
         date: {date}\n\
         goal: deliver the user-stated goal\n\n\
         ## Summary\n\
         {summary}\n\n\
         ## Findings\n\
         {findings}\n\n\
         ## Verdict\n\
         - status: {verdict}\n\
         - blocking:\n\
         {blocking}\
         - non_blocking:\n\
         ",
        date = chrono::Utc::now().format("%Y-%m-%d"),
        summary = if approved {
            "All milestones appear complete."
        } else {
            "Missing milestone: produce hello.txt."
        },
        findings = if approved {
            ""
        } else {
            "### produce hello.txt\n\
             - severity: blocking\n\
             - where: design-level\n\
             - detail: The implementing phase did not land the expected file.\n\
             - suggestion: write hello.txt in the next IMPLEMENTING turn.\n"
        },
        verdict = verdict,
        blocking = blocking,
    );

    let review_path = workdir.join(format!("codex_review_v{next}.md"));
    lib::atomic_write(&review_path, &body)?;
    lib::emit("file_created", &format!("codex_review_v{next}.md"));
    lib::emit_tokens(200, 120);
    println!(
        "REVIEW done: v{next}, verdict {verdict}, blocking {}",
        if approved { 0 } else { 1 }
    );
    Ok(())
}

fn do_goal_check(workdir: &std::path::Path) -> anyhow::Result<()> {
    let done = workdir.join("hello.txt").exists() && workdir.join("PRD.md").exists();
    let blob = serde_json::json!({
        "done": done,
        "missing": if done { Vec::<String>::new() } else { vec!["hello.txt".to_string()] },
        "next_state": if done { "DONE" } else { "IMPLEMENTING" },
        "rationale": if done { "artifacts present" } else { "implementation incomplete" },
    });
    println!("```json");
    println!("{}", serde_json::to_string_pretty(&blob)?);
    println!("```");
    Ok(())
}
