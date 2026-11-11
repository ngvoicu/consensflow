//! Claude Code's prompt suggestion, which a project's Project settings has its windows run
//! without ("Prompt suggestion" left off, as every project is until the human ticks it; RS-29,
//! T-1434, T-1444): after each turn Claude Code makes a call of its own to guess the person's
//! next prompt, a fork of the whole conversation read from the cache. The launch's own settings
//! file (`--settings`) names `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` as `false` in its `env` block,
//! which Claude Code reads before its `promptSuggestionEnabled` setting (2.1.322, run against a
//! stub of the API: with it no suggestion request follows a turn's end, in the file alone or over
//! the owner's own `env` and setting). It is given to the chief's launch as to a member's, fresh or
//! resumed. With the suggestion ticked on, the key is absent and the file is as it always was,
//! byte for byte: ConsensFlow never writes `true`, so Claude Code's own default and the owner's own
//! settings decide. The owner's own files are never written.

use super::compaction_window::{
    plain_copy, settings_of, settings_text, without_session, worker_settings, CHIEF_SETTINGS,
};
use super::*;

/// The file's `env` block as the switch writes it.
fn switched() -> Value {
    json!({ "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION": "false" })
}

const ROLES: [&str; 6] = [
    "chief", "worker", "advisor", "reviewer", "tester", "designer",
];

fn asked(role: &'static str) -> Request {
    if role == "chief" {
        Request::chief()
    } else {
        Request {
            role,
            ..Request::default()
        }
    }
}

#[test]
fn a_launch_of_a_project_with_the_suggestion_on_writes_the_settings_file_it_always_wrote_byte_for_byte(
) {
    let home = Home::new();
    let adapter = home.adapter();
    let member = settings_text(&prepare(&adapter, &Request::default()).unwrap());
    assert!(!member.contains("env"), "{member}");
    assert!(!member.contains("SUGGESTION"), "{member}");
    assert_eq!(member, worker_settings(&home));
    // The chief's, too: nothing about the suggestion, not even `true`, so Claude Code's own
    // default and the owner's own settings decide.
    let chief = settings_text(&prepare(&adapter, &Request::chief()).unwrap());
    assert!(!chief.contains("SUGGESTION"), "{chief}");
    assert_eq!(chief, CHIEF_SETTINGS);
}

#[test]
fn every_launch_of_a_project_with_the_suggestion_off_has_it_switched_in_its_own_file_the_chiefs_too_and_nothing_else_moves(
) {
    for role in ROLES {
        let home = Home::new();
        let adapter = home.adapter();
        let on = asked(role);
        let off = Request {
            suggestion_off: true,
            ..plain_copy(&on)
        };
        let without = prepare(&adapter, &on).unwrap();
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
        assert!(
            given
                .env
                .iter()
                .all(|(name, _)| !name.contains("SUGGESTION")),
            "{role}: not in the environment, where the owner's own is: {:?}",
            given.env
        );
        assert_eq!(given.drop_env, without.drop_env, "{role}");
        let hooks = &settings_of(&given)["hooks"]["PreToolUse"];
        assert_eq!(
            hooks.as_array().map(Vec::len),
            Some(usize::from(role != "chief")),
            "{role}: the board still answers a member's questions, and the chief's are Claude's own"
        );
    }
}

#[test]
fn the_switch_is_given_to_a_window_resumed_as_to_one_launched_the_chiefs_among_them_and_a_resumed_window_of_a_project_with_it_on_is_as_it_was(
) {
    let home = Home::new();
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Write the parser")]);
    let adapter = home.adapter();
    for role in ROLES {
        let resumed = Request {
            suggestion_off: true,
            resume: Some(session.to_owned()),
            ..asked(role)
        };
        let plan = prepare(&adapter, &resumed).unwrap();
        assert!(
            plan.argv.iter().any(|arg| arg == "--resume"),
            "{role}: it is resumed"
        );
        assert_eq!(settings_of(&plan)["env"], switched(), "{role}");
        // The same file, bar the launch it is for, as the one a fresh launch is given.
        let fresh = prepare(
            &adapter,
            &Request {
                suggestion_off: true,
                ..asked(role)
            },
        )
        .unwrap();
        assert_eq!(settings_of(&plan), settings_of(&fresh), "{role}");
        assert_eq!(plan.env, fresh.env, "{role}");
        assert_eq!(plan.drop_env, fresh.drop_env, "{role}");
        // A window resumed on a project with the suggestion on is as it was: Claude Code's own
        // default and the owner's own settings, which a window of the same conversation that
        // had it switched off no longer holds.
        let plain = prepare(
            &adapter,
            &Request {
                resume: Some(session.to_owned()),
                ..asked(role)
            },
        )
        .unwrap();
        assert!(settings_of(&plain).get("env").is_none(), "{role}");
    }
    let plain = prepare(&adapter, &Request::resumed(session)).unwrap();
    assert_eq!(settings_text(&plain), worker_settings(&home));
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
                suggestion_off: true,
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
                suggestion_off: true,
                compaction: Some(400_000),
                ..Request::default()
            },
        )
        .unwrap(),
    );
    assert_eq!(alone["env"], switched());
    // And with the suggestion left on, the advisor's switch is alone, as it was.
    let advisor = settings_of(
        &prepare(
            &adapter,
            &Request {
                advisor_off: true,
                ..Request::default()
            },
        )
        .unwrap(),
    );
    assert_eq!(
        advisor["env"],
        json!({ "CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1" })
    );
}

#[test]
fn the_switch_is_written_in_the_launch_s_own_file_and_in_no_file_of_the_owner_s() {
    let home = Home::new();
    let owner = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "settings.json"]);
    fs::create_dir_all(home.var("CLAUDE_CONFIG_DIR")).unwrap();
    let theirs = "{\"promptSuggestionEnabled\":true,\"env\":{\"OWNER_KEEPS_THIS\":\"yes\"}}\n";
    fs::write(&owner, theirs).unwrap();
    for role in ["worker", "chief"] {
        let plan = prepare(
            &home.adapter(),
            &Request {
                suggestion_off: true,
                ..asked(role)
            },
        )
        .unwrap();
        assert_eq!(
            settings_of(&plan)["env"],
            switched(),
            "{role}: the launch's own file"
        );
        let at = plan
            .argv
            .iter()
            .position(|arg| arg == "--settings")
            .unwrap();
        assert!(
            plan.argv[at + 1].starts_with(&home.launch_folder()),
            "{role}: under the launch's own folder: {}",
            plan.argv[at + 1]
        );
    }
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
    // With the suggestion left on, the launch writes nothing about it, though the owner's own
    // setting says it is on: it is theirs to decide.
    let on = prepare(&home.adapter(), &Request::chief()).unwrap();
    assert!(!settings_text(&on).contains("SUGGESTION"));
}
