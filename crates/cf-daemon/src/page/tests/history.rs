//! `history.get` and `history.part`, asked over the bridge as the app asks
//! them: a participant's whole history for the viewer of its window's History
//! button, the current conversation too, its reply held whole; a delivery's
//! line only for a message this participant was sent or sent; a long item read
//! a part at a time, the participant's own alone; and nothing written or woken.

use cf_ledger::{
    ChiefSwitch, HistoryAsk, HistoryFrom, NewChief, NewMember, NewNote, NewProject, HISTORY_PART,
    HISTORY_SCAN_BYTES, PAGE_BYTES,
};
use serde_json::Value;

use super::*;

/// A project of `name` whose chief runs on Claude Code as `ares`, and zeus on its staff.
fn project(rig: &Rig, name: &str) -> i64 {
    let id = rig
        .kit
        .ledger
        .borrow_mut()
        .create_project(&NewProject {
            directory: format!("/work/{name}"),
            name: name.into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("ares".into()),
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
        })
        .unwrap()
        .id;
    rig.kit
        .ledger
        .borrow_mut()
        .add_member(
            id,
            &NewMember {
                agent: "zeus".into(),
                harness: "pi".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .unwrap();
    id
}

/// A note from `from` to `to` in `project`: its id.
fn note(rig: &Rig, project: i64, from: &str, to: &str, body: &str) -> i64 {
    rig.kit
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some(from.into()),
                to: to.into(),
                body: body.into(),
                task: None,
            },
        )
        .unwrap()
        .id
}

/// The chief of `project`, by its id.
fn chief_of(rig: &Rig, project: i64) -> i64 {
    rig.kit
        .ledger
        .borrow()
        .project(project)
        .unwrap()
        .unwrap()
        .participants
        .iter()
        .find(|participant| participant.handle == "chief")
        .unwrap()
        .id
}

fn item(id: &str, role: &str, text: &str) -> Value {
    json!({ "id": id, "role": role, "text": text, "complete": true, "at": "2026-10-16T09:00:00.000Z" })
}

