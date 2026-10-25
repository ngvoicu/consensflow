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
//! The watch drops its readings of the first Claude Code result's worker's record for
//! 12 s after that result (`--drop-reading-sec`), as a disk that refused them would: the
//! result is unjudged a while, and the readings after fill the gap, which the report
//! keeps among what was resolved and which counts against nothing.
//!
//! What is held is that the run gets to its end (9), plays every disruption with its
//! effect seen (10; the stand-in compacts as Claude Code does at `/compact`), and keeps
//! a report of the shape it always keeps (JSON and Markdown, the harnesses and their
//! versions, the invariants, the disruptions, the breaks, the timeline), on the machine
//! alone: its console says the run's own words and identifiers, none of the report's
//! evidence, as whatever runs it takes the console off the machine. The rest of
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
/// the home they are run on; each Claude window's answers held `held` milliseconds where
/// it says (the stand-in's `CF_TEST_ANSWER_MS`: its turn reads working that long).
fn write_harnesses(bin: &Path, held: Option<u64>) {
    // A window given a conversation is the stand-in, on the Claude folder of its home,
    // which accepts each result it is given (a worker is given none); the one the run
    // opens to trust its workspace, given none, a Claude with nothing to ask; and the
    // run asks its version.
    let hold = held.map_or_else(String::new, |ms| {
        format!("\x20   CF_TEST_ANSWER_MS={ms}\n\x20   export CF_TEST_ANSWER_MS\n")
    });
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
             {hold}\
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

/// `cf-evals long --ask stand-in` with `words` besides, its reports kept in
/// `<folder>/reports`, on a home of its own in `folder` whose harnesses are the stand-ins,
/// each Claude window's answers held as `held` says ([`write_harnesses`]).
fn long_run(folder: &Path, words: &[&str], held: Option<u64>) -> cf_e2e::process::Ran {
    let (home, bin, reports) = (
        folder.join("home"),
        folder.join("bin"),
        folder.join("reports"),
    );
    files::make_dir(&home.join(".claude")).unwrap();
    write_harnesses(&bin, held);
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
        .args(["long", "--ask", "stand-in"])
        .args(words)
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
    ran
}

/// The files `folder` keeps, by name.
fn kept_in(folder: &Path) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    kept.sort();
    kept
}

#[test]
#[ignore = "plays a whole long run against the stand-ins: about fifteen minutes, and nothing spent"]
fn the_long_run_is_played_against_the_stand_ins_to_its_end_with_every_disruption_and_a_report() {
    let folder = tempfile::tempdir().unwrap();
    let reports = folder.path().join("reports");
    let ran = long_run(
        folder.path(),
        &[
            "--chief",
            "claude",
            "--staff",
            "claude,devin",
            "--model",
            "fake-chief",
            "--models",
            "claude=fake+fake,devin=fake+fake",
            "--switch-to",
            "claude",
            "--hours",
            "0.5",
            "--every-min",
            "1.5",
            "--look-sec",
            "3",
            "--quiet-min",
            "1.5",
            "--wake-min",
            "5",
            "--settle-sec",
            "5",
            "--drop-reading-sec",
            "12",
        ],
        None,
    );

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

    // It kept one report, as JSON and as Markdown, and no ledger: no window stalled, and
    // the project reached its end.
    let kept = kept_in(&reports);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept[0]
        .to_string_lossy()
        .ends_with("-long-chief-claude-staff-claude-devin.json"));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&kept[0]).unwrap()).unwrap();
    let markdown = std::fs::read_to_string(&kept[1]).unwrap();
    // The console says the run's own words and identifiers alone: none of the report's
    // evidence, which stays on the machine the run ran on.
    for part in ["## Harnesses", "## Invariants", "## Timeline"] {
        assert!(markdown.contains(part), "{part}\n{markdown}");
        assert!(!ran.stdout.contains(part), "{part} said: {ran}");
    }
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
    // Each break the watch found is kept with what it found it by, and said by its key
    // alone.
    // The readings of the first Claude Code result's worker dropped for 12 s after it
    // left that result unjudged a while: filled by the readings after, kept in the report,
    // counted against nothing.
    let resolved = report["resolved"].as_array().unwrap();
    assert!(
        resolved
            .iter()
            .any(|filled| filled["invariant"] == 2 && filled["broke"] == false),
        "{report}"
    );
    assert_eq!(report["unresolved"], serde_json::json!([]), "{report}");
    for found in report["breaks"].as_array().unwrap() {
        assert!(
            found["said"].is_string() && found["evidence"].is_object(),
            "{found}"
        );
        assert!(
            !ran.stdout.contains(found["said"].as_str().unwrap()),
            "{found} said: {ran}"
        );
    }
}

