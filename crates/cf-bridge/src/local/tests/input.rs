//! `Bridge input robustness` and `Bridge multibyte input`: what the reader
//! does with lines that are broken, split, or not text.

use std::time::Duration;

use cf_proto::bridge::Role;
use serde_json::json;

use super::wire::{collector, frame, lonely, lonely_with, quiet, run, wait_for, within};
use crate::local::BridgeBuilder;
use crate::BridgeError;

#[test]
fn reports_a_malformed_line_through_on_error_and_keeps_going() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send_text("not json at all\n").await;
        wire.send(frame("req", "r-1", "ping", json!({}))).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0]["body"],
            json!({ "ok": true })
        );
        wait_for(|| !errors.borrow().is_empty()).await;
        assert!(errors.borrow()[0].starts_with("malformed frame:"));
    });
}

#[test]
fn reports_a_frame_of_the_wrong_shape_with_its_text() {
    run(async {
        let (errors, on_error) = collector();
        let (_bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        let wrong_version = r#"{"v":2,"id":"r-1","kind":"req","op":"ping","body":{}}"#;
        let no_id = r#"{"v":1,"id":"","kind":"req","op":"ping","body":{}}"#;
        let wrong_kind = r#"{"v":1,"id":"r-1","kind":"event","op":"ping","body":{}}"#;
        let not_an_object = "[1, 2]";
        for line in [wrong_version, no_id, wrong_kind, not_an_object] {
            wire.send_text(&format!("{line}\n")).await;
        }
        wait_for(|| errors.borrow().len() == 4).await;
        assert_eq!(
            errors.borrow()[0],
            format!("malformed frame: {wrong_version}")
        );
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn keeps_buffered_bytes_when_junk_and_a_frame_arrive_in_one_chunk() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send_text(&format!(
            "junk before the first newline\n{}\n",
            frame("req", "r-5", "ping", json!({}))
        ))
        .await;
        assert_eq!(
            wire.wait_for_frames(1).await[0],
            frame("res", "r-5", "ping", json!({ "ok": true }))
        );
        wait_for(|| errors.borrow().len() == 1).await;
    });
}

#[test]
fn keeps_a_frame_split_across_two_chunks() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        let line = format!("{}\n", frame("req", "r-9", "ping", json!({})));
        wire.send_text(&line[..10]).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(wire.text(), "");
        wire.send_text(&line[10..]).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0],
            frame("res", "r-9", "ping", json!({ "ok": true }))
        );
    });
}

#[test]
fn rejects_every_outstanding_request_with_eof_and_marks_itself_closed() {
    run(async {
        let (bridge, mut wire) = lonely();
        let first = bridge.request("hangs", json!({}), None);
        let second = bridge.request("hangs-too", json!({}), Some(Duration::from_secs(10)));
        wire.end();
        assert_eq!(within(first).await, Err(BridgeError::Eof));
        assert_eq!(within(second).await, Err(BridgeError::Eof));
        assert!(bridge.closed());
        assert_eq!(
            within(bridge.request("late", json!({}), None)).await,
            Err(BridgeError::Eof)
        );
    });
}

#[test]
fn the_end_of_the_input_is_the_normal_end_and_nothing_is_reported_of_it() {
    run(async {
        let (errors, on_error) = collector();
        let (fatal, on_fatal) = collector();
        let (bridge, mut wire) = lonely_with(
            BridgeBuilder::new(Role::Daemon)
                .on_error(on_error)
                .on_fatal(on_fatal),
        );
        let pending = bridge.request("hangs", json!({}), None);
        wire.end();
        assert_eq!(within(pending).await, Err(BridgeError::Eof));
        quiet().await;
        assert!(errors.borrow().is_empty() && fatal.borrow().is_empty());
    });
}

#[test]
fn the_end_of_the_input_leaves_the_output_as_it_is() {
    run(async {
        let (bridge, mut wire) = lonely();
        wire.end();
        wait_for(|| bridge.closed()).await;
        quiet().await;
        assert!(!wire.output_ended());
        assert!(!bridge.event("late", json!({})));
    });
}

#[test]
fn drops_an_unterminated_line_at_the_end_of_the_input_without_a_word() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send_text(&frame("req", "r-1", "ping", json!({})).to_string())
            .await;
        wire.end();
        wait_for(|| bridge.closed()).await;
        assert!(errors.borrow().is_empty());
        assert_eq!(wire.text(), "");
    });
}