#[tokio::test]
async fn reads_a_participant_s_whole_history_for_the_viewer_and_writes_and_wakes_nothing() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let elsewhere = project(&rig, "elsewhere");
            let here = note(&rig, harbour, "human", "chief", "Look at the docs");
            let zeus_s = note(&rig, harbour, "human", "zeus", "Zeus's own plans");
            // Another project's chief's own note: a sender of the same handle, not this one.
            let theirs = note(&rig, elsewhere, "chief", "human", "Not yours");
            let chief = chief_of(&rig, harbour);
            // The chief works on Claude Code, which compacts its conversation; the
            // human switches it to Codex, on maia, and it works again.
            let first = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            let mut summary = item("u-3", "user", "This session is being continued.");
            summary["compaction"] = true.into();
            let quoted = format!("[ConsensFlow m-{zeus_s} · note from @human]\nas zeus was told");
            let worked = [
                item(
                    "u-1",
                    "user",
                    &format!("[ConsensFlow m-{here} · note from @human]\nLook at the docs"),
                ),
                item("a-1", "assistant", "Reading <b>them</b>"),
                item("t-1", "tool", "ok 12 passed"),
                summary,
                item("u-4", "user", &format!("[ConsensFlow m-{theirs} · note]\nx")),
                item("u-5", "user", &quoted),
            ];
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(first.id, &worked, 0)
                .unwrap();
            rig.kit
                .ledger
                .borrow_mut()
                .switch_chief(
                    harbour,
                    &ChiefSwitch {
                        harness: "codex".into(),
                        model: "maia".into(),
                        cut: false,
                    },
                )
                .unwrap();
            let second = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "codex")
                .unwrap();
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(second.id, &[item("m-1", "assistant", "Taken over")], 0)
                .unwrap();
            let ask_all = HistoryAsk {
                from: HistoryFrom::Newest,
                find: None,
                tools: false,
                room: PAGE_BYTES,
                scan: HISTORY_SCAN_BYTES,
            };
            let read = rig
                .kit
                .ledger
                .borrow()
                .read_history(harbour, "chief", &ask_all)
                .unwrap();
            // The place each item stands at, which a page gives with it.
            let row = |id: &str| {
                read.entries
                    .iter()
                    .find(|row| row.item.id == id)
                    .unwrap()
                    .key
                    .row
            };
            let logged = rig.kit.ledger.borrow().events(harbour, 0, 500).unwrap().len();

            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let at = "2026-10-16T09:00:00.000Z";
            assert_eq!(
                answer,
                json!({
                    "ok": true,
                    "history": {
                        "items": [
                            {
                                "conversation": first.id, "row": row("u-1"), "id": "u-1", "role": "user", "complete": true, "at": at,
                                "text": format!("[ConsensFlow m-{here} · note from @human]\nLook at the docs"),
                                "delivery": { "line": format!("· m-{here}: a note from @human: \"Look at the docs\" (cf inbox read m-{here})"), "at": 0 },
                            },
                            { "conversation": first.id, "row": row("a-1"), "id": "a-1", "role": "assistant", "text": "Reading <b>them</b>", "complete": true, "at": at },
                            { "conversation": first.id, "row": row("u-3"), "id": "u-3", "role": "user", "text": "This session is being continued.", "complete": true, "at": at, "compaction": true },
                            // Another project's message, and another participant's: text, and nothing of them.
                            {
                                "conversation": first.id, "row": row("u-4"), "id": "u-4", "role": "user", "complete": true, "at": at,
                                "text": format!("[ConsensFlow m-{theirs} · note]\nx"),
                            },
                            { "conversation": first.id, "row": row("u-5"), "id": "u-5", "role": "user", "complete": true, "at": at, "text": quoted },
                            // The conversation after the switch, in the same stream.
                            { "conversation": second.id, "row": row("m-1"), "id": "m-1", "role": "assistant", "text": "Taken over", "complete": true, "at": at },
                        ],
                        "older": null,
                        "newer": null,
                        "partial": false,
                    },
                })
            );
            assert!(!answer.to_string().contains("Zeus's own plans"));
            assert!(!answer.to_string().contains("Not yours"));
            // The tools' outputs on request, and a search in any case.
            let tools = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "tools": true, "find": "PASSED" }),
            )
            .await;
            assert_eq!(tools["history"]["items"][0]["text"], "ok 12 passed");
            // A page from a place a page said (before Codex's first item); never
            // before and after at once.
            let place = serde_json::to_value(read.entries[0].key).unwrap();
            assert_eq!(place["conversation"], second.id);
            let before = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "before": place }),
            )
            .await;
            let conversations: std::collections::HashSet<&Value> = before["history"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| &item["conversation"])
                .collect();
            assert_eq!(conversations.len(), 1, "{before}");
            // Newer goes on after the newest item that page shows, the chief's last on Claude.
            assert_eq!(before["history"]["newer"]["conversation"], first.id);
            assert_eq!(
                before["history"]["newer"],
                serde_json::to_value(read.entries[1].key).unwrap(),
                "u-5's place"
            );
            assert_eq!(
                ask(&app, "history.get", json!({ "project": harbour, "handle": "chief", "before": place, "after": place })).await,
                json!({ "ok": false, "error": "a page is before a place or after one, not both" })
            );
            assert_eq!(
                ask(&app, "history.get", json!({ "project": harbour, "handle": "chief", "before": 3 })).await,
                json!({ "ok": false, "error": "before is a place a page said, not 3" })
            );
            assert_eq!(
                ask(&app, "history.get", json!({ "project": harbour, "handle": "nobody" })).await,
                json!({ "ok": false, "error": "\"nobody\" is not in project 1" })
            );
            assert_eq!(rig.kicks.get(), 0, "a read wakes nothing");
            assert_eq!(
                rig.kit.ledger.borrow().events(harbour, 0, 500).unwrap().len(),
                logged,
                "and writes nothing"
            );
        })
        .await;
}

/// What a page must never say, in any key or value: a heading's fields, and the
/// harnesses, the models and the handle of the chief whose history it is (CHF-11).
const NO_HEADING: [&str; 14] = [
    "conversations",
    "harness",
    "agent",
    "startedAt",
    "endedAt",
    "endedBy",
    "toolsLeftOut",
    "claude-code",
    "codex",
    "Claude",
    "Codex",
    "ares",
    "maia",
    "chief",
];

/// Whether `answer` says nothing [`NO_HEADING`] holds; the first it says, where it does.
fn headed(answer: &Value) -> Option<&'static str> {
    let said = answer.to_string();
    NO_HEADING.into_iter().find(|word| said.contains(word))
}

