//! fake-claude: a deterministic stand-in for the real `claude` CLI used by
//! tests. Takes a subcommand that tells it which phase to simulate; follows
//! the same contracts the real CLI would (writes files atomically, prints
//! stream-json to stdout, exits 0 on success).

use std::env;
use std::path::PathBuf;

use cccplayer_fake_cli as lib;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    // Support "--version" probe.
    if args.iter().any(|a| a == "--version") {
        println!("fake-claude 0.1.0 (stub)");
        return Ok(());
    }
    // Support "--dangerously-skip-permissions --help" probe — just succeed.
    if args.iter().any(|a| a == "--help") {
        println!("fake-claude — test fixture");
        return Ok(());
    }
    // The only argument we care about is the phase name, passed via
    // "--phase <name>".
    let phase = args
        .windows(2)
        .find(|w| w[0] == "--phase")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "planning".to_string());

    // Workdir: --workdir <path> or current dir.
    let workdir = args
        .windows(2)
        .find(|w| w[0] == "--workdir")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| env::current_dir().unwrap());

    // Dispatch.
    match phase.as_str() {
        "planning" => do_planning(&workdir)?,
        "implementing" => do_implementing(&workdir)?,
        "refining" => do_refining(&workdir)?,
        "goal-check" => do_goal_check(&workdir)?,
        other => anyhow::bail!("unknown phase {other}"),
    }
    Ok(())
}

fn do_planning(workdir: &std::path::Path) -> anyhow::Result<()> {
    lib::emit("agent_started", "planning");
    let goal = std::fs::read_to_string(workdir.join("GOAL.md")).unwrap_or_default();
    let goal_line = goal.lines().next().unwrap_or("(no goal)").to_string();
    let prd = format!(
        "# PRD\n\n\
         ## 1. Goal\n{goal_line}\n\n\
         ## 2. Current state\nempty workspace\n\n\
         ## 3. Scope\n- tiny increment\n\n\
         ## 4. Non-goals\n- nothing\n\n\
         ## 5. Design\n- single file\n\n\
         ## 6. Milestones\n- [ ] produce hello.txt\n\n\
         ## 7. Open Questions\nnone\n"
    );
    lib::atomic_write(&workdir.join("PRD.md"), &prd)?;
    lib::emit("file_edited", "PRD.md");
    lib::emit_tokens(100, 200);
    lib::emit("agent_finished", "planning");
    println!("PLANNING done: 7 sections, +{}/-0 lines", prd.lines().count());
    Ok(())
}

fn do_implementing(workdir: &std::path::Path) -> anyhow::Result<()> {
    lib::emit("agent_started", "implementing");
    // If the test asked for the "first-round fails, refining fixes it"
    // pattern, leave hello.txt unwritten this turn so Codex will issue a
    // changes_requested verdict.
    // Read the force-changes flag from either an env var or a workdir-local
    // marker file (`.fake-force-changes`). The file-based path is robust
    // against parallel-test env-var pollution.
    let force_env = std::env::var("CCCPLAYER_FAKE_FORCE_CHANGES").is_ok();
    let force_file = workdir.join(".fake-force-changes").exists();
    let forcing_changes = (force_env || force_file) && !workdir.join(".fake-refined").exists();
    if forcing_changes {
        // Emit *some* artifact so the turn passes output_malformed — we write
        // a stub file that doesn't satisfy the goal.
        lib::atomic_write(&workdir.join("stub.txt"), "placeholder\n")?;
        lib::emit("file_created", "stub.txt");
        println!("IMPLEMENTING done: stub (intentional for test)");
        println!("files: stub.txt");
    } else {
        lib::atomic_write(&workdir.join("hello.txt"), "hello from fake-claude\n")?;
        lib::emit("file_created", "hello.txt");
        println!("IMPLEMENTING done: produce hello.txt");
        println!("files: hello.txt");
    }
    lib::emit_tokens(50, 30);
    lib::emit("agent_finished", "implementing");
    println!("build: n/a   tests: n/a");
    Ok(())
}

fn do_refining(workdir: &std::path::Path) -> anyhow::Result<()> {
    // Find latest review and append the "Claude Code 回应" section.
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
    if max_n == 0 {
        anyhow::bail!("no review to refine");
    }
    let review_path = workdir.join(format!("codex_review_v{max_n}.md"));
    let existing = std::fs::read_to_string(&review_path).unwrap_or_default();
    let appended = format!(
        "{existing}\n\n## Claude Code 回应\n\n\
         ### produce hello.txt\n\
         - status: accepted\n\
         - action: wrote hello.txt\n\
         - reason: blocker addressed by creating the file\n"
    );
    lib::atomic_write(&review_path, &appended)?;
    // Make sure hello.txt exists (in the refining-first scenario we write it
    // here rather than during IMPLEMENTING).
    if !workdir.join("hello.txt").exists() {
        lib::atomic_write(&workdir.join("hello.txt"), "hello from fake-claude (refined)\n")?;
    }
    // Drop the marker so the next review approves.
    std::fs::write(workdir.join(".fake-refined"), "").ok();
    lib::emit("file_edited", &format!("codex_review_v{max_n}.md"));
    lib::emit_tokens(80, 60);
    println!("REFINING done on v{max_n}: accepted 1, partial 0, rejected 0");
    Ok(())
}

fn do_goal_check(workdir: &std::path::Path) -> anyhow::Result<()> {
    lib::emit("agent_started", "goal_check");
    // Declared "done" when hello.txt exists (i.e. implementation succeeded)
    // and PRD.md exists. This keeps tests deterministic.
    let done = workdir.join("hello.txt").exists() && workdir.join("PRD.md").exists();
    let blob = serde_json::json!({
        "done": done,
        "missing": if done { Vec::<String>::new() } else { vec!["hello.txt".to_string()] },
        "next_state": if done { "DONE" } else { "IMPLEMENTING" },
        "rationale": if done { "all artifacts present" } else { "hello.txt missing" },
    });
    println!("```json");
    println!("{}", serde_json::to_string_pretty(&blob)?);
    println!("```");
    Ok(())
}