#[test]
fn hears_nothing_after_the_input_ended() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send(frame("req", "r-1", "ping", json!({}))).await;
        wire.end();
        wait_for(|| bridge.closed()).await;
        assert_eq!(
            wire.frames().len(),
            1,
            "what arrived before the end is served"
        );
    });
}

#[test]
fn skips_empty_lines_and_takes_the_carriage_return_off_a_line() {
    run(async {
        let (errors, on_error) = collector();
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).on_error(on_error));
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        wire.send_text(&format!(
            "\n\r\n{}\r\n\n",
            frame("req", "r-1", "ping", json!({}))
        ))
        .await;
        assert_eq!(wire.wait_for_frames(1).await.len(), 1);
        quiet().await;
        assert!(errors.borrow().is_empty());
    });
}

#[test]
fn writes_nothing_but_frames_to_the_output_stream() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
        let pending = bridge.request("hangs", json!({}), Some(Duration::from_millis(30)));
        bridge.event("note", json!({ "n": 2 }));
        wire.send(frame("req", "r-1", "ping", json!({}))).await;
        wire.send(frame("req", "r-2", "unknown", json!({}))).await;
        wire.send_text("junk\n").await;
        within(pending).await.unwrap();
        wire.wait_for_frames(4).await;
        let text = wire.text();
        assert!(text.ends_with('\n'));
        for line in text.lines() {
            let frame: serde_json::Value = serde_json::from_str(line).unwrap();
            let object = frame.as_object().unwrap();
            assert_eq!(object["v"], 1);
            assert!(["req", "res", "evt"].contains(&object["kind"].as_str().unwrap()));
            assert!(object["id"].is_string() && object["op"].is_string());
            assert!(object.contains_key("body"));
            assert_eq!(object.len(), 5);
        }
        assert_eq!(text.lines().count(), 4);
    });
}

#[test]
fn decodes_a_multibyte_character_split_at_every_byte_boundary() {
    run(async {
        let body = json!({ "text": "héllo 🌊 世界" });
        let bytes = format!("{}\n", frame("req", "r-1", "echo", body.clone())).into_bytes();
        for at in 1..bytes.len() - 1 {
            let (bridge, mut wire) = lonely();
            bridge.on("echo", |_, echoed| async move { Ok(echoed) });
            wire.send_bytes(&bytes[..at]).await;
            quiet().await;
            wire.send_bytes(&bytes[at..]).await;
            assert_eq!(
                wire.wait_for_frames(1).await[0]["body"],
                body,
                "split at {at}"
            );
            bridge.close();
        }
    });
}

#[test]
fn reads_text_that_is_not_utf_8_as_node_did_with_replacement_characters() {
    run(async {
        let (bridge, mut wire) = lonely();
        bridge.on("echo", |_, echoed| async move { Ok(echoed) });
        let mut line = br#"{"v":1,"id":"r-1","kind":"req","op":"echo","body":"a"#.to_vec();
        line.extend_from_slice(&[0xff, 0xfe]);
        line.extend_from_slice(b"b\"}\n");
        wire.send_bytes(&line).await;
        assert_eq!(
            wire.wait_for_frames(1).await[0]["body"],
            json!("a\u{fffd}\u{fffd}b")
        );
    });
}

#[test]
fn a_line_that_grows_past_the_limit_once_its_bytes_are_replaced_is_refused() {
    run(async {
        let (bridge, mut wire) = lonely_with(BridgeBuilder::new(Role::Daemon).max_frame_bytes(89));
        bridge.on("echo", |_, echoed| async move { Ok(echoed) });
        // The frame is 53 bytes with an empty body, and each invalid byte is
        // one byte on the wire and three once replaced: twelve of them make it
        // 65 bytes that read as 89, which fits, and thirteen make it 66 that
        // read as 92, which does not.
        let open = br#"{"v":1,"id":"r-1","kind":"req","op":"echo","body":""#;
        let mut fits = open.to_vec();
        fits.extend_from_slice(&[0xff; 12]);
        fits.extend_from_slice(b"\"}\n");
        let mut over = open.to_vec();
        over.extend_from_slice(&[0xff; 13]);
        over.extend_from_slice(b"\"}\n");
        wire.send_bytes(&fits).await;
        wire.send_bytes(&over).await;
        let frames = wire.wait_for_frames(2).await;
        assert_eq!(frames[0]["body"], json!("\u{fffd}".repeat(12)));
        assert_eq!(
            frames[1]["body"],
            json!({ "ok": false, "error": "too-large" })
        );
    });
}
