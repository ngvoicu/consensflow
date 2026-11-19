//! What each verdict of the extension, and each failure after the hand-over,
//! make of the answer: Pi's own word, or uncertain.

use std::cell::RefCell;

use super::*;

#[test]
fn t1813_pi_hands_over_at_inbox_publication_and_settles_its_verdict() {
    let mut stage = Stage::admitting();
    assert!(stage.send("first").is_empty());
    assert_eq!(stage.host.handovers.get(), 1, "the daemon sent the brief");
    assert_eq!(
        stage.host.reports.get(),
        0,
        "the extension has not acknowledged it"
    );
    stage.acknowledges(&json!({ "admitted": false, "bytesWritten": 0 }));
    assert!(!stage.until_answered().ok);
    assert_eq!(stage.host.send_failures.get(), 1);
    assert_eq!(stage.host.reports.get(), 0, "a refusal is no take");

    let mut stage = Stage::admitting();
    assert!(stage.send("uncertain").is_empty());
    stage.acknowledges(&json!({ "admitted": null }));
    assert!(!stage.until_answered().ok);
    assert_eq!(stage.host.send_unknowns.get(), 1);
    assert_eq!(
        stage.host.reports.get(),
        0,
        "an uncertain take is not a report"
    );

    let mut stage = Stage::admitting();
    assert!(stage.send("second").is_empty());
    assert_eq!(stage.host.reports.get(), 0);
    stage.acknowledges(&json!({ "admitted": true, "mode": "tui" }));
    assert!(stage.until_answered().ok);
    assert_eq!(
        stage.host.reports.get(),
        1,
        "the extension reported its take"
    );
}

#[test]
fn what_each_verdict_says_is_what_the_answer_says() {
    let ack = |fields: Value| {
        let mut ack = json!({ "id": "m-x" });
        ack.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        ack
    };
    let said = |fields: Value| Answer::from_ack(Some(ack(fields)));
    let unknown = (false, false, Some("admission-unknown"), Some("uncertain"));
    assert_eq!(read(&Answer::from_ack(None)), unknown);
    assert_eq!(Answer::from_ack(None).ack, None);
    assert_eq!(read(&said(json!({ "admitted": null }))), unknown);
    let invalid = (false, false, Some("invalid-admission"), Some("uncertain"));
    for admitted in [json!("yes"), json!(1), json!([]), json!({})] {
        assert_eq!(read(&said(json!({ "admitted": admitted }))), invalid);
    }
    assert_eq!(read(&said(json!({}))), invalid);
    // Refused before it was sent: nothing reached Pi; its reason is the
    // extension's own, in the acknowledgement.
    let refused = said(json!({ "admitted": false, "bytesWritten": 0, "reason": "wrong-launch" }));
    assert_eq!(
        read(&refused),
        (false, true, None, Some("failed-with-zero-bytes"))
    );
    assert!(refused.zero_bytes);
    for bytes in [json!(0.0), json!(-0.0), json!(0)] {
        assert!(said(json!({ "admitted": false, "bytesWritten": bytes })).zero_bytes);
    }
    for bytes in [json!(1), json!("0"), json!(null), json!(false)] {
        assert!(!said(json!({ "admitted": false, "bytesWritten": bytes })).zero_bytes);
    }
    assert!(!said(json!({ "admitted": false })).zero_bytes);
    let taken = said(json!({ "admitted": true, "mode": "tui" }));
    assert_eq!(read(&taken), (true, false, None, None));
    assert_eq!(
        taken.ack,
        Some(ack(json!({ "admitted": true, "mode": "tui" })))
    );
}