/// Each item a page shows, as its conversation, its id and its text.
fn stream(answer: &Value) -> Vec<(i64, String, String)> {
    answer["history"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("a page of items: {answer}"))
        .iter()
        .map(|item| {
            (
                item["conversation"].as_i64().unwrap(),
                item["id"].as_str().unwrap().to_owned(),
                item["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The place a page shows `id` at, as a page says a place.
fn place_of(answer: &Value, id: &str) -> Value {
    let item = answer["history"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap();
    json!({ "conversation": item["conversation"], "row": item["row"] })
}

#[tokio::test]
async fn the_history_is_one_stream_of_items_oldest_first_across_three_conversations_on_two_harnesses(
) {
    // CHF-11: three conversations of the chief, two on Claude Code and one on
    // Codex after a Switch chief, and the one going on now with nothing copied
    // yet. The page is their items, oldest first, each with its conversation and
    // row: no heading, harness, model or name anywhere, and the empty one absent.
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let copy = |conversation: i64, items: &[Value]| {
                rig.kit
                    .ledger
                    .borrow_mut()
                    .copy_transcript(conversation, items, 0)
                    .unwrap();
            };
            let first = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            copy(first.id, &[item("u-1", "user", "Ship the parser"), item("a-1", "assistant", "On it")]);
            let cleared = rig
                .kit
                .ledger
                .borrow_mut()
                .follow_conversation(chief, "claude-code", "s-2")
                .unwrap();
            copy(cleared.id, &[item("u-2", "user", "A fresh start")]);
            rig.kit
                .ledger
                .borrow_mut()
                .switch_chief(
                    harbour,
                    &ChiefSwitch {
                        harness: "codex".into(),
                        model: "maia".into(),
                        cut: false,
                    },
                )
                .unwrap();
            let third = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "codex")
                .unwrap();
            copy(third.id, &[item("m-1", "assistant", "Taken over"), item("u-3", "user", "Go on")]);
            let now = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "codex")
                .unwrap();

            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            assert_eq!(answer["ok"], true, "{answer}");
            let keys: Vec<&String> = answer["history"].as_object().unwrap().keys().collect();
            assert_eq!(keys, ["items", "older", "newer", "partial"], "{answer}");
            assert_eq!(headed(&answer), None, "{answer}");
            let said = |id: &str, text: &str, conversation: i64| (conversation, id.to_owned(), text.to_owned());
            assert_eq!(
                stream(&answer),
                [
                    said("u-1", "Ship the parser", first.id),
                    said("a-1", "On it", first.id),
                    said("u-2", "A fresh start", cleared.id),
                    said("m-1", "Taken over", third.id),
                    said("u-3", "Go on", third.id),
                ]
            );
            assert!(stream(&answer).iter().all(|(conversation, ..)| *conversation != now.id));
            let fields: Vec<&String> = answer["history"]["items"][0].as_object().unwrap().keys().collect();
            assert_eq!(fields, ["conversation", "row", "id", "role", "text", "complete", "at"]);

            // Paging across a conversation's edge, each way, from a place a page said:
            // before Codex's first item are Claude Code's two conversations, and after
            // the first conversation's last item, the second's and the third's.
            let before = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "before": place_of(&answer, "m-1") }),
            )
            .await;
            assert_eq!(headed(&before), None, "{before}");
            let ids = |page: &Value| -> Vec<String> { stream(page).into_iter().map(|(_, id, _)| id).collect() };
            assert_eq!(ids(&before), ["u-1", "a-1", "u-2"]);
            assert_eq!(before["history"]["older"], Value::Null, "nothing older");
            assert_eq!(before["history"]["newer"], place_of(&answer, "u-2"));
            let after = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "after": place_of(&answer, "a-1") }),
            )
            .await;
            assert_eq!(headed(&after), None, "{after}");
            assert_eq!(ids(&after), ["u-2", "m-1", "u-3"]);
            assert_eq!(after["history"]["older"], place_of(&answer, "u-2"));
            assert_eq!(after["history"]["newer"], Value::Null, "nothing newer");
            // A member's history is drawn the same way: one stream, nothing named.
            let zeus = ask(&app, "history.get", json!({ "project": harbour, "handle": "zeus" })).await;
            assert_eq!(zeus["history"], json!({ "items": [], "older": null, "newer": null, "partial": false }));
        })
        .await;
}

