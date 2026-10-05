use std::cell::RefCell;

use super::*;
use crate::testing::{finished, AnsweringHost};

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
