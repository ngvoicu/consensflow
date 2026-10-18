//! One case of one harness: a window on a message that has the harness work, a
//! kill of its process with the turn at work (`pane.kill`, which the pane host
//! does to each window as the app quits), and then a window on `resume` of the
//! conversation, opened with the app's own arguments, and watched: the harness's
//! own status where it keeps one (Claude's), what it writes to its record, its
//! screen, and how the daemon's record reader reads the record. At the end a
//! second message is pasted, to show that the window takes it.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::ask::{self, Record};
use cf_e2e::live::claude::{record, runs, transcript_file, Status};
use cf_e2e::live::members::{end_chosen, find, Account, Ended, Identity, ThisMachine};
use cf_e2e::live::window::{Open, Window, ANSWER, READY};
use cf_e2e::live::{seconds, until, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{files, Result};
use serde_json::Value;

use crate::read::{change, reading, tail, unanswered};
use crate::{Case, FOLDER, LONG, SECOND, WATCH, WORK};

/// What a case needs of the check that runs it.
pub struct Run<'a> {
    pub rig: &'a Rig,
    pub install: &'a Installed,
    /// The agent the harness's windows run: Claude's model is the one `--model` names,
    /// the others' their cheap model.
    pub agent: Value,
    /// Where each Claude transcript is copied (`--transcripts`).
    pub transcripts: Option<&'a Path>,
    pub log: &'a Log,
}

impl<'a> Run<'a> {
    fn harness(&self) -> Harness {
        self.install.harness()
    }

    /// The window `id`, on the agent: a new conversation, or `resume` of the one
    /// `session` names, as the app opens it. It is killed when it is dropped.
    fn window(&self, id: &str, session: Option<&str>) -> Result<Window<'a>> {
        let open = Open::new(self.rig, self.install, FOLDER, id).agent(self.agent.clone());
        match session {
            Some(session) => open.resume(session).open(self.log),
            None => open.open(self.log),
        }
    }

    /// How the daemon's own reader reads the record of `session`.
    fn read(&self, session: &str) -> Option<Record> {
        let kind = self.agent["kind"].as_str().unwrap_or_default();
        ask::records(kind, session, self.rig.environment())
            .ok()
            .flatten()
    }
}

/// The long command is running in the window whose own process is `own`: a process
/// that descends from it names the command's mark (the shell the harness runs it in
/// does, as the command itself does). A process outside the window is none of it.
fn command_running(own: Option<Identity>) -> bool {
    own.is_some_and(|own| {
        LONG.in_window(&ThisMachine, own)
            .is_ok_and(|found| !found.is_empty())
    })
}

/// What the copy of a transcript is called in the folder `--transcripts` names:
/// `<case>-<when>-<session>.jsonl`, `when` being `cut` (as the cut left it) or
/// `resumed` (as it ended).
pub fn kept_name(case: Case, when: &str, session: &str) -> PathBuf {
    PathBuf::from(format!("{}-{when}-{session}.jsonl", case.name()))
}

/// Copies Claude's transcript of `session` as it is now into the folder `--transcripts`
/// names, where it names one.
fn keep(run: &Run, window: &Window, (case, when): (Case, &str), session: &str) -> Result<()> {
    let Some(folder) = run.transcripts else {
        return Ok(());
    };
    if let Some(file) = transcript_file(&window.claude_config(), session) {
        files::copy(&file, &folder.join(kept_name(case, when, session)))?;
    }
    Ok(())
}

/// Runs `case` on the harness: what each step saw, which holds where the case
/// could be run (a window that did not open, or a turn that was never at work,
/// cannot): what a harness does with the turn is the result, and it is said.
pub fn run(run: &Run, case: Case) -> Result<Account> {
    let mut said = Account::new();
    // What the cut left of the long command is ended after the cut, and what the window
    // opened again runs of it at the end, each in its own window's tree: a case that fails
    // on the way leaves it to end by itself, in five minutes.
    drive(run, case, &mut said)?;
    Ok(said)
}

