//! One case, step by step: a member's Devin window opened as the daemon opens one,
//! cut as the app's quit cuts it (`pane.kill`, which the pane host does to each window
//! as the app quits), opened again on its conversation as the daemon opens it after
//! the quit (`--resume`, the launch the adapter prepares, with the note the engine
//! writes that its turn was cut where the case gives it one), brought back as the
//! case has it ([`crate::back`]), and then asked to write, edit and run `cf`. What
//! each step saw is said as it came; what Devin does is the result, and the case
//! holds as long as it could be run.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::devin_store;
use cf_e2e::live::ask::{self, Interrupt};
use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::members::{squash, Account, Identity, ThisMachine};
use cf_e2e::live::window::{Member, Open, Window};
use cf_e2e::live::{keep_requested, secs, until, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{files, Error, Result};
use serde_json::Value;

use crate::asks::{holds, put, Ask, Place};
use crate::back;
use crate::case::{Case, Cut, SECOND};
use crate::life::Life;
use crate::outcome::{sightings_said, summary, turn_line, Outcome};
use crate::scene::Scene;
use crate::turn::{reading, tail, Turn};
use crate::words::{config_summary, marker_of};
use crate::{FOLDER, LONG, WORK};

/// The number of the first ask, and so of the others after it.
const FIRST_ASK: u32 = 10;

/// What a case needs of the check that runs it.
pub struct Run<'a> {
    pub rig: &'a Rig,
    pub install: &'a Installed,
    pub agent: Value,
    /// The folder of the stand-in `cf`, whose log says what ran.
    pub calls: &'a Path,
    pub log: &'a Log,
    /// What is left of the case's time.
    pub life: Life,
    /// The keys that interrupt a turn, as the adapter says them.
    pub interrupt: Interrupt,
}

/// What a case came to: what it saw, the mode the window opened again had (from the
/// first the wire said to the last), and the asks it made.
pub struct Done {
    pub account: Account,
    pub mode: String,
    pub outcomes: Vec<Outcome>,
}

impl Done {
    /// A case that has seen nothing yet.
    pub fn new() -> Self {
        Self {
            account: Account::new(),
            mode: "-".to_owned(),
            outcomes: Vec::new(),
        }
    }
}

impl Run<'_> {
    /// The member's window `id` in the case's folder: a new one, or the one that goes
    /// on with the conversation `resume`, with `member` its launch. It is killed when
    /// it is dropped.
    fn window(
        &self,
        folder: &str,
        id: &str,
        member: Member,
        resume: Option<&str>,
    ) -> Result<Window<'_>> {
        let open = Open::new(self.rig, self.install, folder, id)
            .agent(self.agent.clone())
            .member(member);
        match resume {
            Some(session) => open.resume(session).open(self.log),
            None => open.open(self.log),
        }
    }

    /// Where Devin keeps its store, in the home the windows have.
    fn store(&self) -> PathBuf {
        let home = self
            .rig
            .var("HOME")
            .or_else(|| self.rig.var("USERPROFILE"))
            .unwrap_or_default();
        devin_store::location(
            cfg!(windows),
            home,
            self.rig.var("APPDATA"),
            self.rig.var("XDG_DATA_HOME"),
        )
    }

    /// A scene of `window`, watched in Devin's store and the wire log of its launch.
    pub fn scene<'w>(&'w self, window: &'w Window<'w>) -> Scene<'w> {
        Scene::new(
            self.rig,
            window,
            self.store(),
            wire_of(window.argv()),
            self.life,
        )
    }
}

/// The wire log of a launch, which is beside its config: the config is the word
/// after `--config` in the command line the adapter prepared.
pub fn wire_of(argv: &[String]) -> Option<PathBuf> {
    let at = argv.iter().position(|word| word == "--config")?;
    Some(Path::new(argv.get(at + 1)?).with_file_name("wire.jsonl"))
}

