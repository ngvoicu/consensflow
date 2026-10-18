//! The whole long run, played against the stand-ins: what a run does from its first
//! word to its report, with no real harness and so nothing spent. The `claude` the
//! run finds is a script that starts the stand-in agent (`fake-agent` of
//! `crates/cf-e2e`) and the `devin` one that starts the stand-in Devin (`fake-devin`),
//! on a home of their own, so that nothing of the person's is touched. The owner
//! speaks the stand-in's words (`--ask stand-in`): the chief's question is answered
//! with `DISPATCH` lines, which put the work of the real ask on the board in the
//! stand-in's shapes (a job that runs while a turn works, a job that wakes its
//! worker when it ends, Devin's long turns), and each disruption is due a minute and
//! a half after the one before.
//!
//! The chief, before the switch and after it, accepts each result it is given, as a
//! real chief takes its staff's work (`CF_TEST_ACCEPT_RESULTS`, which only this run's
//! `claude` sets), so that the project reaches its end: every task accepted, and
//! the owner told after the last.
//!
//! What is held is that the run gets to its end (9), plays every disruption with its
//! effect seen (10; the stand-in compacts as Claude Code does at `/compact`), and keeps
//! a report of the shape it always keeps (JSON and Markdown, the harnesses and their
//! versions, the invariants, the disruptions, the breaks, the timeline). The rest of
//! its verdict is whatever the watch found: what the daemon does with a job a turn left
//! is the daemon's, and a break of a known shape (`evals/known-breaks.json`) passes as
//! known.
//!
//! It is opt-in, and takes about fifteen minutes, on a budget of half an hour (the
//! `cf` and the pane host are built first):
//!
//! ```text
//! cargo test -p cf-evals --test long_dry_run -- --ignored --nocapture
//! ```

#![cfg(unix)]
// A failure in the scaffolding is the test's; and what the run said is shown, with
// `--nocapture`, whether the test holds or not: it is what this test is read for.
#![allow(clippy::unwrap_used, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::machine;
use cf_e2e::process::{own_var, Run};
use cf_e2e::{checkout, files};
use serde_json::Value;

/// The stand-in `name` of `crates/cf-e2e`, built.
fn stand_in(name: &str) -> PathBuf {
    let ran = Run::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "-p",
            "cf-e2e",
            "--features",
            "test-support",
            "--bin",
            name,
        ])
        .cwd(checkout::root())
        .inheriting_env()
        .unlimited()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0), "{ran}");
    checkout::path("app/src-tauri/target/debug").join(name)
}

/// The `claude` and the `devin` of the run, in `bin`: the stand-ins, on the folders of
/// the home they are run on.
fn write_harnesses(bin: &Path) {
    // A window given a conversation is the stand-in, on the Claude folder of its home,
    // which accepts each result it is given (a worker is given none); the one the run
    // opens to trust its workspace, given none, a Claude with nothing to ask; and the
    // run asks its version.
    files::write_executable(
        &bin.join("claude"),
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = --version ]; then echo '2.1.0 (Claude Code, stand-in)'; exit 0; fi\n\
             case \" $* \" in\n\
             \x20 *\" --session-id \"*|*\" --resume \"*)\n\
             \x20   CLAUDE_CONFIG_DIR=\"$HOME/.claude\"\n\
             \x20   CF_TEST_ACCEPT_RESULTS=1\n\
             \x20   export CLAUDE_CONFIG_DIR CF_TEST_ACCEPT_RESULTS\n\
             \x20   exec '{}' \"$@\" ;;\n\
             esac\n\
             echo 'Welcome back!'\n\
             exec sleep 30\n",
            stand_in("fake-agent").display()
        ),
    )
    .unwrap();
    // The run asks a Devin its version with `devin version`, the daemon with `--version`.
    files::write_executable(
        &bin.join("devin"),
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 version|--version) echo 'devin 3000.11.0 (stand-in)'; exit 0 ;;\n\
             esac\n\
             exec '{}' \"$@\"\n",
            stand_in("fake-devin").display()
        ),
    )
    .unwrap();
}

