//! The whole eval, played against the stand-in agent: what a run does from its first
//! word to its report, with no real harness and so nothing spent. The `claude` the
//! run finds is a script that starts the stand-in (`fake-agent` of `crates/cf-e2e`),
//! on a home of its own, so that nothing of the person's is touched; the chief and the
//! staff are on the stand-in's models (`fake-chief`, `fake`). The stand-in does not read
//! the scenario's prompt, so the chief does not do what it asks and the run's verdict
//! is mostly `FAIL`: what is held here is that the run gets to its end, says what it
//! always says in the shape it always says it, and keeps a report.
//!
//! It is opt-in, and takes a few minutes for each arm (the scenario's quiet time is a
//! minute and a half, and the `cf` and the pane host are built first); the arms take
//! their turns, since each starts a daemon:
//!
//! ```text
//! cargo test -p cf-evals --test dry_run -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(unix)]
// A failure in the scaffolding is the test's; and what the run said is shown, with
// `--nocapture`, whether the test holds or not: it is what this test is read for.
#![allow(clippy::unwrap_used, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::live::machine;
use cf_e2e::process::{own_var, Ran, Run};
use cf_e2e::{checkout, files};
use serde_json::Value;

/// The stand-in agent, built (it is built for the tests of `crates/cf-e2e` too).
fn stand_in() -> PathBuf {
    let ran = Run::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "-p",
            "cf-e2e",
            "--features",
            "test-support",
            "--bin",
            "fake-agent",
        ])
        .cwd(checkout::root())
        .inheriting_env()
        .unlimited()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    checkout::path("app/src-tauri/target/debug/fake-agent")
}

/// What a run against the stand-in left: what it said, and the report it kept.
struct Played {
    ran: Ran,
    report: Value,
    /// The name of the report's file.
    name: String,
}

impl Played {
    /// Whether the run said a line that begins with `start`.
    fn says(&self, start: &str) -> bool {
        self.ran.stdout.lines().any(|line| line.starts_with(start))
    }
}

/// Plays `words` of `cf-evals run` against the stand-in, on a home of its own.
fn play(words: &[&str]) -> Played {
    let folder = tempfile::tempdir().unwrap();
    let (home, bin, reports) = (
        folder.path().join("home"),
        folder.path().join("bin"),
        folder.path().join("reports"),
    );
    files::make_dir(&home.join(".claude")).unwrap();
    // The `claude` of the run: the stand-in, on the Claude folder of the home it is run
    // on, for a window that is given a conversation; and for the one the run opens to
    // trust its workspace, which is given none, a Claude that has nothing to ask.
    files::write_executable(
        &bin.join("claude"),
        format!(
            "#!/bin/sh\n\
             case \" $* \" in\n\
             \x20 *\" --session-id \"*|*\" --resume \"*)\n\
             \x20   CLAUDE_CONFIG_DIR=\"$HOME/.claude\"\n\
             \x20   export CLAUDE_CONFIG_DIR\n\
             \x20   exec '{}' \"$@\" ;;\n\
             esac\n\
             echo 'Welcome back!'\n\
             exec sleep 30\n",
            stand_in().display()
        ),
    )
    .unwrap();
    // The person's own cargo, which the run builds the `cf` and the pane host with.
    let real_home = machine::home().unwrap();
    let cargo_home = own_var("CARGO_HOME")
        .unwrap_or_else(|| real_home.join(".cargo").to_string_lossy().into_owned());
    let rustup_home = own_var("RUSTUP_HOME")
        .unwrap_or_else(|| real_home.join(".rustup").to_string_lossy().into_owned());
    let path = std::env::join_paths(
        std::iter::once(bin.clone())
            .chain(std::env::split_paths(&own_var("PATH").unwrap_or_default())),
    )
    .unwrap();

    let ran = Run::new(env!("CARGO_BIN_EXE_cf-evals"))
        .arg("run")
        .args(words)
        .args(["--chief", "claude", "--staff", "claude"])
        .args(["--model", "fake-chief", "--claude-staff-model", "fake"])
        .args(["--timeout-min", "6"])
        .arg("--reports")
        .arg(&reports)
        .inheriting_env()
        .var("HOME", &home)
        .var("PATH", &path)
        .var("CARGO_HOME", cargo_home)
        .var("RUSTUP_HOME", rustup_home)
        .cwd(checkout::root())
        .limit(Duration::from_secs(25 * 60))
        .run()
        .unwrap();
    eprintln!("{ran}");

    // It ended, with a verdict: 0 if it held, 1 if it did not. (A run that fails
    // says why on its error stream and exits 1 as well: it has no verdict to say.)
    assert!(matches!(ran.code, Some(0 | 1)), "{ran}");
    assert!(
        ran.stdout
            .lines()
            .any(|line| line.starts_with("  report: ")),
        "the run ended without a verdict: {ran}"
    );
    // It kept one report, of the shape the summary reads.
    let kept: Vec<PathBuf> = std::fs::read_dir(&reports)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let report = serde_json::from_str(&std::fs::read_to_string(&kept[0]).unwrap()).unwrap();
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    Played { ran, report, name }
}

