//! `GET /api/history`: a page of what the chief before this one said, which
//! only a chief reads and which writes down that it was read. Where a test says
//! what Node answered, it is what the real API answered the same request (the
//! probe that printed those answers went with Node's API).

use cf_ledger::ChiefSwitch;
use hyper::Method;
use serde_json::{json, Value};

use super::written_page;
use crate::api::routes::tests::support::api;
use crate::testing::{scene, Scene};

async fn history(scene: &Scene, token: &str, query: &str) -> (u16, Value) {
    api(
        scene,
        Method::GET,
        &format!("/api/history{query}"),
        token,
        "",
    )
    .await
}

/// What the ledger wrote down of reads of the history, as events.
fn reads(scene: &Scene) -> Vec<Value> {
    scene
        .context
        .ledger
        .borrow()
        .events(scene.project.id, 0, 500)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "chief.history.read")
        .map(|event| event.data)
        .collect()
}

/// A chief that was switched out, with what it said and what the human said.
fn switched(scene: &Scene) {
    let mut ledger = scene.context.ledger.borrow_mut();
    let project = ledger.project(scene.project.id).unwrap().unwrap();
    let chief = project
        .participants
        .iter()
        .find(|p| p.handle == "chief")
        .unwrap();
    let old = ledger.start_conversation(chief.id, "claude-code").unwrap();
    ledger
        .copy_transcript(
            old.id,
            &[
                json!({ "id": "u1", "role": "user", "text": "Build the parser", "complete": true }),
                json!({ "id": "a1", "role": "assistant", "text": "On it. The codeword is plum.", "complete": true }),
                json!({ "id": "t1", "role": "tool", "text": "ls -la output", "complete": true }),
            ],
            0,
        )
        .unwrap();
    ledger
        .switch_chief(
            scene.project.id,
            &ChiefSwitch {
                harness: "claude-code".to_owned(),
                agent: "other".to_owned(),
                cut: false,
            },
        )
        .unwrap();
}

#[tokio::test]
async fn only_a_chief_reads_the_history_and_a_member_is_told_before_anything_is_read() {
    let scene = scene();
    switched(&scene);
    let said = history(&scene, &scene.zeus, "?page=abc&find=x").await;
    assert_eq!(
        said,
        (
            403,
            json!({ "error": "not-the-chief", "message": "the chief's history is the chief's to read" })
        )
    );
    assert!(reads(&scene).is_empty());
}

#[tokio::test]
async fn a_page_is_the_page_the_text_and_how_many_pages_there_are() {
    let scene = scene();
    switched(&scene);
    let (status, said) = history(&scene, &scene.chief, "").await;
    assert_eq!(status, 200);
    assert_eq!(
        said.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["page", "pages", "text"]
    );
    assert_eq!(
        (said["page"].clone(), said["pages"].clone()),
        (json!(1), json!(1))
    );
    let text = said["text"].as_str().unwrap();
    assert!(
        text.starts_with(
            "The chief's history, page 1 of 1: the most recent.\n\n── The chief on Claude Code, "
        ),
        "{text}"
    );
    assert!(
        text.ends_with(
            "; 1 tool output left out (--tools) ──\n\nHuman: Build the parser\n\nClaude Code chief: On it. The codeword is plum.\n\nThis is the oldest page."
        ),
        "{text}"
    );
    assert_eq!(
        reads(&scene),
        [json!({ "page": 1, "find": null, "tools": false })]
    );
    assert_eq!(scene.kicks.get(), 0, "a read wakes nothing");
}

#[tokio::test]
async fn tools_are_shown_only_for_exactly_1_and_a_search_is_in_any_case_and_none_when_empty() {
    let scene = scene();
    switched(&scene);
    let (_, shown) = history(&scene, &scene.chief, "?tools=1").await;
    assert!(shown["text"]
        .as_str()
        .unwrap()
        .contains("Tool output:\nls -la output"));
    for query in ["?tools=true", "?tools=0", "?tools=", "?tools=11"] {
        let (_, hidden) = history(&scene, &scene.chief, query).await;
        assert!(
            !hidden["text"].as_str().unwrap().contains("Tool output"),
            "{query}"
        );
    }
    let (_, found) = history(&scene, &scene.chief, "?find=PLUM").await;
    let text = found["text"].as_str().unwrap();
    assert!(
        text.starts_with(
            "The chief's history, entries with \"PLUM\", page 1 of 1: the most recent."
        ),
        "{text}"
    );
    assert!(text.contains("Claude Code chief: On it. The codeword is plum."));
    let (_, none) = history(&scene, &scene.chief, "?find=zzz").await;
    assert_eq!(
        none,
        json!({ "page": 0, "pages": 0, "text": "Nothing in the chief's history contains \"zzz\"." })
    );
    // An empty search is none.
    let (_, empty) = history(&scene, &scene.chief, "?find=").await;
    assert_eq!(empty["page"], 1);
    assert_eq!(
        reads(&scene).last(),
        Some(&json!({ "page": 1, "find": null, "tools": false }))
    );
}