/// What a window was opened on, as a line: its command line (the prompt of its first
/// message is in a file, and not in it) and what its config says of its hooks.
fn launch_line(window: &Window) -> String {
    let argv = window.argv();
    let config = argv
        .iter()
        .position(|word| word == "--config")
        .and_then(|at| argv.get(at + 1))
        .and_then(|file| files::read_string(Path::new(file)).ok())
        .map_or_else(|| "no config".to_owned(), |text| config_summary(&text));
    format!("{}; {config}", squash(&argv.join(" "), 420))
}

/// Which process the program of the window of `scene` is, read as the window opened:
/// what descends from it is the window's own tree.
fn window_process(scene: &Scene) -> Result<Identity> {
    scene.own.ok_or_else(|| {
        Error::Daemon(
            "the window's process could not be told: the pane host said none, or the machine knew none of its number"
                .to_owned(),
        )
    })
}

/// Whether the window of `scene` runs the long command: a process that descends from the
/// window's own and holds the command's mark. A process outside the window that holds it
/// (an editor, the worker that runs the check) is none of it. The check looked among all
/// of the machine's before, with `ps`, which Windows has none of, so no case could run
/// there (zeewin, 2026-10-16), and which found the worker whose brief quoted the mark.
/// Nothing of the command is ended: it ends by itself ([`crate::LONG`]).
pub fn command_running(scene: &Scene) -> Result<bool> {
    Ok(!LONG
        .in_window(&ThisMachine, window_process(scene)?)?
        .is_empty())
}

/// A tag for this run of the case, which makes the names of its files its own.
fn new_tag() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..6].to_owned()
}

/// Runs `case`: what each step saw and the asks it made. The case holds where it
/// could be run: a window that did not open, a turn that never was at work, or one
/// that never came to rest where it was to be cut or asked, cannot be, and what
/// Devin does with the rest is the result.
pub fn run(run: &Run, case: Case) -> Done {
    let mut done = Done::new();
    let folder = format!("{FOLDER}-{}", case.name());
    let home = run.rig.var("HOME").unwrap_or_default();
    match live_folder_in(Path::new(home), &folder) {
        Ok(workspace) => {
            // Its windows go with it, and their pane host ends what is theirs as each closes.
            let outcome = drive(run, case, &folder, &workspace, &mut done);
            if let Err(failed) = outcome {
                done.account.fail(failed);
            }
            // A wait that was cut short by the case's clock says nothing of Devin: the case
            // could not be run to its end.
            if run.life.over() {
                done.account.fail(
                    "the case ran out of its time (a rig that lives past ten minutes is read as one that lost the process the pane host is to leave be): what it saw is cut short",
                );
            }
            // What the case made, and the launches' files with the copies of the owner's
            // config, go with it, unless the run is to be looked at.
            if !keep_requested() {
                let _ = std::fs::remove_dir_all(&workspace);
            }
        }
        Err(failed) => done.account.fail(failed),
    }
    done
}

/// The steps of a case, in `workspace`, the folder its windows run in.
fn drive(run: &Run, case: Case, folder: &str, workspace: &Path, done: &mut Done) -> Result<()> {
    let tag = new_tag();
    let place = Place {
        folder: workspace,
        calls: run.calls,
        tag: &tag,
    };
    // The window, as the daemon opens a member's: its task is in its prompt file.
    let cut = run.window(
        folder,
        folder,
        case.member(Some(case.first_message(&tag))),
        None,
    )?;
    run.log
        .say(format!("{}: its window is open, on its task", case.name()));
    done.account.say(format!("opened: {}", launch_line(&cut)));
    let mut scene = run.scene(&cut);
    if !first_turn(case, &mut scene, &place, &mut done.account)? {
        return Ok(());
    }
    run.log.say(format!("{}: cutting its window", case.name()));
    let Some(session) = quit(case, &mut scene, &mut done.account) else {
        return Ok(());
    };
    drop(scene);

    // The daemon that starts again: the window on the conversation, as it opens it,
    // with the note the engine writes for it.
    let (message, task) = Case::note_of();
    let note = ask::cut_note(message, &[task])?;
    let again = run.window(
        folder,
        &format!("{folder}-again"),
        case.member_again(&note),
        Some(&session),
    )?;
    run.log
        .say(format!("{}: its window is open again", case.name()));
    done.account
        .say(format!("opened again: {}", launch_line(&again)));
    let mut scene = run.scene(&again);
    let came = back::come_back(run, case, &mut scene, &mut done.account);
    let outcome = match came {
        Ok(true) => ask_all(run, case, &mut scene, &place, done),
        Ok(false) => Ok(()),
        Err(failed) => Err(failed),
    };
    done.mode = scene.mode_over_time();
    // A window that is held by a dialog is told what the app's interrupt does to it.
    back::release(run, &mut scene, &mut done.account);
    outcome
}

