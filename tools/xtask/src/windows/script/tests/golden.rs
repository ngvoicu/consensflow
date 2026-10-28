//! The scripts recorded for seven runs (`tests/golden/windows`), which the templates
//! must make again byte for byte, and the test that records them again after a
//! change to the scripts that is meant. A run has two: `<name>.ps1`, the bootstrap that
//! goes on ssh's command line, and `<name>.run.ps1`, the run script that goes in the
//! tarball.

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

/// The two scripts the templates make for a recorded run: the bootstrap, the run script.
fn sent(run: &Recorded) -> [(String, String); 2] {
    let env = parse_env(&strings(run.env)).unwrap();
    let script = Script {
        id: ID,
        dir: run.dir,
        inside: run.inside,
        steps: run.steps,
        command: &command_line(run.words),
        env: &env,
        node: r"C:\Program Files\nodejs",
        // The runs recorded are zeewin's (its Node), which takes less at once than it has.
        limits: crate::windows::machine::Machine::named("zeewin")
            .unwrap()
            .limits,
    };
    [
        (run.golden.to_owned(), remote_script(&script)),
        (format!("{}.run", run.golden), run_script(&script)),
    ]
}

#[test]
fn a_run_is_sent_the_script_that_was_recorded_for_it_byte_for_byte() {
    for run in RECORDED {
        for (name, sent) in sent(&run) {
            let expected = golden(&name);
            assert!(
                sent == expected,
                "{name}: not what was recorded (a change that is meant is recorded again: `record_the_goldens`)\n--- sent\n{sent:?}\n--- recorded\n{expected:?}"
            );
        }
    }
}

/// Records the goldens again from what the templates make. It is what to run after
/// a change to the scripts that is meant (`cargo test -p xtask --lib
/// windows::script::tests::golden::record_the_goldens -- --ignored`), and the diff
/// of the files is then the review of the change: a golden is no check of a script
/// that is read from it.
#[test]
#[ignore = "records the goldens again: run it after a meant change to the script"]
fn record_the_goldens() {
    for run in RECORDED {
        for (name, text) in sent(&run) {
            fs::write(golden_path(&name), text.replace('\r', "\u{240d}")).unwrap();
        }
    }
}