#[test]
fn what_is_no_verdict_of_this_message_is_not_taken_for_one() {
    let mut stage = Stage::admitting();
    assert!(stage.send("wait for it").is_empty());
    let name = stage.names(&stage.inbox()).remove(0);
    fs::create_dir_all(stage.ack()).unwrap();
    let verdict = path::join(&[&stage.ack(), &name]);
    for text in [
        r#"{"id":"m-someone-else","admitted":true}"#,
        r#"{"id": "m-"#,
        "",
        "\"a string\"",
        "null",
        "[]",
        r#"{"id":7,"admitted":true}"#,
        r#"{"admitted":true}"#,
    ] {
        fs::write(&verdict, text).unwrap();
        assert!(stage.fire(10).unwrap().is_empty(), "{text}");
    }
    stage.acknowledges(&json!({ "admitted": true, "mode": "tui" }));
    assert!(answered(stage.fire(10).unwrap()).ok);
}

#[test]
fn a_verdict_json_here_cannot_hold_is_no_verdict_where_node_read_it() {
    // Kept from Node on purpose: JSON that holds a number past a double's
    // range, or nests past 127 levels, says nothing here, where Node read
    // it. The extension writes neither.
    let mut stage = Stage::admitting();
    assert!(stage.send("wait for it").is_empty());
    let name = stage.names(&stage.inbox()).remove(0);
    let id = name.strip_suffix(".json").unwrap();
    fs::create_dir_all(stage.ack()).unwrap();
    let verdict = path::join(&[&stage.ack(), &name]);
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    for text in [
        format!(r#"{{"id":"{id}","admitted":true,"big":1e400}}"#),
        format!(r#"{{"id":"{id}","admitted":true,"deep":{deep}}}"#),
    ] {
        fs::write(&verdict, text).unwrap();
        assert!(stage.fire(10).unwrap().is_empty());
    }
}

#[test]
fn a_verdict_the_system_will_not_let_be_read_is_uncertain_and_the_record_stays() {
    let mut stage = Stage::admitting();
    assert!(stage.send("unreadable").is_empty());
    let name = stage.names(&stage.inbox()).remove(0);
    // A folder where the verdict is written.
    let verdict = path::join(&[&stage.ack(), &name]);
    fs::create_dir_all(&verdict).unwrap();
    let answer = answered(stage.fire(10).unwrap());
    assert_eq!((answer.ok, answer.admitted), (false, None));
    assert_eq!(
        answer.cause,
        Some(format!(
            "EISDIR: illegal operation on a directory, read '{verdict}'"
        ))
    );
    assert_eq!(stage.names(&stage.inbox()), [name], "Pi may be taking it");
}

#[test]
fn a_record_nobody_took_that_cannot_be_taken_out_is_uncertain_in_node_s_words() {
    let mut stage = Stage::admitting();
    stage.begin(0, "stuck", "native-pi-session", 1, 20);
    assert!(stage.driver.run().is_empty());
    let name = stage.names(&stage.inbox()).remove(0);
    let file = path::join(&[&stage.inbox(), &name]);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let answer = stage.until_answered();
    assert_eq!((answer.ok, answer.admitted), (false, None));
    assert_eq!(
        answer.cause,
        Some(format!(
            "Path is a directory: rm returned EISDIR (is a directory) {file}"
        ))
    );
}

#[test]
fn a_failed_inbox_publish_is_a_definite_failed_send() {
    // The host takes the record away as it claims: the rename then fails.
    let inbox = Rc::new(RefCell::new(String::new()));
    let seen = Rc::clone(&inbox);
    let mut stage = Stage::answering(Box::new(move || {
        let folder = seen.borrow().clone();
        for entry in fs::read_dir(folder).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        Ok(json!({ "ok": true }))
    }));
    *inbox.borrow_mut() = stage.inbox();
    let answer = answered(stage.send("vanishes"));
    assert_eq!(
        (answer.ok, answer.admitted, answer.error),
        (false, Some(false), Some("transport"))
    );
    let cause = answer.cause.unwrap();
    assert!(
        cause.starts_with("ENOENT: no such file or directory, rename '")
            && cause.contains(".json.tmp' -> '"),
        "{cause}"
    );
    assert!(answer.zero_bytes && answer.ack.is_none());
    assert_eq!(stage.host.send_failures.get(), 1);
}