#[tokio::test]
async fn a_search_runs_across_conversations_and_across_a_part_boundary_and_names_no_conversation() {
    // CHF-13: the words found in a long item of the first conversation, where they
    // cross from one part to the next, and in the third's; the second holds none.
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let mut conversations = Vec::new();
            let long = format!("{}needle{}", "é".repeat(HISTORY_PART - 3), "ü".repeat(100));
            let copied = [
                ("claude-code", vec![item("long", "assistant", &long)]),
                ("claude-code", vec![item("u-2", "user", "nothing here")]),
                ("codex", vec![item("u-3", "user", "A NEEDLE again")]),
            ];
            for (harness, items) in copied {
                let conversation = rig
                    .kit
                    .ledger
                    .borrow_mut()
                    .start_conversation(chief, harness)
                    .unwrap();
                rig.kit
                    .ledger
                    .borrow_mut()
                    .copy_transcript(conversation.id, &items, 0)
                    .unwrap();
                conversations.push(conversation.id);
            }
            let found = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "find": "Needle" }),
            )
            .await;
            assert_eq!(found["ok"], true, "{found}");
            assert_eq!(headed(&found), None, "{found}");
            let items = found["history"]["items"].as_array().unwrap();
            let shown: Vec<(i64, &str)> = items
                .iter()
                .map(|item| {
                    (
                        item["conversation"].as_i64().unwrap(),
                        item["id"].as_str().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                shown,
                [(conversations[0], "long"), (conversations[2], "u-3")]
            );
            // The long one shows the part around the words, which begin in its first part.
            let cut = &items[0]["cut"];
            assert!(cut["from"].as_u64().unwrap() > 0, "{cut}");
            assert!(items[0]["text"].as_str().unwrap().contains("needle"));
        })
        .await;
}

#[tokio::test]
async fn reads_a_long_item_whole_a_part_at_a_time_and_only_its_participant_s() {
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let conversation = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            let whole = format!("{}🙂 the end", "é".repeat(HISTORY_PART * 2));
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(conversation.id, &[item("long", "assistant", &whole)], 0)
                .unwrap();
            let page = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let shown = &page["history"]["items"][0];
            assert_eq!(shown["cut"], json!({ "from": 0, "bytes": whole.len(), "revision": 0 }));
            let row = shown["row"].clone();
            let mut read = String::new();
            let mut revisions = Vec::new();
            let mut from = Value::from(0);
            for _ in 0..5 {
                let part = ask(
                    &app,
                    "history.part",
                    json!({ "project": harbour, "handle": "chief", "conversation": conversation.id, "row": row, "from": from }),
                )
                .await;
                read.push_str(part["part"]["text"].as_str().unwrap());
                revisions.push(part["part"]["revision"].clone());
                from = part["part"]["next"].clone();
                if from.is_null() {
                    break;
                }
            }
            assert_eq!(read, whole);
            assert_eq!(revisions, [json!(0), json!(0), json!(0)], "each part of the first copy");
            assert_eq!(
                ask(
                    &app,
                    "history.part",
                    json!({ "project": harbour, "handle": "zeus", "conversation": conversation.id, "row": row }),
                )
                .await,
                json!({ "ok": false, "error": format!("no item at row {row} of zeus's conversation {}", conversation.id) })
            );
            assert_eq!(rig.kicks.get(), 0);
        })
        .await;
}

#[tokio::test]
async fn folds_each_delivery_from_its_message_s_opening_and_never_loads_a_body_whole() {
    // The reviewers' case: twenty messages of a megabyte, each delivered with
    // its opening alone, which a page holds; the lines are said from what
    // opens each message, and no body is read whole.
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let conversation = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            let mut delivered = Vec::new();
            for at in 0..20 {
                let opening = format!("Read part {at} first\n");
                let body = format!("{opening}{}", "x".repeat(1_000_000 - opening.len()));
                let id = note(&rig, harbour, "human", "chief", &body);
                let text = format!("[ConsensFlow m-{id} · note from @human]\n{opening}{}", "x".repeat(15_000));
                delivered.push((id, item(&format!("u-{at}"), "user", &text)));
            }
            let items: Vec<Value> = delivered.iter().map(|(_, item)| item.clone()).collect();
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(conversation.id, &items, 0)
                .unwrap();
            // A read that loaded one body whole would fail.
            rig.kit.ledger.borrow().limit_value_length(512 * 1024).unwrap();
            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let said = answer.to_string();
            assert_eq!(answer["ok"], true, "{}", &said[..said.len().min(300)]);
            let lines: Vec<&str> = answer["history"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["delivery"]["line"].as_str().unwrap())
                .collect();
            let expected: Vec<String> = delivered
                .iter()
                .enumerate()
                .map(|(at, (id, _))| {
                    format!("· m-{id}: a note from @human: \"Read part {at} first\" (cf inbox read m-{id})")
                })
                .collect();
            assert_eq!(lines, expected);
        })
        .await;
}

