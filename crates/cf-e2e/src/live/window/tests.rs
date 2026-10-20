//! What a window is held to without a harness: which harnesses the app pastes
//! into, the id a conversation is opened on, what is the screen since a mark, and
//! when a window has drawn and answered.

use std::cell::Cell;

use super::open::{
    add_hooks, asked_for, consensflows_hooks_of, new_session, pasted_into, session_for, trust_line,
    with_hooks, Member,
};
use super::*;

fn item(role: &str, text: &str) -> Item {
    Item {
        id: String::new(),
        role: role.to_owned(),
        text: text.to_owned(),
        complete: false,
    }
}

#[test]
fn the_app_pastes_into_claude_and_devin_and_no_other_harness() {
    let names = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        pasted_into(&names(&["claude", "devin"])).unwrap(),
        [Harness::Claude, Harness::Devin]
    );
    assert!(pasted_into(&[]).unwrap().is_empty());
    for other in ["codex", "pi", "opencode"] {
        let failed = pasted_into(&names(&[other])).unwrap_err().to_string();
        assert_eq!(
            failed,
            format!(
                "the app pastes into claude and devin only; {other} takes its messages through its own queue"
            )
        );
    }
    let failed = pasted_into(&names(&["claud"])).unwrap_err().to_string();
    assert_eq!(
        failed,
        "no harness is called claud: claude, opencode, pi, devin, codex"
    );
}

#[test]
fn what_came_of_the_trust_asked_of_a_folder_is_said_as_the_window_of_whoever_asked() {
    assert_eq!(trust_line("claude", "trusted"), "claude: trust: trusted");
    assert_eq!(
        trust_line("huginn max", "already trusted"),
        "huginn max: trust: already trusted"
    );
}

#[test]
fn a_run_that_names_no_harness_is_asked_for_devin_and_one_that_names_some_for_those() {
    assert_eq!(asked_for(Vec::new()), ["devin"]);
    let named = vec!["claude".to_owned(), "devin".to_owned()];
    assert_eq!(asked_for(named.clone()), named);
}

#[test]
fn a_conversation_id_is_a_version_4_uuid_and_a_new_one_each_time() {
    let (first, second) = (new_session(), new_session());
    assert_ne!(first, second);
    for id in [first, second] {
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{id}"
        );
        assert!(parts[2].starts_with('4'), "{id}");
        assert!(
            id.chars().all(|at| at == '-' || at.is_ascii_hexdigit()),
            "{id}"
        );
    }
}

#[test]
fn claude_code_and_pi_are_opened_on_an_id_they_are_given_and_the_others_name_their_own() {
    for kind in ["claude-code", "pi"] {
        assert_eq!(session_for(kind).map(|id| id.len()), Some(36), "{kind}");
    }
    for kind in ["devin", "codex", "opencode", "claude", ""] {
        assert_eq!(session_for(kind), None, "{kind}");
    }
}

#[test]
fn a_member_has_the_first_message_it_was_given_and_none_until_it_is() {
    let member = Member::new("worker", "# worker");
    assert_eq!(member.message(), None);
    let told = member.first_message("[ConsensFlow m-1 · T-1 · task from @chief]\nGo");
    assert_eq!(
        told.message(),
        Some("[ConsensFlow m-1 · T-1 · task from @chief]\nGo")
    );
}

#[test]
fn a_member_has_the_owners_hooks_unless_it_is_asked_to_be_without_them() {
    let member = Member::new("worker", "# worker");
    assert!(!member.is_alone());
    assert!(member.without_the_owners_hooks().is_alone());
}