/// The Mac's 2-hour run of 2026-10-19 in short: the chief reads working for good (here
/// the stand-in, its first answer held longer than the run), the project never reaches
/// its end, and the run kept no screen of it. Now its window is kept as a stall (working
/// for longer than the idle limit) and as it was at the end, with the ledger's last
/// events, in the local report, and the ledger beside it; the console says none of it.
///
/// ```text
/// cargo test -p cf-evals --test long_dry_run -- --ignored --nocapture held
/// ```
#[test]
#[ignore = "plays a short long run against the stand-ins, its chief held working: about five minutes, and nothing spent"]
fn a_chief_held_working_past_the_idle_limit_is_kept_with_its_screen_and_the_ledger_beside_the_report(
) {
    let folder = tempfile::tempdir().unwrap();
    let reports = folder.path().join("reports");
    let ran = long_run(
        folder.path(),
        &[
            "--chief",
            "claude",
            "--staff",
            "claude",
            "--model",
            "fake-chief",
            "--models",
            "claude=fake+fake",
            "--quiet",
            "--hours",
            "0.06",
            "--idle-min",
            "1",
            "--look-sec",
            "3",
            "--settle-sec",
            "5",
        ],
        Some(30 * 60 * 1000),
    );
    // Time ran out, the chief at work: a FAIL, the end broken.
    assert_eq!(ran.code, Some(1), "{ran}");
    let kept = kept_in(&reports);
    let names: Vec<String> = kept
        .iter()
        .map(|file| file.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let file = |end: &str| {
        let at = names
            .iter()
            .position(|name| name.ends_with(end))
            .unwrap_or_else(|| panic!("no file ending {end}: {names:?}"));
        kept[at].clone()
    };
    let base = "-long-chief-claude-staff-claude";
    // The report, the ledger and, on the Mac, a sample of the chief's program when its stall
    // was found and once more at the end.
    let samples = if cfg!(target_os = "macos") { 2 } else { 0 };
    // And of the pane host beside a sample of a program whose terminal held bytes unread.
    let hosts = names
        .iter()
        .filter(|name| name.ends_with("-host-sample.txt"))
        .count();
    assert_eq!(kept.len(), 3 + samples + hosts, "{names:?}");
    let ledger_file = file(&format!("{base}-ledger.db"));
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(file(&format!("{base}.json"))).unwrap())
            .unwrap();
    let markdown = std::fs::read_to_string(file(&format!("{base}.md"))).unwrap();
    assert_eq!(report["verdict"], "FAIL", "{report}");
    assert!(
        report["breaks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|found| found["key"] == "9:end"),
        "{report}"
    );
    // The chief's window, kept as a stall: at work for longer than the minute's limit,
    // with its last screen and the trace's changes of its state.
    let stalls = &report["stalls"];
    let chief = stalls["windows"]
        .as_array()
        .unwrap_or_else(|| panic!("no stalls kept: {report}"))
        .iter()
        .find(|stall| stall["handle"] == "chief")
        .unwrap_or_else(|| panic!("no stall of the chief: {stalls}"));
    assert_eq!(chief["state"], "working", "{chief}");
    assert!(chief["minutes"].as_i64().unwrap() >= 1, "{chief}");
    assert!(!chief["screen"].as_array().unwrap().is_empty(), "{chief}");
    assert!(!chief["events"].as_array().unwrap().is_empty(), "{chief}");
    assert_eq!(chief["cleared"], Value::Null, "{chief}");
    // What the system said of its window's processes, when the stall was found and once more
    // at the end: each process of its tree with its state, and the terminal's unread bytes.
    let held = chief["held"]
        .as_array()
        .unwrap_or_else(|| panic!("no processes kept: {chief}"));
    assert_eq!(held.len(), 2, "{chief}");
    for (taken, when) in held.iter().zip(["found", "end"]) {
        assert_eq!(taken["when"], when, "{taken}");
        assert!(
            !taken["processes"].as_array().unwrap().is_empty(),
            "{taken}"
        );
        if cfg!(target_os = "macos") {
            assert!(taken["unread"]["bytes"].is_u64(), "{taken}");
            let sample = std::path::PathBuf::from(taken["sample"].as_str().unwrap());
            assert_eq!(sample, file(&format!("{base}-chief-1-{when}-sample.txt")));
            assert!(std::fs::metadata(&sample).unwrap().len() > 0, "{taken}");
            // The pane host is sampled where the chief's terminal held bytes it had not read.
            let unread = taken["unread"]["bytes"].as_u64().unwrap();
            if unread > 0 {
                let host = std::path::PathBuf::from(taken["host"].as_str().unwrap());
                assert_eq!(
                    host,
                    file(&format!("{base}-chief-1-{when}-host-sample.txt"))
                );
            } else {
                assert_eq!(taken["host"], Value::Null, "{taken}");
            }
        }
    }
    assert!(markdown.contains("processes"), "{markdown}");
    // And as it was at the end, beside the ledger's last events.
    let at_the_end = stalls["atTheEnd"].as_array().unwrap();
    assert!(
        at_the_end.iter().any(|window| window["handle"] == "chief"
            && window["state"] == "working"
            && !window["screen"].as_array().unwrap().is_empty()),
        "{stalls}"
    );
    assert!(!stalls["events"].as_array().unwrap().is_empty(), "{stalls}");
    // The ledger the app left, whole, beside the report: what the home held.
    assert_eq!(
        stalls["ledger"],
        Value::from(ledger_file.to_string_lossy().into_owned())
    );
    let ledger = rusqlite::Connection::open(&ledger_file).unwrap();
    let events: i64 = ledger
        .query_row("SELECT count(*) FROM event", [], |row| row.get(0))
        .unwrap();
    assert!(events > 0, "the copy holds the ledger's log");
    assert!(report["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .any(|moment| moment["kind"] == "stall"));
    // The records read once more after the close, and said so: each reading held.
    assert!(
        report["timeline"].as_array().unwrap().iter().any(|moment| {
            let text = moment["text"].as_str().unwrap_or_default();
            moment["kind"] == "run"
                && text.starts_with("the records were read once more after the close: ")
                && text.ends_with(", 0 readings failed")
        }),
        "{report}"
    );
    assert!(markdown.contains("\n## Stalls\n"), "{markdown}");
    assert!(
        markdown.contains("\n### The windows at the end\n"),
        "{markdown}"
    );
    // The console says none of it.
    for part in ["## Stalls", "has read working"] {
        assert!(!ran.stdout.contains(part), "{part} said: {ran}");
    }
    let processes = held
        .iter()
        .flat_map(|taken| taken["processes"].as_array().unwrap());
    for line in chief["screen"].as_array().unwrap().iter().chain(processes) {
        let line = line.as_str().unwrap().trim();
        if line.len() > 8 {
            assert!(!ran.stdout.contains(line), "{line} said: {ran}");
        }
    }
}
