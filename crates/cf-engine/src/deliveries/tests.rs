use std::cell::RefCell;
use std::rc::Rc;

use cf_ledger::NewNote;
use serde_json::json;

use super::*;
use crate::testing::Context;

#[test]
fn t1813_only_a_brief_paste_asks_the_host_for_a_write_report() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let brief = context
        .give(project.id, "zeus", "Write the parser")
        .message
        .unwrap()
        .id;
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: "zeus".to_owned(),
                body: "A note".to_owned(),
                task: Some(1),
            },
        )
        .unwrap()
        .id;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&seen);
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op, body| {
        if op == "pane.write_paste" {
            recorded.borrow_mut().push(body.clone());
        }
    }));
    let body = json!({"id":"p1-zeus","generation":1,"body":"words"});
    let admitting = Admitting::handing(&context.dispatcher, brief, Vec::new());
    drop(admitting.request("pane.write_paste", body.clone()));
    let other = Admitting::handing(&context.dispatcher, note, Vec::new());
    drop(other.request("pane.write_paste", body.clone()));
    let again = Admitting::handing(&context.dispatcher, brief, Vec::new()).again();
    drop(again.request("pane.write_paste", body));
    let seen = seen.borrow();
    assert_eq!(seen[0]["brief"], brief);
    assert!(seen[1].get("brief").is_none());
    assert!(seen[2].get("brief").is_none());
}