/// The window's first turn: a file written with its shell, and then, where it is cut
/// inside a command, the long one running (in its second turn, after a first that
/// ended, where the case has it so). Whether it could be had.
fn first_turn(case: Case, scene: &mut Scene, place: &Place, said: &mut Account) -> Result<bool> {
    let window = scene.window;
    if case.cut != Cut::InFirstTurn {
        let turn = scene.wait_turn(&marker_of(1), WORK, |_, _, _| Ok(()))?;
        if !turn.ended {
            said.fail(turn_line("its first turn did not end", &turn));
            return Ok(false);
        }
    }
    if case.cut == Cut::InSecondTurn {
        let text = window.given(&Case::second_message())?;
        scene.paste(&text, format!("paste m-{SECOND}"))?;
    }
    if case.in_command() {
        let started = Instant::now();
        let running = until(scene.life.cap(WORK), Duration::from_millis(500), || {
            scene.look();
            let _ = window.record();
            match command_running(scene) {
                Ok(false) if !window.closed() => None,
                looked => Some(looked),
            }
        });
        match running {
            Some(Ok(true)) if !window.closed() => {}
            Some(Err(failed)) => {
                said.fail(format!(
                    "the long command could not be looked for: {failed}"
                ));
                return Ok(false);
            }
            _ => {
                said.fail(format!(
                    "the long command was never run in {} s: {}",
                    secs(started.elapsed()),
                    scene.rows()
                ));
                return Ok(false);
            }
        }
    }
    let tag = place.tag;
    let wrote = holds(
        place.folder,
        &format!("first-{tag}.txt"),
        &format!("first-{tag}"),
    );
    said.say(format!(
        "before the cut its shell {}: {}",
        if wrote.ran { "ran" } else { "did NOT run" },
        wrote.saw
    ));
    said.say(format!(
        "before the cut: {}; its terminal said: {}",
        scene.mode_line(),
        scene.watch.marks.since(0)
    ));
    Ok(true)
}

/// The quit: the window's process ends, with its turn at work (inside the long
/// command) or at rest. The conversation it leaves, or none where no conversation was
/// recorded.
fn quit(case: Case, scene: &mut Scene, said: &mut Account) -> Option<String> {
    let window = scene.window;
    if case.in_command() {
        // Inside the command for a moment, as the model of this check has it.
        sleep(Duration::from_secs(2));
    }
    scene.look();
    let _ = window.record();
    let Some(session) = window.session() else {
        said.fail(format!("no conversation was recorded: {}", scene.rows()));
        return None;
    };
    let before = scene.watch.sightings.len();
    window.kill();
    until(Duration::from_secs(30), Duration::from_millis(100), || {
        window.closed().then_some(())
    });
    sleep(Duration::from_secs(1));
    let record = window.record();
    said.say(format!(
        "cut: {}; {}",
        tail(record.as_ref(), 3),
        reading(record.as_ref())
    ));
    said.say(format!(
        "before the cut: {}",
        sightings_said(&scene.watch.sightings[..before])
    ));
    Some(session)
}

