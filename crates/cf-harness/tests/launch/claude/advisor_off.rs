//! Claude Code's own advisor tool, which a project's Project settings has its members run
//! without (A5-20; "Advisor tool" left off, as every project is until the human ticks it):
//! the launch's own settings file (`--settings`) names `CLAUDE_CODE_DISABLE_ADVISOR_TOOL` in
//! its `env` block, which Claude Code reads as the kill switch of the tool for the window,
//! fresh or resumed (2.1.322, read as data and run against a stub of the API: with the
//! switch no request declares the tool, and a conversation whose history holds advisor calls
//! has them stripped; `advisorModel` in the file would not remove it on a resume). The owner's
//! own files are never written, and with the setting on the file is as it always was, byte for
//! byte. The chief's launch is not asked it by the engine, so it is never asked of this
//! adapter.

use super::compaction_window::{
    plain_copy, settings_of, settings_text, without_session, worker_settings, CHIEF_SETTINGS,
};
use super::*;

/// The file's `env` block as the switch writes it.
fn switched() -> Value {
    json!({ "CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1" })
}

#[test]
fn a_launch_of_a_project_with_the_tool_on_writes_the_settings_file_it_always_wrote_byte_for_byte() {
    let home = Home::new();
    let adapter = home.adapter();
    let plain = settings_text(&prepare(&adapter, &Request::default()).unwrap());
    assert!(!plain.contains("env"), "{plain}");
    assert!(!plain.contains("ADVISOR"), "{plain}");
    assert_eq!(plain, worker_settings(&home));
    // And the chief's, which the engine never asks to be switched.
    assert_eq!(
        settings_text(&prepare(&adapter, &Request::chief()).unwrap()),
        CHIEF_SETTINGS
    );
}

#[test]
fn every_members_launch_of_a_project_with_the_tool_off_has_it_switched_in_its_own_file_and_nothing_else_moves(
) {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let home = Home::new();
        let adapter = home.adapter();
        let plain = Request {
            role,
            ..Request::default()
        };
        let off = Request {
            advisor_off: true,
            ..plain_copy(&plain)
        };
        let without = prepare(&adapter, &plain).unwrap();
        let without_settings = settings_of(&without);
        let given = prepare(&adapter, &off).unwrap();
        let mut settings = settings_of(&given);
        assert_eq!(settings["env"], switched(), "{role}");
        // Nothing else of the file moves, and nothing of the launch beyond it.
        settings.as_object_mut().unwrap().remove("env");
        assert_eq!(settings, without_settings, "{role}");
        assert_eq!(
            without_session(&given.argv),
            without_session(&without.argv),
            "{role}: the same command line"
        );
        assert_eq!(given.env, without.env, "{role}: the same environment");
        assert_eq!(given.drop_env, without.drop_env, "{role}");
    }
}

#[test]
fn the_switch_is_given_to_a_window_resumed_as_to_one_launched_and_a_resumed_window_of_a_project_with_it_on_is_as_it_was(
) {
    let home = Home::new();
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Write the parser")]);
    let adapter = home.adapter();
    let resumed = Request {
        advisor_off: true,
        ..Request::resumed(session)
    };
    let plan = prepare(&adapter, &resumed).unwrap();
    assert!(
        plan.argv.iter().any(|arg| arg == "--resume"),
        "it is resumed"
    );
    assert_eq!(settings_of(&plan)["env"], switched());
    // A window resumed on a project with the tool on is as it was: the owner's setting, which
    // brings back an advisor an earlier window of the conversation had switched off.
    let plain = prepare(&adapter, &Request::resumed(session)).unwrap();
    assert!(settings_of(&plain).get("env").is_none());
    assert_eq!(settings_text(&plain), worker_settings(&home));
}

#[test]
fn the_switch_and_the_size_are_both_in_the_file_and_neither_moves_the_other() {
    let home = Home::new();
    let adapter = home.adapter();
    let both = settings_of(
        &prepare(
            &adapter,
            &Request {
                advisor_off: true,
                compaction: Some(400_000),
                ..Request::default()
            },
        )
        .unwrap(),
    );
    assert_eq!(both["env"], switched());
    assert_eq!(both["autoCompactWindow"], json!(400_000));
    let off = settings_of(
        &prepare(
            &adapter,
            &Request {
                advisor_off: true,
                ..Request::default()
            },
        )
        .unwrap(),
    );
    assert!(off.get("autoCompactWindow").is_none());
}

#[test]
fn the_switch_is_written_in_the_launch_s_own_file_and_in_no_file_of_the_owner_s() {
    let home = Home::new();
    let owner = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "settings.json"]);
    fs::create_dir_all(home.var("CLAUDE_CONFIG_DIR")).unwrap();
    let theirs = "{\"advisorModel\":\"claude-opus-5-5\",\"env\":{\"OWNER_KEEPS_THIS\":\"yes\"}}\n";
    fs::write(&owner, theirs).unwrap();
    let plan = prepare(
        &home.adapter(),
        &Request {
            advisor_off: true,
            ..Request::default()
        },
    )
    .unwrap();
    assert_eq!(
        settings_of(&plan)["env"],
        switched(),
        "the launch's own file"
    );
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--settings")
        .unwrap();
    assert!(
        plan.argv[at + 1].starts_with(&home.launch_folder()),
        "under the launch's own folder: {}",
        plan.argv[at + 1]
    );
    assert_eq!(
        fs::read_to_string(&owner).unwrap(),
        theirs,
        "the owner's settings are as they were"
    );
    // No other file of Claude's under its config folder was made for it, and none of the owner's
    // names the switch.
    let made: Vec<_> = fs::read_dir(home.var("CLAUDE_CONFIG_DIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(made, ["settings.json"]);
    assert!(!theirs.contains("ADVISOR"));
}
