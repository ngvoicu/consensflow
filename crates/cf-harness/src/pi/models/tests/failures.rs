//! What a read of the model list that fails or finds no model says, and what it must never say
//! or wait for (T-1264, the review T-1270): one start, Pi's own words from that start, names
//! and never values, and no file of Pi's opened that is not a plain file of a size a setup is.

use cf_process::Failed;

use super::standin::{answer, StandIn};
use super::*;
use crate::seams::processes::Streams;

/// Synthetic credentials, one of each kind a failure could carry. None is a real one.
const GIT_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
const ENV_TOKEN: &str = "sk-or-v1-SYNTHETICENVVALUE0123456789abcdef0123456789";

fn empty() -> String {
    answer("models", &[])
}

/// F3: a read that ends before it answers takes one start, and says what that start said and
/// the code it exited with. A second start would say something else, and run what a package
/// does at its start twice.
#[test]
fn a_read_that_ended_before_it_answered_is_one_start_and_says_what_that_start_said() {
    let pi = StandIn::new(&[], &[]);
    pi.child(&[], Ends::Itself);
    pi.says(
        "Error: Failed to load extension \"/p/e.ts\": boom\nHint: Start without extensions using \"pi -ne\".\n",
        Some(1),
    );
    // What a second start would say.
    pi.processes.always_answer(
        "pi --mode rpc --no-session --no-mcp --offline",
        Err(Failed {
            message: "Command failed: pi\nSECOND_UNRELATED_FAILURE".to_owned(),
            code: Some(1),
            killed: false,
            stdout: String::new(),
        }),
    );
    let why = pi.read(false).unwrap_err();
    assert!(pi.processes.take_ran().is_empty(), "no second start: {why}");
    let spawned = pi.processes.take_spawned();
    assert_eq!(spawned.len(), 1, "one start");
    assert_eq!(
        spawned[0].1,
        Streams::Conversation,
        "started to keep what Pi says and to end whatever it started"
    );
    assert!(
        why.starts_with(
            "pi --mode rpc ended before it answered (exit 1); it said:\nError: Failed to load extension"
        ),
        "{why}"
    );
    assert!(why.contains("boom") && why.contains("pi -ne"), "{why}");
    assert!(!why.contains("SECOND_UNRELATED"), "{why}");
}

/// F3: a read given up on at its bound says what Pi had said so far, and what it was loading.
#[test]
fn a_read_given_up_on_says_what_pi_had_said_so_far_and_names_what_it_was_loading() {
    let settings = r#"{"packages":["npm:@scope/slow-provider@1.2.3"]}"#;
    let pi = StandIn::new(&[("settings.json", settings)], &[]);
    pi.child(&[], Ends::Asked);
    pi.says("warming the provider's cache\n", None);
    let mut driver = pi.begin(false);
    assert!(driver.run().is_empty(), "it waits for Pi");
    assert!(pi.time.fire_next(i64::MAX), "the bound is armed");
    let why = settled(&mut driver).unwrap_err();
    assert!(pi.processes.take_ran().is_empty(), "no second start");
    // ConsensFlow's own words, with what Pi was loading by name, come first; Pi's words come
    // last, after the mark, so that they can be screened whole (the third reading, F2).
    assert_eq!(
        why,
        "pi --mode rpc did not answer within 30 s. Pi loads the packages and extensions it has \
         before it lists, and one of them may be waiting for something (packages \
         npm:@scope/slow-provider@1.2.3); it said:\nwarming the provider's cache"
    );
}

/// F2: a package source is shown without what could carry a credential: the user and password
/// of a URL, its query and its fragment. What names the package is kept.
#[test]
fn a_package_source_is_shown_without_its_userinfo_query_or_fragment() {
    let packages = serde_json::json!([
        format!("git:https://user:{GIT_TOKEN}@example.invalid/team/provider.git?token={GIT_TOKEN}#frag"),
        { "source": format!("git:https://{GIT_TOKEN}@example.invalid/o/r"), "extensions": ["a.ts"], "apiKey": GIT_TOKEN },
        format!("git:{GIT_TOKEN}@github.com/owner/repo@v2"),
        "npm:@scope/ok@1.2.3",
        "git:github.com/owner/repo@ref",
        "/Users/me/local-package"
    ]);
    let settings = serde_json::json!({ "packages": packages }).to_string();
    let pi = StandIn::new(&[("settings.json", &settings)], &[]);
    pi.child(&[empty()], Ends::Asked);
    let why = pi.read(false).unwrap_err();
    for leaked in [GIT_TOKEN, "user:", "token=", "#frag", "apiKey"] {
        assert!(!why.contains(leaked), "{leaked} in {why}");
    }
    for shown in [
        "git:https://example.invalid/team/provider.git",
        "git:https://example.invalid/o/r",
        "git:github.com/owner/repo@v2",
        "npm:@scope/ok@1.2.3",
        "git:github.com/owner/repo@ref",
        "/Users/me/local-package",
    ] {
        assert!(why.contains(shown), "{shown} in {why}");
    }
}

