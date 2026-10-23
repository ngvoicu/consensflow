//! A conversation's heading, as a read tells it in a few lookups of the log,
//! is what the log read whole from its start tells (the second round's way,
//! kept here as the oracle), over many histories of windows that open fresh,
//! go to another conversation or back to one, switches of the chief, and ends,
//! of the chief and of a member.

use std::collections::{HashMap, HashSet};

use cf_ledger::{ChiefSwitch, ConversationEnd, EventView, Ledger, NewMember};
use serde_json::Value;

use super::{everything, id_of, ledger_in, project};

/// What the log, read whole from its start, says of the conversations `own`:
/// the agent each began on, and what ended each the last time it ended.
fn told(
    events: &[EventView],
    own: &HashSet<i64>,
    chief: bool,
    now: Option<&str>,
) -> (HashMap<i64, Option<String>>, HashMap<i64, ConversationEnd>) {
    let events: Vec<&EventView> = events
        .iter()
        .filter(|event| {
            [
                "conversation.started",
                "conversation.followed",
                "conversation.ended",
            ]
            .contains(&event.kind.as_str())
                || (chief && event.kind == "chief.switched")
        })
        .collect();
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let switched_from: Vec<Option<String>> = events
        .iter()
        .filter(|event| event.kind == "chief.switched")
        .map(|event| text(&event.data["from"]["agent"]))
        .collect();
    let mut agents = HashMap::new();
    let mut ends = HashMap::new();
    let mut switches = 0;
    let mut current: Option<i64> = None;
    for event in events {
        if event.kind == "chief.switched" {
            if let Some(ended) = current.take() {
                let to = &event.data["to"];
                let end = ConversationEnd::Switch {
                    harness: text(&to["harness"]).unwrap_or_default(),
                    agent: text(&to["agent"]),
                };
                ends.insert(ended, end);
            }
            switches += 1;
            continue;
        }
        let Some(id) = event.data["conversation"]
            .as_i64()
            .filter(|id| own.contains(id))
        else {
            continue;
        };
        if event.kind == "conversation.ended" {
            if current == Some(id) {
                current = None;
            }
            ends.remove(&id);
            continue;
        }
        let seen = agents.contains_key(&id);
        if let Some(before) = current.filter(|before| *before != id) {
            let end = if event.kind == "conversation.started" {
                ConversationEnd::Fresh
            } else {
                ConversationEnd::Window { resumed: seen }
            };
            ends.insert(before, end);
        }
        if !seen {
            let agent = match switched_from.get(switches) {
                Some(from) => from.clone(),
                None => now.map(str::to_owned),
            };
            agents.insert(id, agent);
        }
        current = Some(id);
    }
    (agents, ends)
}

/// A die that rolls the same numbers for the same seed.
struct Die(u64);

impl Die {
    fn roll(&mut self, faces: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as usize % faces
    }
}

/// A participant as the history is made: its handle, its id, its harness and
/// agent now, and the sessions its conversations were bound to.
struct Who {
    handle: &'static str,
    id: i64,
    harness: &'static str,
    agent: &'static str,
    sessions: Vec<String>,
}

/// One step of a history, as `die` rolls it, for the chief or the member.
fn step(ledger: &mut Ledger, project: i64, who: &mut Who, die: &mut Die, sessions: &mut usize) {
    *sessions += 1;
    let fresh = format!("s-{sessions}");
    match die.roll(if who.handle == "chief" { 6 } else { 5 }) {
        0 => {
            ledger
                .start_conversation(who.id, who.harness)
                .expect("started");
        }
        1 => {
            ledger
                .follow_conversation(who.id, who.harness, &fresh)
                .expect("cleared");
            who.sessions.push(fresh);
        }
        2 if !who.sessions.is_empty() => {
            let back = who.sessions[die.roll(who.sessions.len())].clone();
            ledger
                .follow_conversation(who.id, who.harness, &back)
                .expect("resumed");
        }
        3 => {
            if let Some(now) = ledger.current_conversation(who.id).expect("now") {
                ledger.end_conversation(now.id).expect("ended");
            }
        }
        4 => {
            if let Some(now) = ledger.current_conversation(who.id).expect("now") {
                if now.native_session.is_none() {
                    ledger.bind_conversation(now.id, &fresh).expect("bound");
                    who.sessions.push(fresh);
                }
            }
        }
        5 => {
            (who.harness, who.agent) = if who.harness == "codex" {
                ("claude-code", "ares")
            } else {
                ("codex", "maia")
            };
            ledger
                .switch_chief(
                    project,
                    &ChiefSwitch {
                        harness: who.harness.into(),
                        agent: who.agent.into(),
                        cut: false,
                    },
                )
                .expect("switched");
        }
        _ => {}
    }
}

#[test]
fn every_heading_says_what_the_whole_log_says_of_its_conversation() {
    for seed in 0..30 {
        let dir = tempfile::tempdir().expect("a folder");
        let mut ledger = ledger_in(&dir);
        let project = project(&mut ledger);
        let member = ledger
            .add_member(
                project.id,
                &NewMember {
                    agent: "zeus".into(),
                    harness: "pi".into(),
                    designer: false,
                    roles: vec!["worker".into()],
                    tier: "standard".into(),
                },
            )
            .expect("a member");
        let mut chief = Who {
            handle: "chief",
            id: id_of(&project, "chief"),
            harness: "claude-code",
            agent: "ares",
            sessions: Vec::new(),
        };
        let mut zeus = Who {
            handle: "zeus",
            id: member.id,
            harness: "pi",
            agent: "zeus",
            sessions: Vec::new(),
        };
        let mut die = Die(seed);
        let mut sessions = 0;
        for _ in 0..40 {
            let who = if die.roll(3) == 0 {
                &mut zeus
            } else {
                &mut chief
            };
            step(&mut ledger, project.id, who, &mut die, &mut sessions);
        }
        let events = ledger.events(project.id, 0, 100_000).expect("the log");
        for who in [&chief, &zeus] {
            let history = everything(&ledger, project.id, who.handle).expect("the history");
            let own: HashSet<i64> = history
                .iter()
                .map(|(conversation, _)| conversation.conversation.id)
                .collect();
            let (agents, ends) = told(&events, &own, who.handle == "chief", Some(who.agent));
            for (heading, _) in &history {
                let id = heading.conversation.id;
                let agent = agents
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| Some(who.agent.to_owned()));
                let ended_by = heading
                    .conversation
                    .ended_at
                    .as_ref()
                    .and_then(|_| ends.get(&id).cloned());
                assert_eq!(
                    (&heading.agent, &heading.ended_by),
                    (&agent, &ended_by),
                    "seed {seed}: {}'s conversation {id}",
                    who.handle
                );
            }
        }
    }
}
