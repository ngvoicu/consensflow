//! What the stand-in Codex reads of the arguments it was started with. Its server
//! is given the developer instructions the daemon gives a window, as a `-c`
//! setting (`developer_instructions`): what a thread it starts begins with. Its TUI
//! is given the words that open it after those the broker adds (`--remote <url>
//! --remote-auth-token-env <name>`): the thread it resumes (`resume <thread>`) and
//! the message it begins its first turn with; and it asks to resume the thread as
//! the real one does. On Windows a window opens on Codex through the shim of an npm
//! install, which names the stand-in as program and script both, so the first
//! thing it is given there is its own file, which is none of its arguments.

use std::path::Path;

use serde_json::{json, Value};

/// Whether `arg` names the stand-in's own file (`own`), by its file name: the
/// script of the shim that opened it, where the system wraps a program in one.
fn is_own_file(arg: &str, own: Option<&Path>) -> bool {
    own.and_then(Path::file_name).is_some_and(|mine| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(mine))
    })
}

/// The arguments the stand-in was started with, without its own file where a shim
/// put it first.
pub fn asked<'a>(args: &'a [String], own: Option<&Path>) -> &'a [String] {
    match args.split_first() {
        Some((first, rest)) if is_own_file(first, own) => rest,
        _ => args,
    }
}

/// The developer instructions the server was given: the JSON string after
/// `-c developer_instructions=`, as the daemon writes it. None where it was given
/// none.
pub fn developer_instructions(args: &[String]) -> Option<String> {
    args.windows(2)
        .filter(|pair| pair[0] == "-c")
        .find_map(|pair| pair[1].strip_prefix("developer_instructions="))
        .and_then(|setting| serde_json::from_str(setting).ok())
}

/// What a TUI was asked to open, by the words after those the broker adds.
#[derive(Debug, PartialEq, Eq)]
pub struct Opening<'a> {
    /// The thread it resumes, the word after `resume`.
    pub resume: Option<&'a str>,
    /// The message it begins its first turn with: the word that is no flag and no
    /// value of one, nor the subcommand. None where it was given none.
    pub message: Option<&'a str>,
}

/// What the TUI started with `args` was asked to open.
pub fn opening(args: &[String]) -> Opening<'_> {
    let mut opening = Opening {
        resume: None,
        message: None,
    };
    let mut words = args.get(4..).unwrap_or_default().iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-c" | "--config" | "--enable" | "--disable" | "--model" | "-m" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            "resume" if opening.resume.is_none() => {
                opening.resume = words.next().map(String::as_str);
            }
            message => {
                opening.message = Some(message);
                break;
            }
        }
    }
    opening
}

/// The request that resumes `thread`, as Codex 0.159.2's TUI sent it (traced
/// 2026-10-03): every field there, the instructions null, as `codex resume
/// <thread>` has them: what a thread is given is what it began with.
pub fn resume_request(thread: &str) -> Value {
    json!({
        "threadId": thread,
        "history": null,
        "path": null,
        "model": null,
        "modelProvider": null,
        "serviceTier": null,
        "cwd": null,
        "runtimeWorkspaceRoots": null,
        "approvalPolicy": null,
        "approvalsReviewer": null,
        "sandbox": null,
        "permissions": null,
        "config": null,
        "baseInstructions": null,
        "developerInstructions": null,
        "personality": null,
        "excludeTurns": true,
        "initialTurnsPage": null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words a TUI is started with, the broker's four first.
    fn tui_words(after: &[&str]) -> Vec<String> {
        [
            "--remote",
            "ws://127.0.0.1:1",
            "--remote-auth-token-env",
            "CF_CODEX_TUI_TOKEN",
        ]
        .iter()
        .chain(after)
        .map(|word| (*word).to_owned())
        .collect()
    }

    #[test]
    fn a_tui_that_resumes_has_the_thread_after_resume_and_its_message_after_that() {
        let words = tui_words(&[
            "-c",
            "developer_instructions=\"a role\"",
            "--enable",
            "x",
            "resume",
            "01a09094-938f-7fd1-a2d3-315cf92b4559",
            "--model",
            "gpt",
            "-c",
            "model_reasoning_effort=\"low\"",
            "Go on.",
        ]);
        assert_eq!(
            opening(&words),
            Opening {
                resume: Some("01a09094-938f-7fd1-a2d3-315cf92b4559"),
                message: Some("Go on."),
            }
        );
        // Resumed with no message: the window opens on the thread and waits.
        let silent = tui_words(&["resume", "thread-1"]);
        assert_eq!(
            opening(&silent),
            Opening {
                resume: Some("thread-1"),
                message: None,
            }
        );
        // A resume that names no thread resumes none.
        assert_eq!(opening(&tui_words(&["resume"])).resume, None);
    }

    #[test]
    fn a_tui_that_does_not_resume_has_its_first_word_that_is_no_flag_for_its_message() {
        let words = tui_words(&["-c", "a=b", "--model", "gpt", "Do it.", "resume"]);
        assert_eq!(
            opening(&words),
            Opening {
                resume: None,
                message: Some("Do it."),
            }
        );
        assert_eq!(
            opening(&tui_words(&["-c", "a=b"])),
            Opening {
                resume: None,
                message: None,
            }
        );
        assert_eq!(opening(&[]), opening(&tui_words(&[])));
    }

    #[test]
    fn the_resume_the_tui_asks_for_is_what_codex_sent_with_its_instructions_null() {
        let request = resume_request("thread-1");
        assert_eq!(request["threadId"], "thread-1");
        for field in [
            "developerInstructions",
            "baseInstructions",
            "runtimeWorkspaceRoots",
        ] {
            assert!(request.get(field).is_some_and(Value::is_null), "{field}");
        }
        assert_eq!(request["excludeTurns"], true);
    }

    #[test]
    fn the_stand_in_a_shim_names_as_its_script_is_no_argument() {
        let words = |line: &str| -> Vec<String> { line.split(' ').map(str::to_owned).collect() };
        let own = Some(Path::new("/bin/fake-codex"));
        let args = words("/x/y/fake-codex queue --help");
        assert_eq!(asked(&args, own), ["queue", "--help"]);
        // However the system writes the name of the file.
        let args = words("C:/x/FAKE-CODEX app-server");
        assert_eq!(asked(&args, own), ["app-server"]);
        // A stand-in that does not know its own file takes nothing for it.
        let args = words("/x/y/fake-codex --version");
        assert_eq!(asked(&args, None), args.as_slice());
        // What it is asked with no shim is asked as it is.
        let args = words("--remote ws://127.0.0.1:1");
        assert_eq!(asked(&args, own), args.as_slice());
    }

    #[test]
    fn the_developer_instructions_a_server_was_given_are_the_json_string_of_its_setting() {
        let args: Vec<String> = [
            "-c",
            "developer_instructions=\"line one\\n\\nline \\\"two\\\"\"",
            "-c",
            "check_for_update_on_startup=false",
            "app-server",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(
            developer_instructions(&args).as_deref(),
            Some("line one\n\nline \"two\"")
        );
        assert_eq!(developer_instructions(&args[2..]), None);
        // Only a `-c` setting is one: the same word as an argument of another is not.
        let other = [
            "--model".to_owned(),
            "developer_instructions=\"no\"".to_owned(),
        ];
        assert_eq!(developer_instructions(&other), None);
    }
}
