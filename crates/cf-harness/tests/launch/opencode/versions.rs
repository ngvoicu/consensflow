//! Which OpenCode a window is opened on: the version the program says is
//! asked first, once as it is on disk, and an OpenCode of 2 or more is refused
//! in words that say what to do, before anything is made. It is no case of
//! Node's, whose launch never asked.

use std::fs;
use std::path::Path;

use cf_harness::seams::processes::Failed;
use cf_harness::seams::Ports;
use cf_harness::testing::{called, named};
use serde_json::Value;

use super::stage::{planned, Home, Stage, Wanted};

/// The words an OpenCode of 2 or more is refused with.
const REFUSED: &str =
    "OpenCode 2 or newer is not supported yet. Install the last OpenCode 1.x release";

/// What a stage's stand-in `opencode` says to `--version`, its line end with it.
fn says(stage: &Stage, version: &str) {
    stage.fakes.processes.every_answer(
        &called(Path::new(&stage.home.executable)),
        Ok(format!("{version}\n")),
    );
}

/// The programs the stage's adapter has run, as they were asked.
fn asked(stage: &Stage) -> Vec<String> {
    stage
        .fakes
        .processes
        .take_ran()
        .iter()
        .map(|(program, _)| named(program))
        .collect()
}

/// The questions of its version the stage's adapter has asked: each launch
/// asks OpenCode its configuration too (`skills`), which is no question of this.
fn versions_asked(stage: &Stage) -> Vec<String> {
    asked(stage)
        .into_iter()
        .filter(|question| question.ends_with(" --version"))
        .collect()
}

#[test]
fn an_opencode_that_says_it_is_2_is_refused_in_words_that_say_what_to_do_and_nothing_is_made() {
    for wanted in [Wanted::default(), Wanted::resuming("ses_old")] {
        let stage = Stage::new();
        // Everything a launch of OpenCode 1 would go on to is there for it.
        stage.serves_creation("ses_abc123");
        says(&stage, "opencode v2.0.29");
        let refused = stage.prepare(&wanted).err();
        assert_eq!(refused.as_deref(), Some(REFUSED), "{:?}", wanted.resume);
        let name = called(Path::new(&stage.home.executable));
        assert_eq!(asked(&stage), [format!("{name} --version")]);
        // No server was started, no conversation made, nothing asked of a peer.
        assert!(stage.started.started.borrow().is_empty(), "no server");
        assert!(stage.fakes.loopback.take_asked().is_empty(), "no request");
        // No port or token was drawn: the first port is still the first free.
        assert!(stage.fakes.entropy.take_draws().is_empty(), "no draw");
        assert_eq!(stage.fakes.ports.free_loopback(), Ok(41_000));
        // No plugin, role text or copy of Specmint was written: ConsensFlow's
        // folder was never made.
        assert!(
            !Path::new(stage.home.var("CONSENSFLOW_HOME")).exists(),
            "nothing was written"
        );
    }
}

#[test]
fn an_opencode_of_1_x_is_asked_its_version_once_and_launches_as_before_one_updated_since_is_asked_again(
) {
    let stage = Stage::new();
    says(&stage, "1.19.0");
    let name = called(Path::new(&stage.home.executable));
    let question = format!("{name} --version");
    stage.fresh("ses_first");
    stage.fresh("ses_second");
    assert_eq!(
        versions_asked(&stage),
        std::slice::from_ref(&question),
        "one question for an unchanged OpenCode"
    );
    // An update gives the file another size and time: it is asked again, and
    // what it says now is what a launch is held to.
    fs::write(
        &stage.home.executable,
        "#!/bin/sh\n# an OpenCode that updated itself\nexit 0\n",
    )
    .unwrap();
    says(&stage, "opencode v2.0.29");
    stage.serves_creation("ses_third");
    assert_eq!(
        stage.prepare(&Wanted::default()).err().as_deref(),
        Some(REFUSED)
    );
    assert_eq!(versions_asked(&stage), std::slice::from_ref(&question));
}

#[test]
fn an_opencode_that_cannot_be_asked_or_says_no_version_is_launched_as_before() {
    let stage = Stage::new();
    // Nothing scripted: the question fails as an OpenCode that will not start
    // does, and the launch is not refused on a guess.
    stage.fresh("ses_first");
    let name = called(Path::new(&stage.home.executable));
    assert_eq!(versions_asked(&stage), [format!("{name} --version")]);
    // It answers, and says no version.
    says(&stage, "opencode");
    stage.fresh("ses_second");
    assert_eq!(versions_asked(&stage), [format!("{name} --version")]);
    // It says one that exits with a code, and is read all the same.
    stage.fakes.processes.every_answer(
        &name,
        Err(Failed {
            message: format!("Command failed: {name} --version\n"),
            code: Some(1),
            killed: false,
            stdout: "opencode v2.0.29\n".to_owned(),
        }),
    );
    fs::write(&stage.home.executable, "#!/bin/sh\n# another\nexit 1\n").unwrap();
    stage.serves_creation("ses_third");
    assert_eq!(
        stage.prepare(&Wanted::default()).err().as_deref(),
        Some(REFUSED)
    );
}

#[test]
fn a_window_is_told_not_to_update_itself_in_the_configuration_it_and_its_server_run_with() {
    let stage = Stage::new();
    let plan = stage.fresh("ses_abc123");
    let config: Value =
        serde_json::from_str(planned(&plan, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
    assert_eq!(config["autoupdate"], false);
    // The throwaway server that made the conversation ran on the same one.
    let started = stage.started.started.borrow();
    assert_eq!(
        started[0].env.text("OPENCODE_CONFIG_CONTENT"),
        planned(&plan, "OPENCODE_CONFIG_CONTENT")
    );
}

#[test]
fn what_the_human_set_of_updates_is_kept_in_the_windows_configuration() {
    for said in ["true", "\"notify\"", "false"] {
        let home = Home::new().sharing(
            "OPENCODE_CONFIG_CONTENT",
            &format!(r#"{{"autoupdate":{said}}}"#),
        );
        let stage = Stage::with(home, None);
        let plan = stage.fresh("ses_abc123");
        let config: Value =
            serde_json::from_str(planned(&plan, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert_eq!(config["autoupdate"].to_string(), said);
    }
}
