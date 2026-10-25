//! The skills an OpenCode window keeps of the human's own: OpenCode reads the
//! window's configuration last and a list in it replaces the files' own, so the
//! paths OpenCode's files give the window's folder are asked of OpenCode itself
//! (`opencode debug config`) and put before the role's (T-540: a member lost
//! the skills the same OpenCode opened in a terminal there had). It is no case
//! of Node's, whose launch never asked.

use std::path::Path;

use cf_harness::contract::Prepared;
use cf_harness::seams::processes::Failed;
use cf_harness::testing::{called, named};
use serde_json::{json, Value};

use super::stage::{planned, Home, Stage, Wanted};

/// The `OPENCODE_CONFIG_CONTENT` a plan gives the window, as JSON.
fn configuration(plan: &Prepared) -> Value {
    serde_json::from_str(planned(plan, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap()
}

/// What the stage's stand-in `opencode` says to `debug config`.
fn says_its_configuration(stage: &Stage, answer: Result<String, Failed>) {
    let name = called(Path::new(&stage.home.executable));
    stage
        .fakes
        .processes
        .always_answer(&format!("{name} debug config"), answer);
}

/// The skill paths of a plan's window, as texts.
fn skill_paths(plan: &Prepared) -> Vec<String> {
    configuration(plan)["skills"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().replace('\\', "/"))
        .collect()
}

/// The role's own skills folder, where a plan's window finds it.
fn role_skills(stage: &Stage) -> String {
    format!("{}/role/.claude/skills", stage.home.launch_folder()).replace('\\', "/")
}

#[test]
fn the_skill_paths_opencode_gives_the_windows_folder_come_first_and_the_roles_after_them() {
    for wanted in [Wanted::default(), Wanted::resuming("ses_old")] {
        let stage = Stage::new();
        stage.serves_creation("ses_abc123");
        let resolved = json!({
            "$schema": "https://opencode.ai/config.json",
            "skills": { "paths": ["/user/skills", "./kept-as-said"] },
            "autoupdate": false,
        });
        says_its_configuration(&stage, Ok(format!("{resolved}\n")));
        let plan = stage.prepare(&wanted).unwrap();
        assert_eq!(
            skill_paths(&plan),
            ["/user/skills", "./kept-as-said", &role_skills(&stage)],
            "{:?}",
            wanted.resume
        );
        // Asked once, in the folder the window works in, after its version.
        let name = called(Path::new(&stage.home.executable));
        let ran = stage.fakes.processes.take_ran();
        let asked: Vec<String> = ran.iter().map(|(program, _)| named(program)).collect();
        assert_eq!(
            asked,
            [format!("{name} --version"), format!("{name} debug config")]
        );
        let (program, _) = &ran[1];
        assert_eq!(program.cwd.as_deref(), Some(Path::new(&stage.home.work())));
    }
}

#[test]
fn opencode_is_asked_with_the_humans_configuration_and_its_updates_off() {
    for (set, asked_with) in [
        (None, json!({ "autoupdate": false })),
        (
            Some(r#"{"theme":"user"}"#),
            json!({ "theme": "user", "autoupdate": false }),
        ),
        // The human's word on updates is kept, as the window's is.
        (
            Some(r#"{"autoupdate":true}"#),
            json!({ "autoupdate": true }),
        ),
    ] {
        let home = match set {
            Some(config) => Home::new().sharing("OPENCODE_CONFIG_CONTENT", config),
            None => Home::new(),
        };
        let stage = Stage::with(home, None);
        stage.fresh("ses_abc123");
        let ran = stage.fakes.processes.take_ran();
        let (program, _) = ran
            .iter()
            .find(|(program, _)| program.args == ["debug", "config"])
            .unwrap();
        let given: Value =
            serde_json::from_str(program.env.text("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert_eq!(given, asked_with, "{set:?}");
    }
}

#[test]
fn a_path_the_human_set_in_the_environment_and_one_opencode_names_are_each_there_once() {
    let home = Home::new().sharing(
        "OPENCODE_CONFIG_CONTENT",
        r#"{"skills":{"paths":["/from/env"]}}"#,
    );
    let stage = Stage::with(home, None);
    stage.serves_creation("ses_abc123");
    // OpenCode reads the environment's configuration too, and names its path.
    says_its_configuration(
        &stage,
        Ok(r#"{"skills":{"paths":["/from/env"]}}"#.to_owned()),
    );
    let plan = stage.prepare(&Wanted::default()).unwrap();
    assert_eq!(skill_paths(&plan), ["/from/env", &role_skills(&stage)]);
}

#[test]
fn an_opencode_that_cannot_say_its_configuration_leaves_the_window_as_it_was() {
    let failed = Failed {
        message: "Command failed: opencode debug config".to_owned(),
        code: Some(1),
        killed: false,
        stdout: String::new(),
    };
    for answer in [
        None,
        Some(Err(failed)),
        Some(Ok("no json\n".to_owned())),
        Some(Ok(r#"{"skills":{"paths":"/not/a/list"}}"#.to_owned())),
        Some(Ok(r#"{"skills":{"paths":["/ok", 1]}}"#.to_owned())),
        Some(Ok("[]".to_owned())),
    ] {
        let stage = Stage::new();
        stage.serves_creation("ses_abc123");
        if let Some(answer) = answer.clone() {
            says_its_configuration(&stage, answer);
        }
        let plan = stage.prepare(&Wanted::default()).unwrap();
        assert_eq!(skill_paths(&plan), [role_skills(&stage)], "{answer:?}");
    }
}
