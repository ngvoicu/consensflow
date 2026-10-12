//! The chief's window, which a check plays: `cf` run as the chief runs it, and,
//! when `cf` is refused for want of access, what became of the window. The daemon
//! revokes a window's token when its program exits, and a chief that exits of
//! itself takes its project with it (the project is suspended, the members'
//! windows closed), so a refusal that says the window "may have closed" is a
//! chief that is gone, and the way it went is what a failure has to say.

use std::ffi::OsStr;

use serde_json::Value;

use super::{clock, session_of, Members};
use crate::live::{last_lines, warnings};
use crate::process::Ran;
use crate::{Error, Result};

/// What `cf` says when the token it runs with is one the daemon does not know.
const NO_ACCESS: &str = "has no ConsensFlow access";

impl Members {
    /// `cf` as the chief runs it: the chief's own window's environment and token.
    /// A chief opened again by the human's Resume after a restart is the newest
    /// window of its pane. A refusal for want of access is an error that says
    /// what became of the chief's window ([`Members::chief_window`]).
    pub fn as_chief<I, S>(&self, words: I) -> Result<Ran>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let words: Vec<String> = words
            .into_iter()
            .map(|word| word.as_ref().to_string_lossy().into_owned())
            .collect();
        let pane = self.chief_pane();
        let frame = self
            .rig
            .open_frames()
            .into_iter()
            .rev()
            .find(|frame| frame["id"] == pane.as_str())
            .ok_or_else(|| Error::Daemon(format!("the chief's window {pane} was never opened")))?;
        let ran = self.rig.cf_in_window(&frame, &words)?;
        if ran.code != Some(0) && ran.stderr.contains(NO_ACCESS) {
            return Err(Error::Daemon(format!(
                "cf {}: {}; {}",
                words.join(" "),
                ran.stderr.trim(),
                self.chief_window()
            )));
        }
        Ok(ran)
    }

    /// What became of the chief's window, said for a failure: how many times the
    /// daemon opened it and how each opened one ended (the pane host's word: its
    /// code or signal, and what its screen showed last), what the daemon wrote of
    /// it, and what it warned of.
    pub fn chief_window(&self) -> String {
        let pane = self.chief_pane();
        let frames: Vec<Value> = self
            .rig
            .open_frames()
            .into_iter()
            .filter(|frame| frame["id"] == pane.as_str())
            .collect();
        let exits: Vec<Value> = self
            .rig
            .exits()
            .into_iter()
            .filter(|exit| exit["id"] == pane.as_str())
            .collect();
        let warned = warnings(&self.rig.log()).join(" | ");
        let told = story(
            &pane,
            &frames,
            &exits,
            &self.events(),
            if warned.is_empty() { "none" } else { &warned },
        );
        match frames.last().and_then(|frame| self.records_of(frame)) {
            Some(records) => format!("{told}; its records: {records}"),
            None => told,
        }
    }

    /// What the stand-in that is the chief has of the files the daemon reads of it, where
    /// the window `frame` is one: the conversation it writes and the status file that
    /// says what it is doing, in the Claude folder it writes in.
    fn records_of(&self, frame: &Value) -> Option<String> {
        let session = session_of(frame)?;
        let started = self
            .rig
            .started()
            .into_iter()
            .find(|started| started.session == session)?;
        let config = self.claude_config();
        let conversation = config
            .join("projects")
            .join("integration")
            .join(format!("{session}.jsonl"));
        let status = config
            .join("sessions")
            .join(format!("{}.json", started.pid));
        Some(records_line(
            std::fs::read_to_string(conversation)
                .ok()
                .map(|text| text.lines().count()),
            std::fs::read_to_string(status)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .map(|status| status["status"].as_str().unwrap_or("undefined").to_owned()),
        ))
    }

    /// The id of the chief's pane.
    fn chief_pane(&self) -> String {
        format!("p{}-chief", self.project)
    }
}

/// The files of a stand-in said in a line: how many records its conversation holds
/// and what its status file says, or that they are not there.
fn records_line(records: Option<usize>, status: Option<String>) -> String {
    format!(
        "the conversation {}, the status file {}",
        records.map_or_else(
            || "is gone".to_owned(),
            |count| format!("holds {count} lines")
        ),
        status.map_or_else(|| "is gone".to_owned(), |status| format!("says {status}"))
    )
}

