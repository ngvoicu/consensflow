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
            let started = |at: usize| read.conversations[at].conversation.started_at.clone();
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
                                "startedAt": started(0),
                                "endedAt": read.conversations[0].conversation.ended_at,
                                "endedBy": { "by": "switch", "harness": "codex", "agent": "maia" },
                                "toolsLeftOut": 1,
                                "items": [
                                    {
                                        "id": "u-1", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{here} · note from @human]\nLook at the docs"),
                                        "delivery": { "line": format!("· m-{here}: a note from @human: \"Look at the docs\" (cf inbox read m-{here})"), "at": 0 },
                                    },
                                    { "id": "a-1", "role": "assistant", "text": "Reading <b>them</b>", "complete": true, "at": at },
                                    { "id": "u-3", "role": "user", "text": "This session is being continued.", "complete": true, "at": at, "compaction": true },
                                    // Another project's message, and another participant's: text, and nothing of them.
                                    {
                                        "id": "u-4", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{theirs} · note]\nx"),
                                    },
                                    { "id": "u-5", "role": "user", "complete": true, "at": at, "text": quoted },
                                ],
                            },
                            {
                                "id": second.id, "harness": "codex", "agent": "maia",
                                "startedAt": started(1),
                                "endedAt": null, "endedBy": null,
                                "toolsLeftOut": 0,
                                "items": [
                                    { "id": "m-1", "role": "assistant", "text": "Taken over", "complete": true, "at": at },
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
            let place = serde_json::to_value(read.rows[0].key).unwrap();
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
            assert_eq!(before["history"]["newer"]["seq"], 5);
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
            assert_eq!(shown["cut"], json!({ "from": 0, "bytes": whole.len() }));
            let mut read = String::new();
            let mut from = Value::from(0);
            for _ in 0..5 {
                let part = ask(
                    &app,
                    "history.part",
                    json!({ "project": harbour, "handle": "chief", "conversation": conversation.id, "item": "long", "from": from }),
                )
                .await;
                read.push_str(part["part"]["text"].as_str().unwrap());
                from = part["part"]["next"].clone();
                if from.is_null() {
                    break;
                }
            }
            assert_eq!(read, whole);
            assert_eq!(
                ask(
                    &app,
                    "history.part",
                    json!({ "project": harbour, "handle": "zeus", "conversation": conversation.id, "item": "long" }),
                )
                .await,
                json!({ "ok": false, "error": format!("no item long in zeus's conversation {}", conversation.id) })
            );
            assert_eq!(rig.kicks.get(), 0);
        })
        .await;
}
