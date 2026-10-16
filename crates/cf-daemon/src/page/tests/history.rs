//! `history.get`, asked over the bridge as the app asks it: a participant's
//! whole history for the viewer of its window's History button, the current
//! conversation too, its reply held byte for byte, a page that is not there
//! refused in `cf history`'s words, and nothing written or woken.

use cf_ledger::{ChiefSwitch, NewChief, NewNote, NewProject};
use serde_json::Value;

use super::*;

/// A project of `name` whose chief runs on Claude Code as `ares`.
fn project(rig: &Rig, name: &str) -> i64 {
    rig.kit
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
        .id
}

/// A note from the human to the chief of `project`: its id.
fn note(rig: &Rig, project: i64, body: &str) -> i64 {
    rig.kit
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: Some("human".into()),
                to: "chief".into(),
                body: body.into(),
                task: None,
            },
        )
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
            let here = note(&rig, harbour, "Look at the docs");
            let theirs = note(&rig, elsewhere, "Not yours");
            let chief = rig
                .kit
                .ledger
                .borrow()
                .project(harbour)
                .unwrap()
                .unwrap()
                .participants
                .iter()
                .find(|participant| participant.handle == "chief")
                .unwrap()
                .id;
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
            let read = rig
                .kit
                .ledger
                .borrow()
                .participant_history(harbour, "chief")
                .unwrap();
            let logged = rig.kit.ledger.borrow().events(harbour, 0, 500).unwrap().len();

            let answer = ask(&app, "history.get", json!({ "project": harbour, "handle": "chief" })).await;
            let at = "2026-10-16T09:00:00.000Z";
            assert_eq!(
                answer,
                json!({
                    "ok": true,
                    "history": {
                        "page": 1,
                        "pages": 1,
                        "conversations": [
                            {
                                "id": first.id, "harness": "claude-code", "agent": "ares",
                                "startedAt": read[0].conversation.started_at,
                                "endedAt": read[0].conversation.ended_at,
                                "endedBy": { "by": "switch", "harness": "codex", "agent": "maia" },
                                "toolsLeftOut": 1, "earlier": false, "later": false,
                                "items": [
                                    {
                                        "id": "u-1", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{here} · note from @human]\nLook at the docs"),
                                        "delivery": { "line": format!("· m-{here}: a note from @human: \"Look at the docs\" (cf inbox read m-{here})"), "at": 0 },
                                    },
                                    { "id": "a-1", "role": "assistant", "text": "Reading <b>them</b>", "complete": true, "at": at },
                                    { "id": "u-3", "role": "user", "text": "This session is being continued.", "complete": true, "at": at, "compaction": true },
                                    {
                                        "id": "u-4", "role": "user", "complete": true, "at": at,
                                        "text": format!("[ConsensFlow m-{theirs} · note]\nx"),
                                        "delivery": { "line": format!("· m-{theirs}: a message ConsensFlow delivered (no longer on record)"), "at": 0 },
                                    },
                                ],
                            },
                            {
                                "id": second.id, "harness": "codex", "agent": "maia",
                                "startedAt": read[1].conversation.started_at,
                                "endedAt": null, "endedBy": null,
                                "toolsLeftOut": 0, "earlier": false, "later": false,
                                "items": [
                                    { "id": "m-1", "role": "assistant", "text": "Taken over", "complete": true, "at": at },
                                ],
                            },
                        ],
                    },
                })
            );
            // The tools' outputs on request, a search in any case, and a page not there.
            let tools = ask(
                &app,
                "history.get",
                json!({ "project": harbour, "handle": "chief", "tools": true, "find": "PASSED" }),
            )
            .await;
            assert_eq!(tools["history"]["conversations"][0]["items"][0]["text"], "ok 12 passed");
            assert_eq!(
                ask(&app, "history.get", json!({ "project": harbour, "handle": "chief", "page": 2 })).await,
                json!({ "ok": false, "error": "there is 1 page" })
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