/// Whether the window may be asked again once the wait for this ask is over: its turn
/// ended, or the check ended it (the app's interrupt, for a dialog nobody answered). Any
/// other turn that did not end holds the window.
fn free_after(turn: &Turn) -> bool {
    turn.ended || turn.dialog.is_some()
}

/// The asks, one at a time, each once the window has come to rest with nothing of the
/// long command running (a run of it waited for to end by itself: [`back::settled`]):
/// what each came to is said. A window held by a dialog nobody answered is let go as the
/// human's Stop lets it go (the app's interrupt is pressed into it) and asked the next; a
/// window that is held on an ask otherwise, or never comes to rest, is asked no more.
fn ask_all(run: &Run, case: Case, scene: &mut Scene, place: &Place, done: &mut Done) -> Result<()> {
    let mut held = false;
    for (at, ask) in Ask::ALL.into_iter().enumerate() {
        if held {
            done.account.say(format!(
                "ask {} was not made: the window was still held on the ask before",
                ask.name()
            ));
            continue;
        }
        // The first ask comes once the window that was brought back has come to rest.
        if at > 0 {
            let (rested, reruns) = back::settled(scene);
            if reruns > 0 {
                done.account.say(format!(
                    "before ask {} it ran the long command again {reruns} time(s), and each was let run to its end",
                    ask.name()
                ));
            }
            if !rested {
                done.account.say(format!(
                    "ask {} was not made: the window did not come to rest: {}",
                    ask.name(),
                    scene.rows()
                ));
                held = true;
                continue;
            }
        }
        let number = FIRST_ASK + u32::try_from(at).unwrap_or_default();
        run.log
            .say(format!("{}: asking it for {}", case.name(), ask.name()));
        let outcome = put(scene, place, ask, number)?;
        for line in outcome.lines() {
            done.account.say(line);
        }
        if !outcome.turn.ended && outcome.turn.dialog.is_some() {
            back::release(run, scene, &mut done.account);
        }
        held = !free_after(&outcome.turn);
        done.outcomes.push(outcome);
    }
    done.account.say(format!(
        "at the end: {}; its screen: {}",
        scene.mode_line(),
        scene.rows()
    ));
    done.account.say(summary(&done.outcomes));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_log_is_beside_the_config_the_launch_names() {
        let words = |list: &[&str]| {
            list.iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            wire_of(&words(&[
                "/bin/devin",
                "--config",
                "/h/launch/config.json",
                "--model",
                "m"
            ])),
            Some(PathBuf::from("/h/launch/wire.jsonl"))
        );
        assert_eq!(wire_of(&words(&["/bin/devin", "--model", "m"])), None);
        // A config named last has no file after it.
        assert_eq!(wire_of(&words(&["/bin/devin", "--config"])), None);
    }

    #[test]
    fn the_asks_go_on_once_the_turn_ended_or_the_check_let_a_dialog_go() {
        let turn = Turn {
            arrived: true,
            ended: false,
            closed: false,
            took: Duration::from_secs(40),
            held: None,
            dialog: None,
        };
        // A turn that ended frees the window, and so does one the app's interrupt
        // cancelled for a dialog: the next ask waits for the window to come to rest
        // (`back::settled`).
        let ended = Turn {
            ended: true,
            ..turn.clone()
        };
        let dialog = Turn {
            dialog: Some("blocked on permission".to_owned()),
            ..turn.clone()
        };
        for free in [ended, dialog] {
            assert!(free_after(&free));
        }
        // One that did not end, and the check did nothing about, holds it.
        assert!(!free_after(&turn));
    }

    #[test]
    fn a_tag_is_six_hex_digits_and_a_new_one_each_time() {
        let (first, second) = (new_tag(), new_tag());
        assert_eq!(first.len(), 6);
        assert!(first.chars().all(|digit| digit.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