#[test]
fn only_the_hooks_consensflow_added_stay_when_a_launch_is_to_have_none_of_the_owners() {
    // What the launch added, as its config has them written (`ask::devin_hooks`).
    let session = json!({ "matcher": "", "hooks": [{ "type": "command", "command": "'/b/cf' hook devin-session", "timeout": 5 }] });
    let question = json!({ "matcher": "ask_user_question", "hooks": [{ "type": "command", "command": "cf hook devin", "timeout": 3600 }] });
    let added = json!({ "SessionStart": [session], "PreToolUse": [question] });
    let mut config = json!({
        "model": "kept",
        "hooks": {
            "SessionStart": [
                { "matcher": "", "hooks": [{ "type": "command", "command": "notify session" }] },
                session,
            ],
            "PreToolUse": [
                // An owner's that says the words of ConsensFlow's, and refuses every call: it
                // stayed, kept by a word in its command (Sol on the B1 fix, the second look).
                { "hooks": [{ "type": "command", "command": "echo 'calling cf hook devin' >&2; exit 2" }] },
                // An owner's entry that holds ConsensFlow's command beside its own.
                { "hooks": [{ "type": "command", "command": "audit" }, { "type": "command", "command": "cf hook devin" }] },
                { "matcher": "ask_user_question", "hooks": [{ "type": "command", "command": "other" }] },
                question,
            ],
            "PermissionRequest": [{ "hooks": [{ "type": "command", "command": "notify permission" }] }],
            "Stop": "not a list",
        },
    });
    consensflows_hooks_of(&mut config, &added);
    assert_eq!(config["model"], "kept");
    assert_eq!(config["hooks"], added);
    // An event that holds none of what was added goes, and one that is no list.
    let mut odd = json!({ "hooks": { "PreToolUse": [{ "matcher": "x" }, { "hooks": "audit" }] } });
    consensflows_hooks_of(&mut odd, &added);
    assert_eq!(odd, json!({ "hooks": {} }));
    // A config with no hooks is left as it is.
    let mut bare = json!({ "model": "kept" });
    consensflows_hooks_of(&mut bare, &added);
    assert_eq!(bare, json!({ "model": "kept" }));
}

#[test]
fn what_a_screen_printed_after_a_mark_is_counted_in_characters_not_bytes() {
    assert_eq!(after("héllo wörld", 3), "lo wörld");
    assert_eq!(after("abc", 3), "");
    assert_eq!(after("abc", 9), "");
    assert_eq!(after("abc", 0), "abc");
}

#[test]
fn claude_keeps_its_sessions_where_the_window_says_and_else_in_the_dot_claude_of_its_home() {
    let env = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };
    let home = Path::new("/machine").join(".claude");
    assert_eq!(claude_config_of(&env(&[("HOME", "/machine")])), home);
    assert_eq!(
        claude_config_of(&env(&[
            ("HOME", "/machine"),
            ("CLAUDE_CONFIG_DIR", "/elsewhere")
        ])),
        Path::new("/elsewhere")
    );
    // A variable set to nothing is not set, and the last of a name is the one.
    assert_eq!(
        claude_config_of(&env(&[
            ("HOME", "/other"),
            ("HOME", "/machine"),
            ("CLAUDE_CONFIG_DIR", "")
        ])),
        home
    );
    assert_eq!(claude_config_of(&[]), Path::new(".claude"));
}

#[test]
fn an_answer_is_on_the_screen_since_the_paste_or_in_an_answer_of_the_record() {
    let record = [
        item("user", "sum 5555 please"),
        item("assistant", "It is 5555."),
    ];
    assert!(answered("... 5555 ...", &[], "5555"));
    assert!(answered("", &record, "5555"));
    // The question holds the answer's digits, and is not the answer.
    assert!(!answered("", &record[..1], "5555"));
    assert!(!answered("nothing yet", &[], "5555"));
    // Drawn in two strokes, the screen's text splits it: the record has it whole.
    assert!(!answered("55 ⏎ 55", &[], "5555"));
    assert!(answered("55 ⏎ 55", &record, "5555"));
}