/// The steps of a case: the window, the cut, the window again, and a second message.
fn drive(run: &Run, case: Case, said: &mut Account) -> Result<()> {
    let claude = run.harness() == Harness::Claude;
    let id = format!("resume-cut-{}-{}", run.harness().name(), case.name());
    let cut = run.window(&id, None)?;
    if let Err(screen) = cut.ready() {
        said.fail(format!("the window did not open: {screen}"));
        return Ok(());
    }
    // Which process the window's program is, read as the pane host answered its open.
    let own = cut.process();
    let watching = cut
        .session()
        .filter(|_| claude)
        .map(|session| Status::watch(&cut.claude_config(), &session));
    cut.paste(&cut.given(&case.message())?)?;
    // `early`: the message is in the record and nothing of the answer is: Claude says so by
    // its status, the others by what their record holds.
    let at_work = find(
        run.log,
        "the turn to be at work",
        WORK,
        Duration::from_millis(100),
        || {
            Ok(match (case, &watching) {
                (Case::Command, _) => command_running(own).then_some(()),
                (Case::Early, None) => {
                    unanswered(&cut.recorded(), &Case::Early.message()).then_some(())
                }
                (Case::Early, Some(status)) => status
                    .samples()
                    .iter()
                    .any(|sample| sample.status.as_deref() == Some("busy"))
                    .then_some(()),
            })
        },
    )?;
    drop(watching);
    if at_work.is_none() {
        said.fail(format!(
            "the turn was never at work in {} s: {}",
            WORK.as_secs(),
            cut.tail()
        ));
        return Ok(());
    }
    if case == Case::Command {
        sleep(Duration::from_secs(2));
    }
    // The harnesses that name their own conversation have named it by now.
    cut.recorded();
    let Some(session) = cut.session() else {
        said.fail(format!("no conversation was recorded: {}", cut.tail()));
        return Ok(());
    };

    // The quit: the window's process ends with its turn at work. What the window ran of
    // the command is chosen while it is the window's, and what the cut left of it is
    // ended after, each process only while it is still the one chosen: what descended
    // from the window has no window to descend from once it is cut.
    let left = own.map(|own| LONG.chosen_in(&ThisMachine, own));
    cut.kill();
    until(Duration::from_secs(30), Duration::from_millis(100), || {
        cut.closed().then_some(())
    });
    if let Some(left) = left {
        said_ended(said, "the cut window", left.map(|left| end_chosen(&left)));
    }
    sleep(Duration::from_secs(1));
    said.say(format!("cut: {}", tail(&cut.recorded(), 3)));
    said.say(reading(run.read(&session).as_ref()));
    if claude {
        keep(run, &cut, (case, "cut"), &session)?;
    }

    // The daemon that starts again: the window on the conversation, as the app opens it.
    let before = cut.recorded().len();
    let again = run.window(&format!("{id}-again"), Some(&session))?;
    let opened_ms = jiff::Timestamp::now().as_millisecond();
    let watching = claude.then(|| Status::watch_running(&again.claude_config(), &session));
    let opened = Instant::now();
    let drew = again.still(READY);
    said.say(format!(
        "resumed: the window {} {} s after it opened",
        if drew {
            "drew its screen"
        } else {
            "did not hold still"
        },
        seconds(millis(opened))
    ));
    sleep(WATCH);
    if let Some(status) = watching {
        said.say(format!(
            "its status: {}",
            runs(&status.stop(), opened_ms).join(" → ")
        ));
    }
    said.say(change(before, &again.recorded()));
    said.say(reading(run.read(&session).as_ref()));
    said.say(format!("its screen: {}", again.tail()));

    // A message as the daemon pastes one: does the window take it, and alone?
    again.paste(&again.given(SECOND)?)?;
    let pasted = Instant::now();
    let arrived = find(
        run.log,
        "the second message to reach the record",
        Duration::from_secs(60),
        Duration::from_millis(500),
        || {
            Ok(again
                .recorded()
                .into_iter()
                .rev()
                .find(|item| item.role == "user" && item.text.contains("AFTER")))
        },
    )?;
    let answered = find(
        run.log,
        "the second message to be answered",
        ANSWER,
        Duration::from_millis(500),
        || {
            Ok(again
                .recorded()
                .into_iter()
                .find(|item| item.role == "assistant" && item.text.trim() == "AFTER"))
        },
    )?;
    said.say(format!(
        "a second message pasted: {}{}",
        arrived.map_or_else(
            || "it never arrived".to_owned(),
            |item| format!(
                "it arrived as {}",
                Value::String(item.text.chars().take(120).collect())
            )
        ),
        answered.map_or_else(
            || ", and was not answered".to_owned(),
            |_| format!(", and was answered in {} s", seconds(millis(pasted)))
        )
    ));
    // The turn that message began ended as any does: the record is at rest.
    sleep(Duration::from_secs(3));
    said.say(reading(run.read(&session).as_ref()));
    if claude {
        let closed = record(&again.claude_config(), &session)
            .into_iter()
            .find(|entry| {
                entry.kind == "assistant" && entry.raw.contains("\"model\":\"<synthetic>\"")
            });
        said.say(match closed {
            Some(entry) => format!(
                "what Claude wrote of the cut turn, once the second message came: {}: {}",
                entry.kind, entry.text
            ),
            None => "Claude wrote nothing of the cut turn".to_owned(),
        });
        keep(run, &again, (case, "resumed"), &session)?;
    }
    // A run of the long command the window opened again started is not left behind.
    if let Some(own) = again.process() {
        said_ended(said, "the window opened again", LONG.end_in(own));
    }
    Ok(())
}

/// What could not be ended of the long command `window` ran, a failure of the case each:
/// a process that may still run, or a listing of the machine that could not be read.
fn said_ended(said: &mut Account, window: &str, ended: Result<Ended>) {
    match ended {
        Ok(ended) => {
            for problem in ended.problems() {
                said.fail(format!("what {window} ran of the long command: {problem}"));
            }
        }
        Err(failed) => said.fail(format!(
            "what {window} ran of the long command could not be read, and may still run: {failed}"
        )),
    }
}

/// The milliseconds since `since`.
fn millis(since: Instant) -> i64 {
    i64::try_from(since.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_transcript_is_named_by_the_case_when_it_was_copied_and_its_conversation() {
        assert_eq!(
            kept_name(Case::Command, "cut", "s-1"),
            PathBuf::from("command-cut-s-1.jsonl")
        );
        assert_eq!(
            kept_name(Case::Early, "resumed", "s-2"),
            PathBuf::from("early-resumed-s-2.jsonl")
        );
    }
}
