//! The questions a Devin chief's dialog holds, and where Devin keeps them.

use serde_json::json;

use super::*;

fn ask(id: &str, question: &str) -> String {
    json!({
        "role": "assistant",
        "tool_calls": [{
            "id": id,
            "name": "ask_user_question",
            "arguments": { "questions": [{ "question": question, "header": "File", "options": [{ "label": "a.html" }] }] },
        }],
    })
    .to_string()
}

fn answer(id: &str) -> String {
    json!({ "role": "tool", "tool_call_id": id, "content": "User answered your questions" })
        .to_string()
}

#[test]
fn the_dialog_is_open_when_the_newest_call_of_the_question_tool_has_no_answer_after_it() {
    // The newest message first, as the store is read.
    assert_eq!(open_dialog(&[]), None, "nothing asked yet");
    let asked = [
        ask("q2", "Which language?"),
        answer("q1"),
        ask("q1", "Which file?"),
    ];
    assert_eq!(
        open_dialog(&asked),
        Some(vec![
            json!({ "question": "Which language?", "header": "File", "options": [{ "label": "a.html" }] })
        ])
    );
    // An answer after the call closes it.
    let answered = [answer("q1"), ask("q1", "Which file?")];
    assert_eq!(open_dialog(&answered), None);
    // The newest call is the one that counts, whatever an older one was.
    let both = [
        answer("q2"),
        ask("q2", "Which language?"),
        ask("q1", "Which file?"),
    ];
    assert_eq!(open_dialog(&both), None);
}

#[test]
fn a_message_that_is_no_question_of_the_dialog_is_passed_over() {
    let other = json!({
        "role": "assistant",
        "tool_calls": [{ "id": "t1", "name": "bash", "arguments": { "command": "ls" } }],
    })
    .to_string();
    let user = json!({ "role": "user", "content": "go on" }).to_string();
    assert_eq!(open_dialog(&[other.clone(), user.clone()]), None);
    // A message that is not JSON is passed over too.
    let asked = [other, "not json".to_owned(), user, ask("q1", "Which file?")];
    assert_eq!(
        open_dialog(&asked).map(|questions| questions.len()),
        Some(1)
    );
}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[cfg(not(windows))]
#[test]
fn devin_keeps_its_data_in_the_xdg_place_below_the_home_unless_the_window_names_another() {
    assert_eq!(
        devin_data(&env(&[("HOME", "/home/u")])),
        PathBuf::from("/home/u/.local/share/devin")
    );
    assert_eq!(
        devin_data(&env(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "/data")])),
        PathBuf::from("/data/devin")
    );
    // The window's own environment: the last of a name is the one, and an empty one is none.
    assert_eq!(
        devin_data(&env(&[
            ("HOME", "/a"),
            ("HOME", "/b"),
            ("XDG_DATA_HOME", "")
        ])),
        PathBuf::from("/b/.local/share/devin")
    );
}

#[test]
fn on_windows_devin_keeps_it_under_the_roaming_app_data_which_the_window_says_it_is_on() {
    assert_eq!(
        devin_data(&env(&[
            ("OS", "Windows_NT"),
            ("APPDATA", "C:/Users/u/AppData/Roaming"),
            ("HOME", "C:/Users/u"),
        ])),
        PathBuf::from("C:/Users/u/AppData/Roaming/devin")
    );
    assert_eq!(
        devin_data(&env(&[("OS", "windows"), ("USERPROFILE", "C:/Users/u")])),
        PathBuf::from("C:/Users/u")
            .join("AppData")
            .join("Roaming")
            .join("devin")
    );
}
