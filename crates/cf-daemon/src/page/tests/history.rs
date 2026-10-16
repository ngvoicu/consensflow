//! `history.get` and `history.part`, asked over the bridge as the app asks
//! them: a participant's whole history for the viewer of its window's History
//! button, the current conversation too, its reply held whole; a delivery's
//! line only for a message this participant was sent or sent; a long item read
//! a part at a time, the participant's own alone; and nothing written or woken.

use cf_ledger::{
    ChiefSwitch, HistoryAsk, HistoryEntry, HistoryFrom, NewChief, NewMember, NewNote, NewProject,
    HISTORY_PART, HISTORY_SCAN_BYTES, PAGE_BYTES,
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
                agent: Some("ares".into()),
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
                        agent: "maia".into(),
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
            let heading = |id: i64| {
                read.conversations
                    .iter()
                    .find(|heading| heading.conversation.id == id)
                    .unwrap()
                    .conversation
                    .clone()
            };
            // The place each item stands at, which a page gives with it.
            let row = |id: &str| {
                read.entries
                    .iter()
                    .find_map(|entry| match entry {
                        HistoryEntry::Item(row) if row.item.id == id => Some(row.key.row),
                        _ => None,
                    })
                    .unwrap()
            };
            let logged = rig.kit.ledger.borrow().events(harbour, 0, 500).unwrap().len();

            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let at = "2026-10-16T09:00:00.000Z";
            assert_eq!(
                answer,
                json!({
                    "ok": true,
                    "history": {
                        "conversations": [
                            {
                                "id": first.id, "harness": "claude-code", "agent": "ares",
                                "startedAt": heading(first.id).started_at,
                                "endedAt": heading(first.id).ended_at,
                                "endedBy": { "by": "switch", "harness": "codex", "agent": "maia" },
                                "toolsLeftOut": 1,
                                "items": [
                                    {
                                        "row": row("u-1"), "id": "u-1", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{here} · note from @human]\nLook at the docs"),
                                        "delivery": { "line": format!("· m-{here}: a note from @human: \"Look at the docs\" (cf inbox read m-{here})"), "at": 0 },
                                    },
                                    { "row": row("a-1"), "id": "a-1", "role": "assistant", "text": "Reading <b>them</b>", "complete": true, "at": at },
                                    { "row": row("u-3"), "id": "u-3", "role": "user", "text": "This session is being continued.", "complete": true, "at": at, "compaction": true },
                                    // Another project's message, and another participant's: text, and nothing of them.
                                    {
                                        "row": row("u-4"), "id": "u-4", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{theirs} · note]\nx"),
                                    },
                                    { "row": row("u-5"), "id": "u-5", "role": "user", "complete": true, "at": at, "text": quoted },
                                ],
                            },
                            {
                                "id": second.id, "harness": "codex", "agent": "maia",
                                "startedAt": heading(second.id).started_at,
                                "endedAt": null, "endedBy": null,
                                "toolsLeftOut": 0,
                                "items": [
                                    { "row": row("m-1"), "id": "m-1", "role": "assistant", "text": "Taken over", "complete": true, "at": at },
                                ],
                            },
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
            assert_eq!(tools["history"]["conversations"][0]["items"][0]["text"], "ok 12 passed");
            // A page from a place a page said (before Codex's first item); never
            // before and after at once.
            let place = serde_json::to_value(read.entries[0].key()).unwrap();
            assert_eq!(place["conversation"], second.id);
            let before = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "before": place }),
            )
            .await;
            assert_eq!(before["history"]["conversations"].as_array().unwrap().len(), 1, "{before}");
            // Newer goes on after the newest item that page shows, the chief's last on Claude.
            assert_eq!(before["history"]["newer"]["conversation"], first.id);
            assert_eq!(
                before["history"]["newer"],
                serde_json::to_value(read.entries[1].key()).unwrap(),
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
            let shown = &page["history"]["conversations"][0]["items"][0];
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
            let lines: Vec<&str> = answer["history"]["conversations"][0]["items"]
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
            let shown = &answer["history"]["conversations"][0]["items"][0];
            assert_eq!(shown["text"], "ok");
            // As the copy keeps it: its first characters and its hash, as many as a field holds.
            let id = shown["id"].as_str().unwrap();
            assert!(id.starts_with(&format!("{}…", "i".repeat(223))), "{id}");
            assert_eq!(id.chars().count(), cf_ledger::FIELD_MAX);
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
async fn pages_through_thousands_of_conversations_that_show_no_item_each_reply_held_whole() {
    // The reviewers' case: 10,000 conversations of tools' outputs alone, the
    // tools left out, each shown as its heading. Each reply fits a frame and
    // says where the next goes on, and every heading comes once.
    LocalSet::new()
        .run_until(async {
            let rig = rig();
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let harbour = project(&rig, "harbour");
            let chief = chief_of(&rig, harbour);
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
            let mut headed = Vec::new();
            let mut asked = json!({ "project": harbour, "handle": "chief" });
            for _ in 0..20 {
                let answer = ask(&app, "history.get", asked.clone()).await;
                let said = answer.to_string();
                assert_eq!(answer["ok"], true, "{}", &said[..said.len().min(200)]);
                assert!(said.len() <= PAGE_BYTES);
                for conversation in answer["history"]["conversations"].as_array().unwrap() {
                    assert_eq!(conversation["toolsLeftOut"], 1);
                    headed.push(conversation["id"].as_i64().unwrap());
                }
                let older = &answer["history"]["older"];
                if older.is_null() {
                    break;
                }
                asked = json!({ "project": harbour, "handle": "chief", "before": older });
            }
            let once: std::collections::HashSet<i64> = headed.iter().copied().collect();
            assert_eq!((headed.len(), once.len()), (10_000, 10_000));
        })
        .await;
}