#[tokio::test]
async fn an_item_s_id_however_long_is_shown_cut_and_its_page_fits_a_frame() {
    // The reviewers' case: a native id of a mebibyte, its text "ok".
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let conversation = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            let long = "i".repeat(1024 * 1024);
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(conversation.id, &[item(&long, "user", "ok")], 0)
                .unwrap();
            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let said = answer.to_string();
            assert_eq!(answer["ok"], true, "{}", &said[..said.len().min(300)]);
            assert!(said.len() <= PAGE_BYTES, "{} bytes", said.len());
            let shown = &answer["history"]["items"][0];
            assert_eq!(shown["text"], "ok");
            // As a page shows it: its first characters, and "…"; the id is kept whole.
            assert_eq!(
                shown["id"],
                format!("{}…", "i".repeat(cf_ledger::ID_SHOWN))
            );
            // Its text is read by its place, never by its id.
            let part = ask(
                &app,
                "history.part",
                json!({ "project": harbour, "handle": "chief", "conversation": conversation.id, "row": shown["row"] }),
            )
            .await;
            assert_eq!(part["part"]["text"], "ok", "{part}");
        })
        .await;
}

#[tokio::test]
async fn reads_past_thousands_of_conversations_that_show_no_item_to_the_one_item_before_them() {
    // The reviewers' case, with no headings (CHF-11): the chief's first words,
    // then 10,000 conversations of tools' outputs alone, the tools left out. No
    // page shows a conversation that shows no item: each read goes through as
    // much as one may and says where the next goes on, each reply fits a frame,
    // and a few reads come to the first words.
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
            let first = rig
                .kit
                .ledger
                .borrow_mut()
                .start_conversation(chief, "claude-code")
                .unwrap();
            rig.kit
                .ledger
                .borrow_mut()
                .copy_transcript(first.id, &[item("u-1", "user", "The first words")], 0)
                .unwrap();
            for at in 0..10_000 {
                let conversation = rig
                    .kit
                    .ledger
                    .borrow_mut()
                    .start_conversation(chief, "claude-code")
                    .unwrap();
                rig.kit
                    .ledger
                    .borrow_mut()
                    .copy_transcript(
                        conversation.id,
                        &[item(&format!("t-{at}"), "tool", "ok")],
                        0,
                    )
                    .unwrap();
            }
            let mut shown = Vec::new();
            let mut reads = 0;
            let mut asked = json!({ "project": harbour, "handle": "chief" });
            loop {
                reads += 1;
                assert!(reads <= 10, "no end");
                let answer = ask(&app, "history.get", asked.clone()).await;
                let said = answer.to_string();
                assert_eq!(answer["ok"], true, "{}", &said[..said.len().min(200)]);
                assert!(said.len() <= PAGE_BYTES);
                assert_eq!(headed(&answer), None, "{}", &said[..said.len().min(200)]);
                // The 10,000 cost more than one read may go through: the first shows nothing.
                if reads == 1 {
                    assert_eq!(stream(&answer), [], "{}", &said[..said.len().min(200)]);
                }
                shown.extend(stream(&answer));
                let older = &answer["history"]["older"];
                if older.is_null() {
                    break;
                }
                // A read that stopped at its bound with nothing to show says so.
                assert_eq!(
                    answer["history"]["partial"],
                    true,
                    "{}",
                    &said[..said.len().min(200)]
                );
                asked = json!({ "project": harbour, "handle": "chief", "before": older });
            }
            assert_eq!(
                shown,
                [(first.id, "u-1".to_owned(), "The first words".to_owned())]
            );
            assert!((2..=5).contains(&reads), "{reads} reads");
        })
        .await;
}

#[test]
fn a_page_of_a_list_is_at_most_half_the_frame_the_bridge_carries() {
    // `state-list-grows-toward-frame-cap`: the replies above are held to `PAGE_BYTES`, so a
    // bound raised past the frame passes them as it passes itself. It is half the frame:
    // what the page reads beside a list (a refresh asks for several) stays clear of it.
    let frame = cf_bridge::local::DEFAULT_MAX_FRAME_BYTES;
    assert!(PAGE_BYTES * 2 <= frame, "{PAGE_BYTES} bytes a page, {frame} a frame");
}
