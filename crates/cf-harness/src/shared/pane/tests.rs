use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::contract::Admission;
use crate::shared::admission::admission;
use crate::testing::{finished, paste_answers, Answer, AnsweringHost, Driver, ScriptedHost};

fn pane() -> Pane {
    Pane {
        id: "s1-zeus".to_owned(),
        generation: 7,
    }
}

/// What a claim of the pane says, asked of `host`.
fn claimed(host: &dyn PaneHost) -> Value {
    finished(Box::pin(claim(host, &pane())))
}

#[test]
fn a_claim_names_its_pane_pane_and_returns_the_hosts_answer_as_it_came() {
    let answer = RefCell::new(Ok(json!({ "ok": true })));
    let host = AnsweringHost::new(|_| answer.borrow().clone());
    assert_eq!(claimed(&host), json!({ "ok": true }));
    assert_eq!(
        *host.asked.borrow(),
        [(
            "pane.claim".to_owned(),
            json!({ "pane": "s1-zeus", "generation": 7 })
        )],
        "the pane's id is `pane` in a claim, where a paste names it `id`"
    );
    let refused = json!({
        "ok": false, "admitted": false, "error": "stale-generation", "cause": "gone",
    });
    *answer.borrow_mut() = Ok(refused.clone());
    assert_eq!(claimed(&host), refused);
    *answer.borrow_mut() = Ok(json!(null));
    assert_eq!(claimed(&host), json!(null));
}

#[test]
fn keys_go_in_as_the_daemons_own_and_a_refusal_is_said_in_the_hosts_word_else_the_bridges() {
    let answer = RefCell::new(Ok(json!({ "ok": true })));
    let host = AnsweringHost::new(|_| answer.borrow().clone());
    let pressed = |host: &dyn PaneHost| finished(Box::pin(write_keys(host, &pane(), &[0x15])));
    assert_eq!(pressed(&host), Ok(()));
    assert_eq!(
        *host.asked.borrow(),
        [(
            "pane.input".to_owned(),
            json!({ "id": "s1-zeus", "generation": 7, "bytes": [21] })
        )]
    );
    *answer.borrow_mut() = Ok(json!({ "ok": false, "error": "stale", "cause": "stale pane" }));
    assert_eq!(pressed(&host), Err("stale pane".to_owned()));
    *answer.borrow_mut() = Ok(json!({ "ok": false, "error": "stale" }));
    assert_eq!(pressed(&host), Err("stale".to_owned()));
    *answer.borrow_mut() = Ok(json!({ "ok": false }));
    assert_eq!(
        pressed(&host),
        Err("the window refused the keys".to_owned())
    );
    *answer.borrow_mut() = Err(HostError {
        error: Some("eof".to_owned()),
        message: "bridge ended".to_owned(),
    });
    assert_eq!(pressed(&host), Err("bridge ended".to_owned()));
}

#[test]
fn a_paste_into_a_window_is_read_as_the_delivery_contract_has_it_whatever_the_host_does() {
    // Claude Code's channel: every way a pane host can answer a paste (the
    // refusal before a byte, an error after bytes, the bridge's deadline, the
    // host gone, the paste written) and how the adapters read what it says.
    for (way, answer, read) in paste_answers() {
        let host = AnsweringHost::new(move |_| answer.clone());
        let sent = finished(Box::pin(write_paste(
            &host,
            &pane(),
            "the cache key is per conversation",
        )));
        assert_eq!(admission(&sent, "refused", false), read, "{way}");
        assert_eq!(
            *host.asked.borrow(),
            [(
                "pane.write_paste".to_owned(),
                json!({
                    "id": "s1-zeus",
                    "generation": 7,
                    "body": "the cache key is per conversation",
                })
            )],
            "{way}: one paste, as the human would type it"
        );
    }
}

/// Both reviews of 2026-10-16, round 4: the engine's last look at a closed
/// project came before an adapter's own waits (Claude's keys that clear its input
/// box). A paste asks the host whether it admits it as it is asked for.
#[test]
fn a_paste_the_host_no_longer_admits_is_refused_unwritten_and_never_asked_of_the_pane() {
    let host = ScriptedHost::default();
    host.refuse_hand_overs();
    let sent = finished(Box::pin(write_paste(&host, &pane(), "hello")));
    assert_eq!(
        admission(&sent, "refused", false),
        Admission::Refused {
            reason: NOT_ADMITTED.to_owned()
        }
    );
    assert!(host.take_asked().is_empty(), "the pane was asked nothing");
}

/// A claim is no hand-over, and is not the last wait of every native send (one
/// over loopback waits for its connection after it): whether the host still
/// admits the send is asked by each send at its own last wait (Astra's review of
/// 2026-10-16, round 5).
#[test]
fn a_claim_returns_the_hosts_answer_and_asks_nothing_of_whether_it_admits() {
    let host = Rc::new(ScriptedHost::default());
    host.answer("pane.claim", [Answer::Held]);
    let mut driver = Driver::default();
    let claiming = Rc::clone(&host);
    driver.begin(0, async move { claim(&*claiming, &pane()).await });
    assert!(driver.run().is_empty(), "the claim waits for the host");
    host.refuse_hand_overs();
    assert!(host.release("pane.claim", Ok(json!({ "ok": true }))));
    assert_eq!(driver.run(), [(0, json!({ "ok": true }))]);
    assert_eq!(host.admissions(), 0);
}

#[test]
fn a_request_the_host_never_answered_is_a_transport_answer_in_the_hosts_word_else_its_message() {
    let error = |error: Option<&str>| {
        let failed = HostError {
            error: error.map(str::to_owned),
            message: "bridge ended".to_owned(),
        };
        claimed(&AnsweringHost::new(|_| Err(failed.clone())))
    };
    let transport = |cause: &str| json!({ "ok": false, "error": "transport", "cause": cause });
    assert_eq!(error(Some("eof")), transport("eof"));
    assert_eq!(error(None), transport("bridge ended"));
    // An empty word is a word, as `??` takes it.
    assert_eq!(error(Some("")), transport(""));
}
