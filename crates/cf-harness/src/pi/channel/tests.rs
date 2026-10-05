//! A launch's channel: the folders and environment its window is opened
//! with, and what the extension says of the conversation its window shows.

use std::fs;

use super::*;

const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";

fn launch() -> LaunchId {
    LaunchId::new(LAUNCH).unwrap()
}

/// A channel whose evidence is in a folder of its own, which names the launch.
fn channel_in(settled: &Path) -> Channel {
    Channel {
        launch_id: LAUNCH.to_owned(),
        inbox: "inbox".to_owned(),
        ack: "ack".to_owned(),
        settled: settled.to_string_lossy().into_owned(),
    }
}

#[test]
fn a_window_is_told_its_folders_in_the_order_node_told_them_and_to_load_the_extension() {
    let env = Env::from_vars([("CONSENSFLOW_HOME", "/home/me/cf")]);
    let launched = launch_configuration(&env, &launch(), "/work/app", "/ext/pi.mjs").unwrap();
    assert_eq!(launched.args, ["--extension", "/ext/pi.mjs"]);
    let root = path::join(&["/home/me/cf", "integrations", "pi", LAUNCH]);
    let folder = |name: &str| path::join(&[&root, name]);
    let told: Vec<(&str, String)> = launched
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    assert_eq!(
        told,
        [
            ("CF_DELIVERY_INBOX", folder("inbox")),
            ("CF_DELIVERY_ACK", folder("ack")),
            ("CF_DELIVERY_QUARANTINE", folder("quarantine")),
            ("CF_DELIVERY_SETTLED", folder("settled")),
            ("CF_DELIVERY_EXPIRED", folder("expired")),
            ("CF_DELIVERY_LAUNCH_ID", LAUNCH.to_owned()),
        ]
    );
    let channel = launched.channel;
    assert_eq!(channel.launch_id, LAUNCH);
    assert_eq!(
        (channel.inbox, channel.ack, channel.settled),
        (folder("inbox"), folder("ack"), folder("settled"))
    );
    assert!(!Path::new(&root).exists(), "no folder is made here");
}

#[test]
fn a_launch_with_no_workspace_or_no_home_has_no_channel() {
    let env = Env::from_vars([("CONSENSFLOW_HOME", "/home/me/cf")]);
    assert_eq!(
        launch_configuration(&env, &launch(), "", "/ext/pi.mjs").err(),
        Some("launch configuration needs a workspace".to_owned())
    );
    // Kept from Node on purpose: an environment that names no home fails,
    // where Node read the process's own.
    assert_eq!(
        launch_configuration(&Env::default(), &launch(), "/work/app", "/ext/pi.mjs").err(),
        Some("missing home in env".to_owned())
    );
}

#[test]
fn a_message_is_addressed_to_the_launch_s_inbox_with_thirty_seconds_to_be_answered() {
    let channel = channel_in(Path::new("settled"));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 2,
    };
    let host = crate::testing::AnsweringHost::new(|_| Ok(serde_json::json!({ "ok": true })));
    let target = channel.target("cf-1-zeus-0000abcd", &pane, &host);
    assert_eq!(
        (target.launch_id, target.inbox, target.ack, target.session),
        (LAUNCH, "inbox", "ack", "cf-1-zeus-0000abcd")
    );
    assert_eq!(target.ack_timeout_ms, 30_000);
}

/// What the extension writes at `settled`, in the file `name`.
fn put(settled: &Path, name: &str, text: &str) {
    fs::create_dir_all(settled).unwrap();
    fs::write(settled.join(name), text).unwrap();
}

fn shown(channel: &Channel) -> Result<Option<String>, String> {
    channel.shown_session()
}

#[test]
fn the_conversation_the_extension_says_the_window_shows_is_its_own_launch_s_and_a_text() {
    let dir = tempfile::tempdir().unwrap();
    let channel = channel_in(&dir.path().join("settled"));
    let file = format!("{LAUNCH}.shown.json");
    assert_eq!(shown(&channel), Ok(None), "before it has said");
    let said = |text: &str| {
        put(&dir.path().join("settled"), &file, text);
        shown(&channel)
    };
    assert_eq!(
        said(&format!(r#"{{"launchId":"{LAUNCH}","sessionId":"s-1"}}"#)),
        Ok(Some("s-1".to_owned()))
    );
    // An empty name is a name: the window shows a conversation of none.
    assert_eq!(
        said(&format!(r#"{{"launchId":"{LAUNCH}","sessionId":""}}"#)),
        Ok(Some(String::new()))
    );
    for nothing in [
        "",
        "{",
        "null",
        "[]",
        "\"s-1\"",
        "\u{feff}{}",
        r#"{"launchId":"other","sessionId":"s-1"}"#,
        r#"{"launchId":7,"sessionId":"s-1"}"#,
        &format!(r#"{{"launchId":"{LAUNCH}"}}"#),
        &format!(r#"{{"launchId":"{LAUNCH}","sessionId":7}}"#),
        &format!(r#"{{"launchId":"{LAUNCH}","sessionId":null}}"#),
    ] {
        assert_eq!(said(nothing), Ok(None), "{nothing:?}");
    }
}

#[test]
fn a_shown_file_the_system_will_not_read_is_a_failure_in_node_s_words() {
    let dir = tempfile::tempdir().unwrap();
    let settled = dir.path().join("settled");
    let channel = channel_in(&settled);
    let file = path::join(&[&channel.settled, &format!("{LAUNCH}.shown.json")]);
    // A folder where the file is: Probed on Node v26.8.1, the promised
    // `readFile` of a folder names it.
    fs::create_dir_all(&file).unwrap();
    assert_eq!(
        shown(&channel),
        Err(format!(
            "EISDIR: illegal operation on a directory, read '{file}'"
        ))
    );
    // A file where the folder is. Windows says no such file there, which is
    // no failure.
    fs::remove_dir_all(&settled).unwrap();
    fs::write(&settled, "x").unwrap();
    #[cfg(unix)]
    assert_eq!(
        shown(&channel),
        Err(format!("ENOTDIR: not a directory, open '{file}'"))
    );
}
