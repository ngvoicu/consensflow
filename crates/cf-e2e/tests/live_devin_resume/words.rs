//! What the check says to a window, and the words it looks for in what comes
//! back: the shape of the messages the daemon gives a member, the role a member's
//! session starts with, and the words of Devin's own that tell a tool call was not
//! run. The note a resumed window is told its turn was cut is the engine's own
//! (`live::ask::cut_note`), not a copy of it.

use serde_json::Value;

/// The role text a member's session starts with, as the session hook gives it
/// (`additionalContext`): short, and naming `cf ask`, which a member runs in its
/// shell as it runs any command.
pub const ROLE: &str = "# ConsensFlow worker\n\
You are a ConsensFlow worker. Do what each message asks, with your tools, and reply as it says.\n\
To ask your chief a question, run `cf ask \"<question>\"` in your shell.\n";

/// What Devin says of a tool call that did not run: three lines of its own, found
/// beside each other in its program (its permission dialog's), and the sentence the
/// field report that began this quotes. A word is looked for as it is, whatever its
/// case.
pub const MARKERS: [&str; 4] = [
    "User skipped this tool call",
    "Tool execution was rejected",
    "PermissionRequest hook denied tool",
    "User edited tool command",
];

/// The marker that says the user skipped the call.
pub const SKIPPED: &str = MARKERS[0];

/// The markers `text` holds, each with where it first is (a byte of `text`), in the
/// order the list has them.
pub fn find_markers(text: &str) -> Vec<(&'static str, usize)> {
    // ASCII only, so that a byte of the lowercase text is a byte of `text`.
    let lower = text.to_ascii_lowercase();
    MARKERS
        .into_iter()
        .filter_map(|marker| Some((marker, lower.find(&marker.to_ascii_lowercase())?)))
        .collect()
}

/// The header the daemon puts on a message it gives a window
/// (`cf-engine`'s `delivery_text`): `[ConsensFlow m-12 · T-3 · note from @chief]`,
/// then the words on the next line.
pub fn delivery(message: u32, task: u32, kind: &str, from: &str, body: &str) -> String {
    format!("[ConsensFlow m-{message} · T-{task} · {kind} from {from}]\n{body}")
}

/// The start of a header, which the window's record shows once the message
/// arrived: the space after the number keeps m-1 from being found in m-12.
pub fn marker_of(message: u32) -> String {
    format!("[ConsensFlow m-{message} ")
}

/// What a launch's config says, as a line: the names of its keys, and for its
/// hooks, each event with the matchers of its entries (`*` for one with none), both
/// in alphabetical order. Only names: the owner's config holds their settings and
/// secrets, and the report is not theirs to read.
pub fn config_summary(text: &str) -> String {
    let Ok(config) = serde_json::from_str::<Value>(text) else {
        return "its config is no JSON".to_owned();
    };
    let mut keys: Vec<&str> = config
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys().map(String::as_str))
        .collect();
    keys.sort_unstable();
    let mut hooks: Vec<String> = config["hooks"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(event, entries)| {
            let matchers: Vec<&str> = entries
                .as_array()
                .into_iter()
                .flatten()
                .map(|entry| entry["matcher"].as_str().unwrap_or(""))
                .map(|matcher| if matcher.is_empty() { "*" } else { matcher })
                .collect();
            format!("{event}[{}]", matchers.join(","))
        })
        .collect();
    hooks.sort();
    let said = |names: Vec<String>| {
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(" ")
        }
    };
    format!(
        "config keys {}; hooks {}",
        said(keys.iter().map(|key| (*key).to_owned()).collect()),
        said(hooks)
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_words_of_a_call_that_did_not_run_are_found_whatever_their_case_with_where_they_are() {
        assert!(find_markers("all well").is_empty());
        let screen = "a: Tool execution was rejected: User skipped this tool call";
        assert_eq!(
            find_markers(screen),
            [
                ("User skipped this tool call", 32),
                ("Tool execution was rejected", 3)
            ]
        );
        assert_eq!(
            find_markers("é permissionrequest HOOK DENIED TOOL"),
            [("PermissionRequest hook denied tool", 3)]
        );
        assert_eq!(SKIPPED, "User skipped this tool call");
    }

    #[test]
    fn a_message_has_the_header_the_daemon_gives_it_and_is_known_by_the_start_of_it() {
        assert_eq!(
            delivery(10, 1, "note", "@chief", "Run it"),
            "[ConsensFlow m-10 · T-1 · note from @chief]\nRun it"
        );
        assert_eq!(marker_of(10), "[ConsensFlow m-10 ");
        // The number's end is in the marker: m-1 is not found in m-12.
        assert!(!delivery(12, 1, "note", "@chief", "x").contains(&marker_of(1)));
    }

    #[test]
    fn the_role_a_member_starts_with_names_the_command_it_is_asked_to_run() {
        assert!(ROLE.starts_with("# ConsensFlow worker\n"));
        assert!(ROLE.contains("cf ask"));
    }

    #[test]
    fn a_config_is_summed_up_by_the_names_of_its_keys_and_its_hooks_and_nothing_it_says() {
        let text = json!({
            "model": "secret-model",
            "hooks": {
                "SessionStart": [{ "matcher": "", "hooks": [] }],
                "PreToolUse": [{ "matcher": "ask_user_question", "hooks": [] }, { "hooks": [] }],
            },
            "auto_update": false,
        })
        .to_string();
        let said = config_summary(&text);
        assert_eq!(
            said,
            "config keys auto_update hooks model; hooks PreToolUse[ask_user_question,*] SessionStart[*]"
        );
        assert!(!said.contains("secret-model"));
        assert_eq!(config_summary("{}"), "config keys none; hooks none");
        assert_eq!(config_summary("not json"), "its config is no JSON");
    }
}