#[test]
#[ignore = "plays a whole eval against the stand-in agent: a few minutes, and nothing spent"]
fn the_card_arm_is_played_against_the_stand_in_agent_and_says_and_keeps_what_it_always_does() {
    let played = play(&["--scenario", "round-trip"]);
    let (ran, report) = (&played.ran, &played.report);
    assert!(
        played.says("daemon: native (rust "),
        "the daemon it ran on: {ran}"
    );
    assert!(played.says("trust: "), "the trust of the workspace: {ran}");
    assert!(
        ran.stdout.lines().any(|line| line.starts_with("  [")
            && line.contains("s] chief (claude) ready; typing the prompt")),
        "{ran}"
    );
    assert!(
        played.says(
            "round-trip · chief claude (fake-chief, effort high) · staff effort medium · staff claude · run 1 · "
        ),
        "{ran}"
    );
    assert!(played.says("  the plumbing:"), "{ran}");
    assert!(played.says("  report: "), "{ran}");

    assert!(
        played
            .name
            .ends_with("-round-trip-chief-claude-staff-claude-1.json"),
        "{}",
        played.name
    );
    for key in [
        "scenario",
        "chief",
        "model",
        "metrics",
        "checks",
        "mechanics",
        "log",
    ] {
        assert!(report.get(key).is_some(), "{key} is in the report");
    }
    assert_eq!(report["arm"], "card");
    assert_eq!(report["model"], "fake-chief");
    assert_eq!(report["checks"].as_array().unwrap().len(), 7);
    assert_eq!(report["mechanics"].as_array().unwrap().len(), 6);
    assert!(!report["log"].as_array().unwrap().is_empty());
    // The chief took part: the daemon's copy of its conversation is not empty.
    assert!(
        report["metrics"]["chiefTurns"].as_i64().unwrap() >= 1,
        "{report}"
    );
}

#[test]
#[ignore = "plays a whole eval against the stand-in agent: a few minutes, and nothing spent"]
fn the_nocard_arm_is_played_against_the_stand_in_agent_with_the_card_the_daemon_reads_from_a_file()
{
    // The chief's card is the one line of `evals/fixtures/no-card.md`, named to the
    // daemon by a variable: a path it could not read would leave the chief with no
    // window to be ready in.
    let played = play(&["--scenario", "simple-fix", "--arm", "nocard"]);
    let (ran, report) = (&played.ran, &played.report);
    assert!(played.says("daemon: native (rust "), "{ran}");
    assert!(
        played.says(
            "simple-fix · chief claude [nocard] (fake-chief, effort high) · staff effort medium · staff claude · run 1 · "
        ),
        "{ran}"
    );
    assert!(played.says("  report: "), "{ran}");
    assert!(
        played
            .name
            .ends_with("-simple-fix-chief-claude-nocard-staff-claude-1.json"),
        "{}",
        played.name
    );
    assert_eq!(report["arm"], "nocard");
    assert_eq!(report["checks"].as_array().unwrap().len(), 5);
    assert_eq!(report["mechanics"].as_array().unwrap().len(), 6);
    assert!(
        report["metrics"]["chiefTurns"].as_i64().unwrap() >= 1,
        "{report}"
    );
}

#[test]
#[ignore = "plays a whole eval against the stand-in agent: a few minutes, and nothing spent"]
fn the_bare_arm_is_played_against_the_stand_in_agent_and_says_and_keeps_what_it_always_does() {
    let played = play(&["--scenario", "simple-fix", "--arm", "bare"]);
    let (ran, report) = (&played.ran, &played.report);
    assert!(played.says("daemon: native (rust "), "{ran}");
    assert!(
        ran.stdout.lines().any(|line| line.starts_with("  [")
            && line.contains("s] chief (claude, bare) ready; typing the prompt")),
        "{ran}"
    );
    assert!(
        played.says("simple-fix · chief claude [bare] (fake-chief, effort high) · run 1 · "),
        "{ran}"
    );
    assert!(played.says("  report: "), "{ran}");
    assert!(
        played
            .name
            .ends_with("-simple-fix-chief-claude-bare-1.json"),
        "{}",
        played.name
    );
    assert_eq!(report["arm"], "bare");
    assert_eq!(report["checks"].as_array().unwrap().len(), 5);
    // A bare run has no board: its numbers are those of a chief that put nothing on it.
    assert_eq!(report["metrics"]["tasks"], serde_json::json!([]));
    assert!(report.get("mechanics").is_none());
    assert!(report["session"].is_string(), "{report}");
}