#[tokio::test]
async fn a_page_that_is_none_of_them_is_refused_whatever_it_is_read_as_and_nothing_is_written() {
    let scene = scene();
    switched(&scene);
    for query in [
        "?page=2",
        "?page=0",
        "?page=",
        "?page=abc",
        "?page=1.5",
        "?page=-1",
        "?page=Infinity",
    ] {
        let (status, said) = history(&scene, &scene.chief, query).await;
        assert_eq!(
            (status, said),
            (
                400,
                json!({ "error": "no-such-page", "message": "there is 1 page" })
            ),
            "{query}"
        );
    }
    assert!(reads(&scene).is_empty());
    // And the page is read as `Number` reads it.
    for query in ["?page=1e0", "?page=%201%20", "?page=0x1", "?page=1.0"] {
        let (status, said) = history(&scene, &scene.chief, query).await;
        assert_eq!((status, said["page"].clone()), (200, json!(1)), "{query}");
    }
    assert_eq!(reads(&scene).len(), 4);
}

#[tokio::test]
async fn a_line_may_name_any_message_number_and_only_this_project_s_are_read_out() {
    let scene = scene();
    // Message numbers run across projects: m-2 is a note of another project's
    // chief, and m-1 is the question zeus put to this one.
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let other = ledger
            .create_project(&cf_ledger::NewProject {
                directory: "/work/site".to_owned(),
                name: "site".to_owned(),
                chief: cf_ledger::NewChief {
                    harness: "codex".to_owned(),
                    agent: Some("mybuilder".to_owned()),
                },
                staff: Vec::new(),
                gate: false,
            })
            .unwrap();
        let note = ledger
            .note(
                other.id,
                &cf_ledger::NewNote {
                    from: Some("chief".to_owned()),
                    to: "human".to_owned(),
                    body: "Launch code 4417".to_owned(),
                    task: None,
                },
            )
            .unwrap();
        assert_eq!(note.id, 2);
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let chief = project
            .participants
            .iter()
            .find(|p| p.handle == "chief")
            .unwrap();
        let old = ledger.start_conversation(chief.id, "claude-code").unwrap();
        ledger
            .copy_transcript(
                old.id,
                &[
                    json!({ "id": "a", "role": "user", "text": "[ConsensFlow m-2 · pasted from elsewhere]", "complete": true }),
                    json!({ "id": "b", "role": "user", "text": "[ConsensFlow m-1 · pasted from elsewhere]", "complete": true }),
                ],
                0,
            )
            .unwrap();
        ledger
            .switch_chief(
                scene.project.id,
                &ChiefSwitch {
                    harness: "claude-code".to_owned(),
                    agent: "other".to_owned(),
                    cut: false,
                },
            )
            .unwrap();
    }
    let (status, said) = history(&scene, &scene.chief, "").await;
    assert_eq!(status, 200, "{said}");
    let text = said["text"].as_str().unwrap();
    assert!(
        text.contains("· m-2: a message ConsensFlow delivered (no longer on record)"),
        "another project's message is not read out: {text}"
    );
    assert!(
        !text.contains("· m-1: a message ConsensFlow delivered (no longer on record)"),
        "this project's is: {text}"
    );
    assert!(!text.contains("4417"), "{text}");
}

#[tokio::test]
async fn a_project_with_no_earlier_chief_says_so_whatever_page_is_asked_for() {
    let scene = scene();
    for query in ["", "?page=3", "?page=abc", "?page=0"] {
        let (status, said) = history(&scene, &scene.chief, query).await;
        assert_eq!(
            (status, said),
            (
                200,
                json!({
                    "page": 0,
                    "pages": 0,
                    "text": "No earlier chief conversations: you are the first chief of this project."
                })
            ),
            "{query}"
        );
    }
    let (_, said) = history(&scene, &scene.chief, "?find=x").await;
    assert_eq!(
        said["text"],
        "Nothing in the chief's history contains \"x\"."
    );
}

#[tokio::test]
async fn the_page_written_down_is_the_page_asked_for_where_it_is_a_number_the_ledger_writes() {
    let scene = scene();
    for query in ["?page=3", "?page=0", "?page=-2"] {
        history(&scene, &scene.chief, query).await;
    }
    assert_eq!(
        reads(&scene),
        [
            json!({ "page": 3, "find": null, "tools": false }),
            json!({ "page": 0, "find": null, "tools": false }),
            json!({ "page": -2, "find": null, "tools": false }),
        ]
    );
}

/// Node wrote the page down as it was read, `null` for the page `abc` is and
/// `2.5` for another; the ledger writes whole numbers (`i64`), and the page of
/// an empty history, which answers whichever it is asked for, is written as the
/// page that answer is: 0. A departure, and the only one.
#[tokio::test]
async fn a_page_that_is_no_whole_number_is_written_down_as_the_page_of_the_answer() {
    let scene = scene();
    for query in ["?page=abc", "?page=2.5", "?page=1e21", "?page=Infinity"] {
        history(&scene, &scene.chief, query).await;
    }
    assert_eq!(
        reads(&scene),
        vec![json!({ "page": 0, "find": null, "tools": false }); 4]
    );
}

#[test]
fn a_page_is_written_as_a_whole_number_a_double_holds_exactly_or_as_0() {
    for (page, written) in [
        (1.0, 1),
        (0.0, 0),
        (-7.0, -7),
        (9_007_199_254_740_991.0, 9_007_199_254_740_991),
        (9_007_199_254_740_992.0, 0),
        (2.5, 0),
        (f64::NAN, 0),
        (f64::INFINITY, 0),
        (f64::NEG_INFINITY, 0),
        (1e21, 0),
    ] {
        assert_eq!(written_page(page), written, "{page}");
    }
}
