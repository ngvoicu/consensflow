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
//! and the summary named as a compaction's. Since 3.0.7 (chief-harness, CHF-11) it
//! reads them as one timeline: their items oldest first, each with the
//! conversation and the row it stands at, and no heading, harness or model named
//! anywhere in the page; a page goes on across a conversation's edge either way.
//!
//! The case is that shape on the rig: a Claude chief works, its record is
//! compacted while it is idle (as Claude Code 2.1 writes it), the human switches
//! it to a Codex agent, and the new chief works again.

use cf_e2e::rig::{chief_body, Project, Rig};
use serde_json::{json, Value};

use crate::idle_compaction_read_as_a_cut_turn::{
    compacted_while_idle, history, is_said, items, told, wait_for_the_turn_to_end, SUMMARY,
};
use crate::resume_of_a_chief_with_no_turn::{wait_for_the_chief, windows_of};
use crate::{config_with_codex, secs, Outcome};

/// What a page of the history must never say of itself, in any key or value
/// (CHF-11): a heading's fields, and the harnesses and models the chief ran on.
const NO_HEADING: [&str; 10] = [
    "conversations",
    "harness",
    "agent",
    "endedBy",
    "startedAt",
    "toolsLeftOut",
    "claude-code",
    "codex",
    "fake-chief",
    "codex-chief",
];

/// Whether `items` hold one of `role` whose text holds `text`.
fn holds(items: &[&Value], role: &str, text: &str) -> bool {
    items.iter().any(|item| is_said(item, role, text))
}

/// What a page says itself, with what its items say taken out: their text is
/// what the window was given, which may name a harness (the handoff
/// ConsensFlow delivers names the one the chief left), shown in full.
fn page_itself(page: &Value) -> Value {
    let mut page = page.clone();
    for item in page["items"].as_array_mut().into_iter().flatten() {
        item["text"] = json!("");
    }
    page
}

/// The place a page shows `item` at, as a page says a place.
fn place(item: &Value) -> Value {
    json!({ "conversation": item["conversation"], "row": item["row"] })
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
    rig.stand_ins(&roster)?;
    let project = Project::open(&rig, "chief", json!({}))?;

    // The chief works, and Claude Code compacts its conversation while it is idle.
    let session = told(&rig, &project, "Reply with exactly: BEFORE_THE_SWITCH")?;
    wait_for_the_turn_to_end(&rig, &session, "Reply with exactly: BEFORE_THE_SWITCH")?;
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
        chief_body(&rig, "codex-chief", json!({ "project": project.id() }))?,
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("the Codex chief's window", secs(60), || {
        Ok(windows_of(&rig, &project).len() == 2)
    })?;
    wait_for_the_chief(&rig, &project)?;
    rig.tell(project.id(), "Reply with exactly: AFTER_THE_SWITCH")?;
    rig.wait_for("the Codex chief's answer to be copied", secs(60), || {
        Ok(items(&history(&rig, &project)?)
            .iter()
            .any(|item| is_said(item, "assistant", "AFTER_THE_SWITCH")))
    })?;

    let read = history(&rig, &project)?;
    // One page holds it all: nothing older, nothing newer, and nothing named.
    assert_eq!(
        (read["older"].clone(), read["newer"].clone()),
        (Value::Null, Value::Null),
        "{read}"
    );
    let said = page_itself(&read).to_string();
    assert_eq!(
        NO_HEADING.into_iter().find(|word| said.contains(word)),
        None,
        "{read}"
    );
    let all = items(&read);
    // Two conversations, one after the other: every item of the Claude one before
    // every item of the Codex one.
    let mut conversations: Vec<i64> = all
        .iter()
        .map(|item| item["conversation"].as_i64().unwrap_or_default())
        .collect();
    let in_order = conversations.windows(2).all(|pair| pair[0] <= pair[1]);
    conversations.dedup();
    assert!(in_order && conversations.len() == 2, "{read}");
    let of = |conversation: i64| -> Vec<&Value> {
        all.iter()
            .filter(|item| item["conversation"] == conversation)
            .collect()
    };
    let (before, now) = (of(conversations[0]), of(conversations[1]));
    // The Claude conversation, whole: what the human typed, the answer, and the summary named.
    assert!(
        holds(&before, "user", "Reply with exactly: BEFORE_THE_SWITCH"),
        "{read}"
    );
    assert!(holds(&before, "assistant", "BEFORE_THE_SWITCH"), "{read}");
    let summary = before
        .iter()
        .find(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(SUMMARY))
        })
        .ok_or("no summary")?;
    assert_eq!(
        (&summary["role"], &summary["compaction"]),
        (&json!("user"), &json!(true))
    );
    // The Codex conversation going on now: the handoff that brought it in, marked as a
    // delivery (where it begins, and no line of its outcome), and its work.
    assert!(
        now.iter().any(|item| item["delivery"]["at"].is_u64()
            && item["delivery"].get("line").is_none()
            && item["text"]
                .as_str()
                .is_some_and(|text| text.contains("You are the chief now"))),
        "{read}"
    );
    assert!(
        holds(&now, "user", "Reply with exactly: AFTER_THE_SWITCH"),
        "{read}"
    );

    // A page goes on across the switch either way, from a place a page said: before
    // the Codex conversation's first item is the Claude one alone, and after the
    // Claude one's last item is the Codex one alone.
    let page = |side: &str, at: &Value| -> cf_e2e::Result<Value> {
        let read = rig.page(
            "history.get",
            json!({ "project": project.id(), "handle": "chief", "tools": true, side: place(at) }),
        )?;
        Ok(read["history"].clone())
    };
    let ids =
        |items: &[&Value]| -> Vec<Value> { items.iter().map(|item| item["id"].clone()).collect() };
    let older = page("before", now[0])?;
    assert_eq!(
        ids(&items(&older).iter().collect::<Vec<_>>()),
        ids(&before),
        "{older}"
    );
    assert_eq!(older["newer"], place(before[before.len() - 1]), "{older}");
    let newer = page("after", before[before.len() - 1])?;
    assert_eq!(
        ids(&items(&newer).iter().collect::<Vec<_>>()),
        ids(&now),
        "{newer}"
    );
    assert_eq!(newer["older"], place(now[0]), "{newer}");

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