#[test]
fn a_screen_of_blanks_has_drawn_nothing_and_one_with_text_has_drawn_its_characters() {
    // What Windows' console host prints at once: a jump of the cursor, a line break
    // in the screen's text. Devin draws its first frame seconds after it.
    assert_eq!(drawn(""), 0);
    assert_eq!(drawn("\n"), 0);
    assert_eq!(drawn(" \n   \t\r\n "), 0);
    assert_eq!(drawn("\nDevin CLI  v3000\n"), 13);
    assert_eq!(drawn("é ü\n"), 2);
    // A window that printed only that has not held still with something printed.
    let tiny = Duration::from_millis(1);
    let hold = Duration::from_millis(30);
    assert!(!held_still(
        || drawn("\n"),
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    assert!(held_still(
        || drawn("\nDevin CLI\n"),
        || false,
        Duration::from_secs(10),
        hold,
        tiny
    ));
}

#[test]
fn a_window_has_held_still_when_its_output_stopped_growing_with_something_printed() {
    let tiny = Duration::from_millis(1);
    let hold = Duration::from_millis(30);
    let within = Duration::from_secs(10);
    assert!(held_still(|| 5, || false, within, hold, tiny));
    // Output that grows at every look never holds still, and the wait runs out.
    let grown = Cell::new(0);
    assert!(!held_still(
        || {
            grown.set(grown.get() + 1);
            grown.get()
        },
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    // Nothing printed is not a window that drew its prompt.
    assert!(!held_still(
        || 0,
        || false,
        Duration::from_millis(100),
        hold,
        tiny
    ));
    // A window that ended did not hold still.
    assert!(!held_still(|| 5, || true, within, hold, tiny));
    // It counts from the last change: output that stops growing is still after the hold.
    let grows_then_stops = Cell::new(0);
    assert!(held_still(
        || {
            grows_then_stops.set((grows_then_stops.get() + 1).min(5));
            grows_then_stops.get()
        },
        || false,
        within,
        hold,
        tiny
    ));
}

#[test]
fn a_probes_hooks_go_after_consensflows_own_in_the_launchs_one_settings_file() {
    let consensflows = json!({ "type": "command", "command": "cf hook stop" });
    let mut config = json!({
        "permissions": { "defaultMode": "bypassPermissions" },
        "hooks": { "Stop": [{ "hooks": [consensflows] }] },
    });
    let printing = json!({ "hooks": [{ "type": "command", "command": "echo compacted" }] });
    let again = json!({ "hooks": [{ "type": "command", "command": "echo stopped" }] });
    add_hooks(
        &mut config,
        &json!({ "PostCompact": [printing], "Stop": [again] }),
    );
    assert_eq!(
        config,
        json!({
            "permissions": { "defaultMode": "bypassPermissions" },
            "hooks": {
                "Stop": [{ "hooks": [consensflows] }, again],
                "PostCompact": [printing],
            },
        })
    );
    // A config with no hooks is given them.
    let mut bare = json!({});
    add_hooks(&mut bare, &json!({ "PostCompact": [printing] }));
    assert_eq!(bare, json!({ "hooks": { "PostCompact": [printing] } }));

    // The file the launch loads is written beside the app's, which stays as it was.
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("settings.json");
    crate::files::write(&file, config.to_string()).unwrap();
    let words = vec!["--settings".to_owned(), file.to_string_lossy().into_owned()];
    let loaded = with_hooks(&words, &json!({ "SessionStart": [printing] })).unwrap();
    assert_eq!(loaded[0], "--settings");
    assert_eq!(
        Path::new(&loaded[1]),
        folder.path().join("settings.hooks.json")
    );
    let written: Value =
        serde_json::from_str(&crate::files::read_string(Path::new(&loaded[1])).unwrap()).unwrap();
    assert_eq!(written["hooks"]["SessionStart"], json!([printing]));
    assert_eq!(
        crate::files::read_string(&file).unwrap(),
        config.to_string()
    );
    assert!(with_hooks(&words[..1], &json!({})).is_err());
}