/// How a window's program ended, in the pane host's words: its code, or the
/// signal that ended it.
fn how_it_ended(exit: &Value) -> String {
    match (exit["exitCode"].as_i64(), exit["signal"].as_str()) {
        (Some(code), _) => format!("code {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        (None, None) => "neither a code nor a signal".to_owned(),
    }
}

/// What a window's screen last showed when it ended: its last lines, as one.
fn what_it_showed(exit: &Value) -> String {
    let lines: Vec<&str> = exit["tail"]
        .as_array()
        .map(|tail| tail.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    last_lines(&lines.join("\n"))
}

/// The line of a window's own event of the daemon's: its number, its time, its
/// kind and what it says (`state`, `reason`).
fn event_line(event: &Value) -> String {
    let said: Vec<String> = ["state", "reason"]
        .into_iter()
        .filter_map(|field| event[field].as_str().map(|text| format!("{field} {text}")))
        .collect();
    format!(
        "{} {} {}{}",
        event["n"],
        clock(event["at"].as_str().unwrap_or_default()),
        event["kind"].as_str().unwrap_or_default(),
        if said.is_empty() {
            String::new()
        } else {
            format!(" ({})", said.join(", "))
        }
    )
}

/// The story of the chief's window `pane` from what the rig saw of it: the
/// `frames` it was opened with, the `exits` the pane host told, the daemon's
/// `events` of its chief, and its `warned` lines.
fn story(pane: &str, frames: &[Value], exits: &[Value], events: &[Value], warned: &str) -> String {
    let opened: Vec<String> = frames
        .iter()
        .map(|frame| {
            let generation = &frame["generation"];
            match exits.iter().find(|exit| exit["generation"] == *generation) {
                Some(exit) => format!(
                    "generation {generation} ended with {}, its screen ending: {}",
                    how_it_ended(exit),
                    what_it_showed(exit)
                ),
                None => format!("generation {generation} has not ended, as the pane host tells"),
            }
        })
        .collect();
    let wrote: Vec<String> = events
        .iter()
        .filter(|event| event["participant"] == "chief")
        .filter(|event| {
            let kind = event["kind"].as_str().unwrap_or_default();
            kind.starts_with("window.") || kind.starts_with("delivery.")
        })
        .map(event_line)
        .collect();
    format!(
        "the chief's window {pane} was opened {} time{}: {}; the daemon wrote of it: {}; its warnings: {warned}",
        frames.len(),
        if frames.len() == 1 { "" } else { "s" },
        if opened.is_empty() {
            "never".to_owned()
        } else {
            opened.join("; ")
        },
        if wrote.is_empty() {
            "nothing".to_owned()
        } else {
            wrote.join("; ")
        }
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn frame(generation: u64) -> Value {
        json!({ "id": "p1-chief", "generation": generation })
    }

    #[test]
    fn the_files_of_a_stand_in_are_said_by_what_they_hold_and_say_or_as_gone() {
        assert_eq!(
            records_line(Some(4), Some("idle".to_owned())),
            "the conversation holds 4 lines, the status file says idle"
        );
        assert_eq!(
            records_line(None, None),
            "the conversation is gone, the status file is gone"
        );
    }

    #[test]
    fn a_window_ended_by_a_code_or_by_a_signal_or_by_neither_says_so() {
        assert_eq!(
            how_it_ended(&json!({ "exitCode": 1, "signal": null })),
            "code 1"
        );
        assert_eq!(how_it_ended(&json!({ "exitCode": 0 })), "code 0");
        assert_eq!(
            how_it_ended(&json!({ "exitCode": null, "signal": "SIGHUP" })),
            "signal SIGHUP"
        );
        assert_eq!(how_it_ended(&json!({})), "neither a code nor a signal");
    }

    #[test]
    fn what_the_screen_showed_is_its_last_lines_and_none_where_it_showed_none() {
        let exit = json!({ "tail": ["fake agent started on s", "", "fake-agent: no space left"] });
        assert_eq!(
            what_it_showed(&exit),
            "fake agent started on s ⏎ fake-agent: no space left"
        );
        assert_eq!(what_it_showed(&json!({})), "");
    }

    #[test]
    fn the_story_of_a_window_that_ended_says_when_how_and_what_the_daemon_wrote_of_it() {
        let exits = [json!({
            "id": "p1-chief", "generation": 7, "exitCode": 1, "signal": null,
            "tail": ["fake-agent: write failed"],
        })];
        let events = [
            json!({ "n": 12, "at": "2026-10-12T00:08:36.925Z", "kind": "window.activity", "participant": "chief", "state": "idle" }),
            json!({ "n": 13, "at": "2026-10-12T00:08:37.000Z", "kind": "window.activity", "participant": "worker-a", "state": "working" }),
            json!({ "n": 30, "at": "2026-10-12T00:08:58.100Z", "kind": "window.activity", "participant": "chief", "state": "closed" }),
            json!({ "n": 31, "at": "2026-10-12T00:08:58.200Z", "kind": "task.state", "participant": "chief" }),
        ];
        assert_eq!(
            story("p1-chief", &[frame(7)], &exits, &events, "none"),
            "the chief's window p1-chief was opened 1 time: generation 7 ended with code 1, \
             its screen ending: fake-agent: write failed; the daemon wrote of it: \
             12 00:08:36.925 window.activity (state idle); 30 00:08:58.100 window.activity (state closed); \
             its warnings: none"
        );
    }

    #[test]
    fn the_story_of_a_window_opened_twice_and_still_there_says_which_has_not_ended() {
        let exits = [
            json!({ "id": "p1-chief", "generation": 7, "exitCode": null, "signal": "SIGTERM", "tail": [] }),
        ];
        let told = story("p1-chief", &[frame(7), frame(9)], &exits, &[], "x");
        assert!(
            told.contains(
                "opened 2 times: generation 7 ended with signal SIGTERM, its screen ending: ;"
            ),
            "{told}"
        );
        assert!(
            told.contains("generation 9 has not ended, as the pane host tells"),
            "{told}"
        );
        assert!(told.contains("the daemon wrote of it: nothing"), "{told}");
        assert!(story("p1-chief", &[], &[], &[], "none").contains("was opened 0 times: never;"));
    }
}