#[test]
#[ignore = "plays a whole long run against the stand-ins: about fifteen minutes, and nothing spent"]
fn the_long_run_is_played_against_the_stand_ins_to_its_end_with_every_disruption_and_a_report() {
    let folder = tempfile::tempdir().unwrap();
    let (home, bin, reports) = (
        folder.path().join("home"),
        folder.path().join("bin"),
        folder.path().join("reports"),
    );
    files::make_dir(&home.join(".claude")).unwrap();
    write_harnesses(&bin);
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
        .arg("long")
        .args([
            "--ask",
            "stand-in",
            "--chief",
            "claude",
            "--staff",
            "claude,devin",
        ])
        .args([
            "--model",
            "fake-chief",
            "--models",
            "claude=fake+fake,devin=fake+fake",
        ])
        .args([
            "--switch-to",
            "claude",
            "--hours",
            "0.5",
            "--every-min",
            "1.5",
        ])
        .args(["--look-sec", "3", "--quiet-min", "1.5", "--wake-min", "5"])
        .args(["--settle-sec", "5"])
        .arg("--reports")
        .arg(&reports)
        .inheriting_env()
        .var("HOME", &home)
        .var("PATH", &path)
        .var("CARGO_HOME", cargo_home)
        .var("RUSTUP_HOME", rustup_home)
        .cwd(checkout::root())
        .limit(Duration::from_secs(60 * 60))
        .run()
        .unwrap();
    eprintln!("{ran}");

    // It ended, with a verdict: 0 if it held, 1 if it did not, and its report said.
    assert!(matches!(ran.code, Some(0 | 1)), "{ran}");
    let says = |start: &str| ran.stdout.lines().any(|line| line.starts_with(start));
    assert!(says("daemon: native (rust "), "the daemon it ran on: {ran}");
    assert!(says("claude: 2.1.0 (Claude Code, stand-in)"), "{ran}");
    assert!(says("devin: devin 3000.11.0 (stand-in)"), "{ran}");
    assert!(
        says("long run · chief claude (fake-chief) · staff claude,devin · "),
        "{ran}"
    );
    assert!(says("  report: ") && says("  timeline: "), "{ran}");

    // It kept one report, as JSON and as Markdown.
    let mut kept: Vec<PathBuf> = std::fs::read_dir(&reports)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    kept.sort();
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept[0]
        .to_string_lossy()
        .ends_with("-long-chief-claude-staff-claude-devin.json"));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&kept[0]).unwrap()).unwrap();
    let markdown = std::fs::read_to_string(&kept[1]).unwrap();
    // The run says its report last, as it keeps it: what a run on zeewin brings back.
    assert!(
        ran.stdout.trim_end().ends_with(markdown.trim_end()),
        "the report is said last: {ran}"
    );
    assert_eq!(report["kind"], "long-run");
    // A pass, with the known breaks it carries said, is the exit code 0; a fail is 1.
    let verdict = report["verdict"].as_str().unwrap();
    let known = verdict
        .strip_prefix("PASS with ")
        .and_then(|rest| rest.strip_suffix(" known"))
        .is_some_and(|count| count.parse::<u32>().is_ok());
    assert!(matches!(verdict, "PASS" | "FAIL") || known, "{verdict}");
    assert_eq!(ran.code == Some(0), verdict.starts_with("PASS"), "{ran}");
    assert!(
        ran.stdout.lines().any(|line| line
            .starts_with("long run · chief claude (fake-chief) · staff claude,devin · ")
            && line.ends_with(&format!(" · {verdict}"))),
        "the verdict is said on the summary line: {ran}"
    );
    assert_eq!(report["error"], Value::Null, "{report}");
    assert_eq!(report["ask"], "stand-in");

    // Every disruption was played, each at a moment the run chose, and each with its
    // effect seen: the task paused and on again, the killed window's task paused, the
    // compaction in the chief's record, the project open again with its chief, the
    // chief's new window on the agent it was switched to.
    let disruptions = report["disruptions"].as_array().unwrap();
    let played: Vec<(&str, &str)> = disruptions
        .iter()
        .map(|one| {
            (
                one["disruption"].as_str().unwrap(),
                one["state"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        played,
        [
            ("pause", "played"),
            ("kill", "played"),
            ("compact", "played"),
            ("restart", "played"),
            ("switch", "played"),
            ("devin-cut", "played"),
            ("suspend", "played"),
        ],
        "{report}"
    );
    // The project reached its end (9), every task accepted and the owner told after
    // the last, and each disruption had its effect (10).
    for (at, number) in [(8, 9), (9, 10)] {
        let invariant = &report["invariants"][at];
        assert_eq!(
            (&invariant["number"], &invariant["held"]),
            (&Value::from(number), &Value::Bool(true)),
            "{report}"
        );
    }

    // The harnesses with their versions, every invariant, and the timeline.
    let harnesses: Vec<&str> = report["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|harness| harness["harness"].as_str().unwrap())
        .collect();
    assert_eq!(harnesses, ["claude", "devin"]);
    assert_eq!(
        report["harnesses"][1]["version"],
        "devin 3000.11.0 (stand-in)"
    );
    assert_eq!(report["invariants"].as_array().unwrap().len(), 10);
    let timeline = report["timeline"].as_array().unwrap();
    assert!(timeline.iter().any(|moment| moment["kind"] == "task"));
    assert!(timeline.iter().any(|moment| moment["kind"] == "owner"));
    for part in [
        "## Harnesses",
        "## Invariants",
        "## Disruptions",
        "## Breaks",
        "## Timeline",
    ] {
        assert!(markdown.contains(part), "{part}\n{markdown}");
    }
    // Each break the watch found is kept with what it found it by.
    for found in report["breaks"].as_array().unwrap() {
        assert!(
            found["said"].is_string() && found["evidence"].is_object(),
            "{found}"
        );
    }
}
