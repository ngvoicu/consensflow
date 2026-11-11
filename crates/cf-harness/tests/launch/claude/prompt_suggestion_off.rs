//! Claude Code's prompt suggestion, which a member's window has off (RS-29, T-1434): after each
//! turn Claude Code makes a call of its own to guess the person's next prompt, a fork of the
//! whole conversation read from the cache, and in a member's window nobody reads it (the next
//! message is typed by ConsensFlow). The launch's own settings file (`--settings`) names
//! `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` as `false` in its `env` block, which Claude Code
//! reads before its `promptSuggestionEnabled` setting (2.1.322, run against a stub of the API:
//! with it no suggestion request follows a turn's end, in the file alone or over the owner's own
//! `env` and setting). There is no project setting: every member's launch has it, fresh or
//! resumed. The chief's window is the human's, and its file is as it always was. The owner's
//! own files are never written.

use super::compaction_window::{
    plain_copy, settings_of, settings_text, without_session, worker_settings, CHIEF_SETTINGS,
};
use super::*;

/// The file's `env` block of a member as it is written.
fn member_env() -> Value {
    json!({ "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION": "false" })
}

#[test]
fn every_members_launch_has_the_suggestion_switched_off_in_its_own_file_and_nothing_else_moves() {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let home = Home::new();
        let adapter = home.adapter();
        let given = prepare(
            &adapter,
            &Request {
                role,
                ..Request::default()
            },
        )
        .unwrap();
        let mut settings = settings_of(&given);
        assert_eq!(settings["env"], member_env(), "{role}");
        // The file is the member's file as it always was, and the one new block.
        settings.as_object_mut().unwrap().remove("env");
        let mut before: Value = serde_json::from_str(&worker_settings(&home)).unwrap();
        before.as_object_mut().unwrap().remove("env");
        assert_eq!(settings, before, "{role}");
        assert_eq!(
            settings_of(&given)["hooks"]["PreToolUse"]
                .as_array()
                .map(Vec::len),
            Some(1),
            "{role}: the board still answers its questions"
        );
    }
}

#[test]
fn the_chiefs_launch_has_its_file_as_it_always_was_launched_or_resumed() {
    let home = Home::new();
    let adapter = home.adapter();
    let launched = prepare(&adapter, &Request::chief()).unwrap();
    assert_eq!(settings_text(&launched), CHIEF_SETTINGS);
    assert!(settings_of(&launched).get("env").is_none());
    assert!(!settings_text(&launched).contains("SUGGESTION"));
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Plan the work")]);
    let resumed = prepare(
        &adapter,
        &Request {
            resume: Some(session.to_owned()),
            ..Request::chief()
        },
    )
    .unwrap();
    assert!(
        resumed.argv.iter().any(|arg| arg == "--resume"),
        "it is resumed"
    );
    assert_eq!(settings_text(&resumed), CHIEF_SETTINGS);
}

#[test]
fn a_member_window_resumed_has_it_as_one_launched_and_the_command_line_and_environment_are_the_same(
) {
    let home = Home::new();
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Write the parser")]);
    let adapter = home.adapter();
    let resumed = prepare(&adapter, &Request::resumed(session)).unwrap();
    assert!(
        resumed.argv.iter().any(|arg| arg == "--resume"),
        "it is resumed"
    );
    assert_eq!(settings_of(&resumed)["env"], member_env());
    let launched = prepare(&adapter, &Request::default()).unwrap();
    assert_eq!(settings_text(&resumed), settings_text(&launched));
    // What the environment of the window holds is not where the switch is: the same as before
    // the switch, with the one variable Claude Code is always given.
    assert_eq!(resumed.env, launched.env);
    assert!(
        resumed
            .env
            .iter()
            .all(|(name, _)| !name.contains("SUGGESTION")),
        "{:?}",
        resumed.env
    );
    assert_eq!(resumed.drop_env, launched.drop_env);
}

#[test]
fn the_advisor_switch_the_size_and_the_suggestion_are_in_the_one_file_and_none_moves_another() {
    let home = Home::new();
    let adapter = home.adapter();
    let all = settings_of(
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
    assert_eq!(
        all["env"],
        json!({
            "CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1",
            "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION": "false",
        })
    );
    assert_eq!(all["autoCompactWindow"], json!(400_000));
    // Written in this order, so that the file is the same on every run.
    let text = serde_json::to_string(&all["env"]).unwrap();
    assert!(
        text.find("ADVISOR").unwrap() < text.find("SUGGESTION").unwrap(),
        "{text}"
    );
    // With the advisor tool left on, the suggestion's switch is alone in `env`.
    let alone = settings_of(
        &prepare(
            &adapter,
            &Request {
                compaction: Some(400_000),
                ..Request::default()
            },
        )
        .unwrap(),
    );
    assert_eq!(alone["env"], member_env());
}

#[test]
fn the_switch_is_written_in_the_launch_s_own_file_and_in_no_file_of_the_owner_s() {
    let home = Home::new();
    let owner = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "settings.json"]);
    fs::create_dir_all(home.var("CLAUDE_CONFIG_DIR")).unwrap();
    let theirs = "{\"promptSuggestionEnabled\":true,\"env\":{\"OWNER_KEEPS_THIS\":\"yes\"}}\n";
    fs::write(&owner, theirs).unwrap();
    let plan = prepare(&home.adapter(), &plain_copy(&Request::default())).unwrap();
    assert_eq!(
        settings_of(&plan)["env"],
        member_env(),
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
    let made: Vec<_> = fs::read_dir(home.var("CLAUDE_CONFIG_DIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(made, ["settings.json"]);
    // The command line is the one it had, whatever the file holds.
    assert_eq!(
        without_session(&plan.argv).len(),
        without_session(&prepare(&home.adapter(), &Request::default()).unwrap().argv).len()
    );
}
