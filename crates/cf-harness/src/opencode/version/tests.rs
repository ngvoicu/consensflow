//! Which OpenCode is one ConsensFlow opens no window on yet: a version of 2 or
//! more, read from what OpenCode says to `--version`, and nothing that cannot
//! be read.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::seams::processes::Failed;
use crate::testing::{called, fake_executable, finished, ScriptedProcesses};

#[test]
fn an_opencode_of_2_or_more_is_not_supported_yet_as_1_x_and_2_say_their_versions() {
    let long = format!("{}.0.0", "9".repeat(400));
    for said in [
        "opencode v2.0.29",
        "opencode v2.0.29\n",
        "2.0.29",
        "v2.0.0",
        "opencode 2.0.0-beta.3",
        "opencode v2.1.0 (build 1.19.0)",
        "opencode v3.1.4",
        "opencode 10.0.0",
        "5.0.0",
        &long,
    ] {
        assert!(unsupported(said), "{said:?}");
    }
    for said in [
        "1.19.0",
        "1.19.0\n",
        "opencode 1.4.0",
        "opencode v1.99.99",
        "v1.0.0",
        "1.19.0-beta.1",
        "opencode 1.19.0 (build 2.0.1)",
        "0.9.9",
        "opencode 0.0.1",
    ] {
        assert!(!unsupported(said), "{said:?}");
    }
}

#[test]
fn what_names_no_version_of_three_numbers_is_no_opencode_that_is_not_supported() {
    for said in [
        "",
        "opencode",
        "opencode dev",
        "opencode version two",
        "2.0",
        "v2",
        "opencode v2.0",
        "unknown option: --version",
        "a2.0.29",
        // Digits JavaScript's `\d` does not take.
        "\u{FF12}.0.29",
    ] {
        assert!(!unsupported(said), "{said:?}");
    }
}

#[test]
fn the_words_say_what_is_not_supported_and_what_to_do() {
    assert_eq!(
        not_supported(),
        "OpenCode 2 or newer is not supported yet. Install the last OpenCode 1.x release"
    );
}

/// A throwaway disk with a stand-in `opencode` on it, and the programs a test
/// scripts.
struct Stand {
    dir: TempDir,
    executable: String,
    scripted: Rc<ScriptedProcesses>,
    probes: Probes,
}

impl Stand {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = fake_executable(&dir.path().join("opencode"))
            .to_string_lossy()
            .into_owned();
        Self {
            dir,
            executable,
            scripted: Rc::new(ScriptedProcesses::default()),
            probes: Probes::default(),
        }
    }

    /// What the stand-in says to `--version`, its line end with it.
    fn says(&self, version: &str) {
        self.scripted.every_answer(
            &called(Path::new(&self.executable)),
            Ok(format!("{version}\n")),
        );
    }

    /// How many times the stand-in was run since it was last asked.
    fn runs(&self) -> usize {
        self.scripted.take_ran().len()
    }

    fn check(&self, executable: &str) -> Result<(), String> {
        let processes: Rc<dyn Processes> = Rc::clone(&self.scripted) as Rc<dyn Processes>;
        finished(Box::pin(check(
            &self.probes,
            &processes,
            &Env::default(),
            executable,
        )))
    }
}

#[test]
fn an_opencode_that_says_it_is_2_is_refused_in_the_words_and_a_1_x_goes_on() {
    let stand = Stand::new();
    stand.says("1.19.0");
    assert_eq!(stand.check(&stand.executable), Ok(()));
    stand.says("opencode v2.0.29");
    // The answer of the one on this disk is the one kept.
    assert_eq!(stand.check(&stand.executable), Ok(()));
    // Another file is asked again: an update of the CLI is.
    fs::write(&stand.executable, "#!/bin/sh\n# 2\nexit 0\n").unwrap();
    assert_eq!(
        stand.check(&stand.executable),
        Err(
            "OpenCode 2 or newer is not supported yet. Install the last OpenCode 1.x release"
                .to_owned()
        )
    );
    assert_eq!(
        stand.runs(),
        2,
        "one question for each of the two OpenCode files, and never one for the same file twice"
    );
}

#[test]
fn a_version_it_says_is_read_whatever_it_exits_with() {
    let stand = Stand::new();
    stand.scripted.every_answer(
        &called(Path::new(&stand.executable)),
        Err(Failed {
            message: "Command failed: opencode --version\n".to_owned(),
            code: Some(1),
            killed: false,
            stdout: "opencode v2.0.29\n".to_owned(),
        }),
    );
    assert_eq!(
        stand.check(&stand.executable),
        Err(not_supported()),
        "an exit with a code is an answer"
    );
}

#[test]
fn an_opencode_that_cannot_be_asked_or_says_no_version_is_not_refused_on_a_guess() {
    let stand = Stand::new();
    // Nothing scripted: the stand-in is not there to be started, as a program
    // that was updated away is, and the question is asked again each time.
    assert_eq!(stand.check(&stand.executable), Ok(()));
    assert_eq!(stand.check(&stand.executable), Ok(()));
    assert_eq!(stand.runs(), 2, "a question that failed is asked again");
    // It answers, and says no version.
    stand.says("opencode");
    assert_eq!(stand.check(&stand.executable), Ok(()));
    assert_eq!(stand.runs(), 1);
    // A file that is gone is not asked at all.
    let gone = stand.dir.path().join("gone").join("opencode");
    assert_eq!(stand.check(&gone.to_string_lossy()), Ok(()));
    assert_eq!(stand.runs(), 0);
}
