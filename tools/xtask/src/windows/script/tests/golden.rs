//! The scripts recorded for seven runs (`tests/golden/windows`), which the template
//! must make again byte for byte, and the test that records them again after a
//! change to the script that is meant.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

/// Where the script recorded for a run is: `tests/golden/windows/<name>.ps1`.
fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("windows")
        .join(format!("{name}.ps1"))
}

/// The script recorded for a run, where each carriage return it holds is the mark
/// `␍` in the file, which git's `text=auto` would take out of a file.
fn golden(name: &str) -> String {
    let path = golden_path(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(!text.contains('\r'), "{name} holds a real carriage return");
    text.replace('\u{240d}', "\r")
}

/// A run as the golden files record it: what the command line said, where its
/// copy was and where in it the command ran, what ran first, the variables named.
struct Recorded {
    golden: &'static str,
    words: &'static [&'static str],
    dir: &'static str,
    inside: &'static str,
    steps: &'static [&'static str],
    env: &'static [&'static str],
}

const RECORDED: [Recorded; 7] = [
    Recorded {
        golden: "live-paste",
        words: &["npm", "run", "live:paste"],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "eval-with-the-build",
        words: &[
            "npm",
            "run",
            "eval",
            "--",
            "--scenario",
            "round-trip",
            "--chief",
            "devin",
        ],
        dir: DIR,
        inside: ".",
        steps: &BUILD_STEPS,
        env: &[],
    },
    Recorded {
        golden: "cargo-test-in-a-folder",
        words: &["cargo", "test", "it"],
        dir: "build two",
        inside: "app/src-tauri",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "node-e-with-quotes",
        words: &["node", "-e", r#"console.log("it's 100%")"#],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    Recorded {
        golden: "echo-with-every-quote",
        words: &["echo", "a'b", "c d", "e\"f", "%PATH%"],
        dir: DIR,
        inside: "tests",
        steps: &[BUILD_STEPS[0], BUILD_STEPS[1]],
        env: &[],
    },
    // The command this landing is proven by on the machine.
    Recorded {
        golden: "xtask-test-integration",
        words: &["cargo", "xtask", "test", "integration"],
        dir: DIR,
        inside: ".",
        steps: &[],
        env: &[],
    },
    // The one with variables: what `--env` added.
    Recorded {
        golden: "env-with-the-build",
        words: &[
            "cargo",
            "xtask",
            "eval",
            "--scenario",
            "round-trip",
            "--chief",
            "devin",
        ],
        dir: DIR,
        inside: ".",
        steps: &BUILD_STEPS,
        env: &[
            "CONSENSFLOW_TEST_DAEMON=native",
            "CONSENSFLOW_NOTE=it's 100% $x `y` \"q\" & |",
        ],
    },
];

/// The script the template makes for a recorded run.
fn sent(run: &Recorded) -> String {
    let env = parse_env(&strings(run.env)).unwrap();
    remote_script(&Script {
        dir: run.dir,
        inside: run.inside,
        steps: run.steps,
        command: &command_line(run.words),
        env: &env,
    })
}

#[test]
fn a_run_is_sent_the_script_that_was_recorded_for_it_byte_for_byte() {
    for run in RECORDED {
        let (sent, expected) = (sent(&run), golden(run.golden));
        assert!(
            sent == expected,
            "{}: not what was recorded (a change that is meant is recorded again: `record_the_goldens`)\n--- sent\n{sent:?}\n--- recorded\n{expected:?}",
            run.golden
        );
    }
}

/// Records the goldens again from what the template makes. It is what to run after
/// a change to the script that is meant (`cargo test -p xtask --lib
/// windows::script::tests::golden::record_the_goldens -- --ignored`), and the diff
/// of the files is then the review of the change: a golden is no check of a script
/// that is read from it.
#[test]
#[ignore = "records the goldens again: run it after a meant change to the script"]
fn record_the_goldens() {
    for run in RECORDED {
        let text = sent(&run).replace('\r', "\u{240d}");
        fs::write(golden_path(run.golden), text).unwrap();
    }
}
