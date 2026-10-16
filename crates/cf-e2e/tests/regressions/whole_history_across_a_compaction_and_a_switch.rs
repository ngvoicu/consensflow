//! The owner's question of 2026-10-16, ConsensFlow 3.0.6: **"near the Switch
//! chief we can have a button where we can see the transcript of the whole
//! session we store? like even if we change chiefs or they are compacted; can we
//! still store everything?"**
//!
//! What they had: the copy of every conversation was kept, but only a chief
//! switched in could read the ended ones (`cf history`), never the current one,
//! and the summary Claude Code writes when it compacts a conversation was kept as
//! the human's words: `cf history` said "Human: This session is being
//! continued…".
//!
//! What should be: the History button's operation (`history.get`) reads the
//! chief's conversations whole, the one before a switch and the one going on now,
//! each with its harness, agent and what ended it, and the summary named as a
//! compaction's.
//!
//! The case is that shape on the rig: a Claude chief works, its record is
//! compacted while it is idle (as Claude Code 2.1 writes it), the human switches
//! it to a Codex agent, and the new chief works again.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::idle_compaction_read_as_a_cut_turn::compacted_while_idle;
use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config_with_codex, secs, Outcome};

/// The words of the summary the compaction writes.
const SUMMARY: &str = "This session is being continued from a previous conversation";

/// The chief's whole history, the tools' outputs too, as the History button reads it.
fn history(rig: &Rig, project: &Project) -> cf_e2e::Result<Value> {
    let read = rig.page(
        "history.get",
        json!({ "project": project.id(), "handle": "chief", "tools": true }),
    )?;
    Ok(read["history"].clone())
}

/// Every item of every conversation of a page of the history.
fn items(history: &Value) -> Vec<Value> {
    history["conversations"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|conversation| {
            conversation["items"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// Whether a conversation holds an item of `role` whose text holds `text`.
fn holds(conversation: &Value, role: &str, text: &str) -> bool {
    conversation["items"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == role
                && item["text"]
                    .as_str()
                    .is_some_and(|said| said.contains(text))
        })
    })
}

#[test]
fn the_history_reads_a_chief_s_conversations_whole_across_a_compaction_and_a_switch_of_harness(
) -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    wait_for_the_chief(&rig, &project)?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the first window has no session")?
        .to_owned();

    // The chief works, and Claude Code compacts its conversation while it is idle.
    rig.tell(project.id(), "Reply with exactly: BEFORE_THE_SWITCH")?;
    rig.wait_for("the Claude chief to answer", secs(60), || {
        Ok(rig.transcript(&session).contains("BEFORE_THE_SWITCH"))
    })?;
    wait_for_the_chief(&rig, &project)?;
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    compacted_while_idle(&claude, &session)?;
    // The summary is copied while the window is there: a switch ends its conversation.
    rig.wait_for("the summary to be copied", secs(60), || {
        Ok(items(&history(&rig, &project)?).iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(SUMMARY))
        }))
    })?;

    // The human switches the chief to Codex, and the new chief works again.
    let switched = rig.page(
        "chief.switch",
        json!({ "project": project.id(), "agent": "codex-chief" }),
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("the Codex chief's window", secs(60), || {
        Ok(windows_of(&rig, &project).len() == 2)
    })?;
    wait_for_the_chief(&rig, &project)?;
    rig.tell(project.id(), "Reply with exactly: AFTER_THE_SWITCH")?;
    rig.wait_for("the Codex chief's answer to be copied", secs(60), || {
        let read = history(&rig, &project)?;
        Ok(read["conversations"]
            .as_array()
            .and_then(|conversations| conversations.get(1))
            .is_some_and(|now| holds(now, "assistant", "AFTER_THE_SWITCH")))
    })?;

    let read = history(&rig, &project)?;
    assert_eq!(
        (read["page"].clone(), read["pages"].clone()),
        (json!(1), json!(1)),
        "{read}"
    );
    let conversations = read["conversations"].as_array().ok_or("no conversations")?;
    assert_eq!(conversations.len(), 2, "{read}");
    let (before, now) = (&conversations[0], &conversations[1]);
    // The Claude conversation, whole: what the human typed, the answer, and the summary named.
    assert_eq!(
        (&before["harness"], &before["agent"]),
        (&json!("claude-code"), &json!("chief"))
    );
    assert_eq!(
        before["endedBy"],
        json!({ "by": "switch", "harness": "codex", "agent": "codex-chief" })
    );
    assert!(
        holds(before, "user", "Reply with exactly: BEFORE_THE_SWITCH"),
        "{before}"
    );
    assert!(holds(before, "assistant", "BEFORE_THE_SWITCH"), "{before}");
    let summary = before["items"]
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains(SUMMARY))
            })
        })
        .ok_or("no summary")?;
    assert_eq!(
        (&summary["role"], &summary["compaction"]),
        (&json!("user"), &json!(true))
    );
    // The Codex conversation going on now: the handoff that brought it in, as one
    // line, and its work.
    assert_eq!(
        (&now["harness"], &now["agent"]),
        (&json!("codex"), &json!("codex-chief"))
    );
    assert!(
        now["endedAt"].is_null() && now["endedBy"].is_null(),
        "{now}"
    );
    assert!(
        items(&json!({ "conversations": [now] }))
            .iter()
            .any(|item| item["delivery"]["line"]
                .as_str()
                .is_some_and(|line| line.ends_with("the handoff that brought this chief in"))),
        "{now}"
    );
    assert!(
        holds(now, "user", "Reply with exactly: AFTER_THE_SWITCH"),
        "{now}"
    );

    // And `cf history`, read by the new chief, names the summary as one, never the human's.
    let second = windows_of(&rig, &project).remove(1);
    let earlier = rig.cf_in_window(&second, ["history"])?;
    assert_eq!(earlier.code, Some(0), "{earlier}");
    assert!(
        earlier
            .stdout
            .contains(&format!("Compaction summary: {SUMMARY}")),
        "{earlier}"
    );
    assert!(
        !earlier.stdout.contains(&format!("Human: {SUMMARY}")),
        "{earlier}"
    );
    rig.close()?;
    Ok(())
}
