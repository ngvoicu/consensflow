//! What a send refuses before the hand-over: nothing reached Pi, and nothing
//! is left in its inbox.

use std::path::Path;

use super::*;
use crate::testing::{finished, EPOCH_MS};

#[test]
fn a_claim_refused_is_said_in_the_hosts_words_with_nothing_left_in_the_inbox() {
    for (claim, cause) in [
        (
            json!({ "ok": false, "admitted": false, "error": "stale-generation", "cause": "gone" }),
            "gone",
        ),
        (
            json!({ "ok": false, "error": "paste-in-flight" }),
            "paste-in-flight",
        ),
        (json!({ "ok": false }), "claim-refused"),
        (json!({ "ok": 1 }), "claim-refused"),
        (json!({ "ok": "true", "error": "odd" }), "odd"),
        (json!(null), "claim-refused"),
        (json!([]), "claim-refused"),
        (json!({ "ok": false, "cause": "", "error": "x" }), ""),
        (
            json!({ "ok": false, "cause": null, "error": "later" }),
            "later",
        ),
        // Kept from Node on purpose: the host's words are text.
        (json!({ "ok": false, "cause": 0 }), "0"),
    ] {
        let mut stage = Stage::answering(Box::new({
            let claim = claim.clone();
            move || Ok(claim.clone())
        }));
        let answer = answered(stage.send("claim me"));
        assert_eq!(
            read(&answer),
            (false, true, Some(cause), Some("failed-with-zero-bytes")),
            "{claim}"
        );
        assert!(answer.zero_bytes);
        assert!(stage.names(&stage.inbox()).is_empty(), "{claim}");
    }
}

#[test]
fn a_claim_the_host_never_answered_is_a_transport_failure_before_the_hand_over() {
    let mut stage = Stage::answering(Box::new(|| {
        Err(HostError {
            error: Some("eof".to_owned()),
            message: "bridge ended".to_owned(),
        })
    }));
    let answer = answered(stage.send("lost"));
    assert_eq!(
        read(&answer),
        (false, true, Some("eof"), Some("failed-with-zero-bytes"))
    );
    assert!(stage.names(&stage.inbox()).is_empty());
}

#[test]
fn a_message_with_nothing_in_it_or_nowhere_to_go_is_refused_before_anything_is_drawn() {
    let mut stage = Stage::admitting();
    stage.begin(0, "", "native-pi-session", 1, 30_000);
    stage.begin(1, "text", "", 1, 30_000);
    let settled = stage.driver.run();
    assert_eq!(settled.len(), 2);
    for (_, answer) in settled {
        let answer = answer.unwrap();
        assert_eq!(read(&answer), (false, true, None, Some("invalid-record")));
        assert!(!answer.zero_bytes, "nothing was to be written");
    }
    assert!(stage.entropy.take_draws().is_empty());
    assert!(!Path::new(&stage.inbox()).exists());
}

#[test]
fn a_message_that_is_expired_the_moment_it_is_written_is_refused_with_its_bytes_said() {
    let mut stage = Stage::admitting();
    stage.begin(0, "late", "native-pi-session", 1, 0);
    let answer = answered(stage.driver.run());
    assert_eq!(read(&answer), (false, true, None, Some("expired")));
    assert!(answer.zero_bytes);
    assert_eq!(stage.entropy.take_draws(), [16], "the id is drawn first");
    assert!(!Path::new(&stage.inbox()).exists());
}

#[test]
fn a_pane_with_no_generation_is_refused_once_the_record_is_written_and_then_removed() {
    for generation in [0, 1 << 53] {
        let mut stage = Stage::admitting();
        stage.begin(0, "to no pane", "native-pi-session", generation, 30_000);
        let answer = answered(stage.driver.run());
        assert_eq!(
            read(&answer),
            (
                false,
                true,
                Some("native delivery needs pane {id, generation}"),
                Some("transport")
            )
        );
        assert!(answer.zero_bytes);
        assert!(stage.names(&stage.inbox()).is_empty());
        assert!(stage.host.asked.borrow().is_empty(), "no claim was made");
    }
    let mut stage = Stage::admitting();
    stage.begin(
        0,
        "the last one",
        "native-pi-session",
        (1 << 53) - 1,
        30_000,
    );
    assert!(stage.driver.run().is_empty());
    assert_eq!(stage.host.asked.borrow().len(), 1);
}

#[test]
fn a_system_that_will_not_give_randomness_is_a_failure_that_was_thrown() {
    struct Dry;
    impl Entropy for Dry {
        fn fill(&self, _: &mut [u8]) -> Result<(), String> {
            Err("no randomness".to_owned())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (inbox, ack) = (
        path::join(&[&dir.path().to_string_lossy(), "inbox"]),
        path::join(&[&dir.path().to_string_lossy(), "ack"]),
    );
    let host = AnsweringHost::new(|_| Ok(json!({ "ok": true })));
    let pane = Pane {
        id: "s1-zeus".to_owned(),
        generation: 1,
    };
    let target = Target {
        launch_id: LAUNCH,
        inbox: &inbox,
        ack: &ack,
        ack_timeout_ms: 30_000,
        session: "native-pi-session",
        pane: &pane,
        host: &host,
    };
    let time = ManualTime::new(EPOCH_MS);
    let sent = finished(Box::pin(send(&time, &Dry, &target, "text")));
    assert_eq!(sent, Err("no randomness".to_owned()));
    assert!(!Path::new(&inbox).exists());
}

#[cfg(unix)]
#[test]
fn a_folder_the_system_refuses_to_make_is_named_by_the_level_it_refused() {
    use std::os::unix::fs::PermissionsExt;
    // The promised `mkdir` names the level that failed, where the
    // synchronous one names the folder asked for: Probed on Node v26.8.1.
    let mut stage = Stage::admitting();
    let locked = path::join(&[&stage.dir.path().to_string_lossy(), "locked"]);
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    stage.inbox_at = Some(path::join(&[&locked, "a", "inbox"]));
    let answer = answered(stage.send("refused"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    // A user the mask does not bind (root) makes the folder, and is refused nothing.
    if !Path::new(&stage.inbox()).exists() {
        assert_eq!(
            answer.cause,
            Some(format!(
                "EACCES: permission denied, mkdir '{}'",
                path::join(&[&locked, "a"])
            ))
        );
    }
}

#[test]
fn a_record_that_cannot_be_written_is_a_failure_before_the_hand_over_in_node_s_words() {
    let mut stage = Stage::admitting();
    // A file where the inbox is, and then where the acknowledgements' is.
    fs::write(stage.inbox(), "x").unwrap();
    let answer = answered(stage.send("nowhere to put it"));
    assert_eq!(
        read(&answer),
        (
            false,
            true,
            Some(format!("EEXIST: file already exists, mkdir '{}'", stage.inbox()).as_str()),
            Some("transport")
        )
    );
    fs::remove_file(stage.inbox()).unwrap();
    fs::write(stage.ack(), "x").unwrap();
    let answer = answered(stage.send("nowhere to hear from it"));
    assert_eq!(
        answer.cause,
        Some(format!(
            "EEXIST: file already exists, mkdir '{}'",
            stage.ack()
        ))
    );
    assert!(answer.zero_bytes);
}
