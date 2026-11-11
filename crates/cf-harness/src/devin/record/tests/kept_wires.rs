//! The rule of a turn's opening and end, folded over Devin's own wire logs as they were
//! kept (T-1423): the ones the live runs of T-1417 and T-1423 wrote (Devin 3000.12.43) and
//! the kit's of 2026-10-21 and 2026-11-05 (Devin 3000.11). The logs hold their owner's
//! paths and words and are not in the repository: `CF_DEVIN_KEPT_WIRES` names a file that
//! lists them, one path to a line, and the test is run by hand
//! (`cargo test -p cf-harness kept_wires -- --ignored --nocapture`).
//!
//! At every line of every log the window's reading of the turn (`Turns`) is held to what
//! the log says: a turn is open from a message said back live until the next `cause` of its
//! session, and closed from that `cause`; the deliveries said back are those of the open
//! turn. What it prints: the lines and messages of each log, the ones said back live, the
//! turns a log ends with open (a run cut, or a turn at work when it was kept), and any
//! message of 3000.11's that would open a turn by the rule (none is meant to).

use std::collections::HashMap;
use std::fs;

use serde_json::Value;

use super::super::wires::{said_back_live, Turns};

#[test]
#[ignore = "run by hand over the kept logs: CF_DEVIN_KEPT_WIRES names their list"]
fn every_kept_devin_wire_log_reads_its_turns_open_from_the_message_said_back_to_their_end() {
    let env = cf_base::env::Env::from_process();
    let list = env
        .path("CF_DEVIN_KEPT_WIRES")
        .expect("CF_DEVIN_KEPT_WIRES names the list");
    let paths: Vec<String> = fs::read_to_string(list)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    let mut wrong = Vec::new();
    let (mut lines_read, mut live, mut said_old, mut left_open) = (0, 0, 0, 0);
    let (mut claimed, mut dropped) = (0, 0);
    for path in &paths {
        let mut turns = Turns::default();
        // What the log says of each session: a message said back live since its last cause.
        let mut open: HashMap<String, bool> = HashMap::new();
        let mut taken: HashMap<String, Vec<i64>> = HashMap::new();
        let mut unclaimed: HashMap<String, Vec<String>> = HashMap::new();
        let text = fs::read_to_string(path).unwrap();
        let (live_before, claimed_before, dropped_before) = (live, claimed, dropped);
        for (at, line) in text.lines().enumerate() {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            lines_read += 1;
            turns.follow(&event);
            let Some(session) = event.get("sessionId").and_then(Value::as_str) else {
                continue;
            };
            let session = session.to_owned();
            let user = event["update"]["sessionUpdate"] == "user_message_chunk";
            if let Some(text) = said_back_live(&event) {
                live += 1;
                open.insert(session.clone(), true);
                if let Some(message) = crate::contract::delivery_of(text) {
                    taken.entry(session.clone()).or_default().push(message);
                }
            } else if user
                && event["update"]["_meta"]
                    .get("cognition.ai/eventType")
                    .is_none()
                && event["update"]["_meta"]
                    .get("cognition.ai/eventId")
                    .is_none()
            {
                said_old += 1;
                unclaimed.entry(session.clone()).or_default().push(
                    event["update"]["content"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                );
            }
            let read = turns.open(&session);
            // An older Devin's message said back is the turn's once a turn is open around it,
            // before the next turn end or `turnClientMessageId`; else it was a replay's.
            if read {
                for text in unclaimed.remove(&session).unwrap_or_default() {
                    claimed += 1;
                    if let Some(message) = crate::contract::delivery_of(&text) {
                        taken.entry(session.clone()).or_default().push(message);
                    }
                }
            }
            if event.get("turnClientMessageId").is_some() || event.get("cause").is_some() {
                dropped += unclaimed.remove(&session).map_or(0, |left| left.len());
            }
            if event.get("cause").is_some() {
                open.insert(session.clone(), false);
                taken.remove(&session);
            }
            let meant_open = open.get(&session).copied().unwrap_or(false);
            // Open by a message said back: the reading is open. Closed by a cause: closed.
            // Open by other work (`working`, a streamed word) the oracle does not count: only
            // a cause is held to closing it.
            if meant_open && !read {
                wrong.push(format!("{path}:{}: said back, read closed", at + 1));
            }
            if event.get("cause").is_some() && read {
                wrong.push(format!("{path}:{}: its cause, read open", at + 1));
            }
            let mut said = turns.said_back(&session);
            let mut meant = if read {
                taken.get(&session).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            said.sort_unstable();
            meant.sort_unstable();
            if said != meant {
                wrong.push(format!(
                    "{path}:{}: took in {said:?}, the log says {meant:?}",
                    at + 1
                ));
            }
        }
        let still: Vec<&String> = open
            .iter()
            .filter(|(_, open)| **open)
            .map(|(session, _)| session)
            .collect();
        left_open += still.len();
        eprintln!(
            "{path}: {} lines, {} said back live, {} said back unmarked and taken by a turn, {} \
             dropped as a replay's; ends with {} turn(s) said back live and not ended",
            text.lines().count(),
            live - live_before,
            claimed - claimed_before,
            dropped - dropped_before,
            still.len()
        );
    }
    eprintln!(
        "{} logs, {lines_read} lines; {live} messages said back live, {said_old} said back with \
         no event type ({claimed} taken by a turn, {dropped} dropped), {left_open} turn(s) open \
         at a log's end",
        paths.len()
    );
    assert!(
        wrong.is_empty(),
        "{} lines:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
