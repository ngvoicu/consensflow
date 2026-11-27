//! The words of a login refusal reach no note, event, card or aside (T-2367's hard rule): where
//! a task that waited in a retry hold is held with its member's account, its requester is told
//! in the hold's own closed words, as the member's own task is, and nothing the provider wrote.
use super::fixtures::{now, Tiers};
use super::n35_hold::{fail, passes, retrying};
use cf_base::env::Env;
use cf_engine::testing::Context;
use cf_harness::records::{self, Options, Quota, Reading};
use cf_proto::agents::Harness;
use serde_json::json;

const MARKER: &str = "violet fixture invoice";
const SESSION: &str = "33383216-87a0-4e6d-a273-07c4b229cdb1";

/// A dated Pi login refusal whose provider part carries the marker, read by the real Pi reader.
fn pi_refusal(c: &Context) -> Quota {
    let home = tempfile::tempdir().unwrap();
    let at = now(c);
    let ms = cf_base::time::parse(&at).unwrap();
    let rows = [
        json!({"type":"session","id":SESSION,"version":3}),
        json!({"type":"message","id":"u1","parentId":null,"timestamp":at,"message":{"role":"user","content":[{"type":"text","text":"work"}]}}),
        json!({"type":"message","id":"a1","parentId":"u1","timestamp":at,"message":{"role":"assistant","timestamp":ms,"content":[],"stopReason":"error","errorMessage":format!("401: Authentication failed ({MARKER})")}}),
    ];
    let path = home.path().join(format!("{SESSION}.jsonl"));
    std::fs::write(path, rows.map(|row| format!("{row}\n")).concat()).unwrap();
    let env = Env::from_vars([("PI_CODING_AGENT_SESSION_DIR", home.path().to_path_buf())]);
    let read = records::answers(
        Harness::from_kind("pi").unwrap(),
        SESSION,
        &env,
        &Options::default(),
        &records::zone("UTC").unwrap(),
        2_000_000_000_000,
    );
    let Reading::Known(read) = read.as_ref() else {
        panic!("the fixture must be read")
    };
    let quota = read
        .quota
        .as_deref()
        .cloned()
        .expect("the Pi reader recognises the refusal");
    assert!(
        matches!(&quota, Quota::Unavailable { at: Some(recorded), because }
            if recorded == &at && because.starts_with("its login was refused") && because.contains(MARKER)),
        "the reader keeps the provider's part in its reason: {quota:?}"
    );
    quota
}

/// Everything a person or the chief can read of the project, by name: each event, the board,
/// each task's thread and card, the requesters' notes, and what the chief was pasted.
fn surfaces(c: &Context, t: &Tiers<'_>) -> Vec<(String, String)> {
    let ledger = c.ledger.borrow();
    let mut all = vec![
        (
            "events".to_owned(),
            serde_json::to_string(&ledger.events(t.project.id, 0, 5000).unwrap()).unwrap(),
        ),
        (
            "board".to_owned(),
            serde_json::to_string(&ledger.board(t.project.id).unwrap()).unwrap(),
        ),
    ];
    drop(ledger);
    for n in 1..=2 {
        let thread = t.task(n);
        all.push((format!("T-{n} card"), serde_json::to_string(&thread.task).unwrap()));
        all.push((format!("T-{n} thread"), serde_json::to_string(&thread.messages).unwrap()));
    }
    all.push(("chief's notes".to_owned(), format!("{:?}", t.notes("chief"))));
    all.push((
        "human's notes".to_owned(),
        format!("{:?}", crate::fixtures::notes(c, c.id(t.project.id, "human"))),
    ));
    all.push(("chief's pastes".to_owned(), crate::fixtures::given(c, "chief").concat()));
    all
}

#[test]
fn a_task_held_from_its_retry_with_its_member_is_told_in_the_hold_s_own_words() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open_body("Check the parser");
    c.give(t.project.id, "zeus", "Check the lexer");
    passes(&c, 10);
    let windows = [t.assignee(1), t.assignee(2)];
    assert_ne!(windows[0], windows[1]);
    for window in &windows {
        fail(&c, window);
    }
    passes(&c, 4);
    for n in 1..=2 {
        assert_eq!(t.task(n).task.retry.unwrap().which, 1, "T-{n} waits for its try");
    }
    c.advance(5_000);
    c.adapter.refuse(&windows[0], pi_refusal(&c));
    passes(&c, 5);
    assert!(t.out_until("zeus").unwrap().starts_with("9999-"));
    let leaked: Vec<String> = surfaces(&c, &t)
        .into_iter()
        .filter(|(_, text)| text.contains(MARKER))
        .map(|(name, _)| name)
        .collect();
    for n in 1..=2 {
        eprintln!("T-{n} notes = {:?}", t.notes("chief").iter().filter(|x| x.starts_with(&format!("T-{n} waits with @"))).collect::<Vec<_>>());
    }
    assert!(leaked.is_empty(), "the provider's words reached: {leaked:?}");
    for n in 1..=2 {
        let told: Vec<String> = t
            .notes("chief")
            .into_iter()
            .filter(|note| note.starts_with(&format!("T-{n} waits with @")))
            .collect();
        assert_eq!(told.len(), 1, "T-{n} is told once: {told:?}");
        eprintln!("T-{n} note = {told:?}");
    }
}