/// F2: a provider key in the environment is named by its provider in the words (not by the
/// name of its variable, which says "key" and "token": the engine withholds a line that does),
/// and its value is in none.
#[test]
fn a_provider_key_of_the_environment_is_named_and_its_value_is_never_said() {
    let pi = StandIn::new(
        &[],
        &[
            ("OPENROUTER_API_KEY", ENV_TOKEN),
            ("ANTHROPIC_OAUTH_TOKEN", ENV_TOKEN),
            ("GROQ_API_KEY", ""),
        ],
    );
    pi.child(&[empty()], Ends::Asked);
    let why = pi.read(false).unwrap_err();
    assert!(
        why.contains("a key in the environment for ANTHROPIC, OPENROUTER:"),
        "{why}"
    );
    assert!(
        !why.contains("API_KEY") && !why.contains("OAUTH_TOKEN"),
        "no variable is named: {why}"
    );
    assert!(!why.contains("GROQ"), "an empty one is none: {why}");
    assert!(
        !why.contains(ENV_TOKEN) && !why.contains("SYNTHETICENV"),
        "{why}"
    );
    assert!(!why.contains("no provider is set up"), "{why}");
}

/// F5a: an extension that `settings.json` declares, outside the folder Pi loads on its own, is a
/// provider that may be set up: the words never say there is none.
#[test]
fn an_extension_the_settings_declare_is_never_called_no_provider() {
    let settings = r#"{"extensions":["./prov.ts","+./pinned.ts","-./off.ts","!glob/*","builtin:llama.cpp","-builtin:mcp"]}"#;
    let pi = StandIn::new(&[("settings.json", settings)], &[]);
    pi.child(&[empty()], Ends::Asked);
    let why = pi.read(false).unwrap_err();
    assert!(!why.contains("no provider is set up"), "{why}");
    assert!(why.contains("extensions ./prov.ts, ./pinned.ts"), "{why}");
    for dropped in ["off.ts", "glob", "builtin"] {
        assert!(
            !why.contains(dropped),
            "{dropped} (an exclusion or a built-in) in {why}"
        );
    }
}

/// F4: a file of Pi's folder that is no plain file is not opened, and a setup that could not be
/// read is not called no provider. Run in a thread: a read that opens a pipe waits for ever.
#[cfg(unix)]
mod unread {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[allow(clippy::disallowed_methods)] // `mkfifo` makes the pipe: nothing else of std can.
    fn fifo(at: &std::path::Path) {
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(at)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
    }

    /// `read` of a stand-in whose `auth.json` is a pipe, within ten seconds of real time.
    fn with_a_pipe(lines: Vec<String>) -> Option<Result<ModelListing, String>> {
        let (tell, told) = mpsc::channel();
        // The thread is left behind if the read waits for the pipe: the test says so and ends.
        std::thread::spawn(move || {
            let pi = StandIn::new(&[], &[]);
            fifo(&pi.agent.join("auth.json"));
            pi.child(&lines, Ends::Asked);
            let _ = tell.send(pi.read(false));
        });
        told.recv_timeout(Duration::from_secs(10)).ok()
    }

    #[test]
    fn a_list_that_names_models_reads_no_file_of_pi_s_folder() {
        let read = with_a_pipe(vec![answer("models", &["openai/gpt-6-luna"])]);
        let listing = read
            .expect("the read ended: it opened no pipe")
            .expect("the models are listed");
        assert_eq!(listing.models.len(), 1);
    }

    #[test]
    fn a_pipe_where_a_file_should_be_is_not_opened_and_is_said_unread() {
        let read = with_a_pipe(vec![empty()]);
        let why = read
            .expect("the read ended: it opened no pipe")
            .unwrap_err();
        assert!(
            why.contains("could not read") && why.contains("the file of logins"),
            "{why}"
        );
        assert!(!why.contains("no provider is set up"), "{why}");
    }
}

/// F4: a file larger than a setup is is not read whole.
#[test]
fn a_file_larger_than_a_setup_is_not_read_and_is_said_unread() {
    let padding = " ".repeat(2 << 20);
    let auth = format!(r#"{{"openai":{{"type":"api_key"}}{padding}}}"#);
    let pi = StandIn::new(&[("auth.json", &auth)], &[]);
    pi.child(&[empty()], Ends::Asked);
    let why = pi.read(false).unwrap_err();
    assert!(
        why.contains("could not read") && why.contains("the file of logins"),
        "{why}"
    );
    assert!(!why.contains("a login for openai"), "{why}");
}

/// F4: a folder is read only when it is wanted: for a list that is empty, and for a read given
/// up on; not for a list that names models, nor for a Pi whose own words say why it failed.
#[test]
fn the_folder_is_read_only_for_a_list_with_no_model_and_a_read_given_up_on() {
    use super::super::super::setup::reads;

    let pi = StandIn::new(&[], &[]);
    pi.child(&[answer("models", &["openai/gpt-6-luna"])], Ends::Asked);
    pi.read(false).unwrap();
    assert_eq!(reads(), 0, "a list that names models");

    let pi = StandIn::new(&[], &[]);
    pi.child(&[], Ends::Itself);
    pi.says("Error: boom\n", Some(1));
    pi.read(false).unwrap_err();
    assert_eq!(reads(), 0, "Pi's own words say why it ended");

    let pi = StandIn::new(&[], &[]);
    pi.child(&[empty()], Ends::Asked);
    pi.read(false).unwrap_err();
    assert_eq!(reads(), 1, "an empty list");

    let pi = StandIn::new(&[], &[]);
    pi.child(&[], Ends::Asked);
    let mut driver = pi.begin(false);
    assert!(driver.run().is_empty());
    assert!(pi.time.fire_next(i64::MAX));
    settled(&mut driver).unwrap_err();
    assert_eq!(reads(), 2, "a read given up on names what Pi was loading");
}
