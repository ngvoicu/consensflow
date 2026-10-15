//! One case, step by step: a member's Devin window opened as the daemon opens one,
//! cut as the app's quit cuts it (`pane.kill`, which the pane host does to each window
//! as the app quits), opened again on its conversation as the daemon opens it after
//! the quit (`--resume`, the launch the adapter prepares, with the note that its turn
//! was cut where the case gives it one), and then asked to write, edit and run `cf`.
//! What each step saw is said as it came; what Devin does is the result, and the case
//! holds as long as it could be run.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::devin_store;
use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::members::{squash, Account};
use cf_e2e::live::window::{Member, Open, Window, READY};
use cf_e2e::live::{keep_requested, secs, until, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{files, Result};
use serde_json::Value;

use crate::asks::{deliver, holds, put, Ask, Place, TASK};
use crate::case::{Case, NOTE};
use crate::outcome::{sightings_said, summary, turn_line, Outcome};
use crate::scene::{reading, tail, Scene};
use crate::words::{config_summary, cut_note, marker_of, ROLE};
use crate::{FOLDER, LONG, SETTLE, WATCH, WORK};

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
}

/// What a case came to: what it saw, and the asks it made.
pub struct Done {
    pub account: Account,
    pub outcomes: Vec<Outcome>,
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
            Some(session) => open.resume(session).open(),
            None => open.open(),
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
    fn scene<'w>(&'w self, window: &'w Window<'w>) -> Scene<'w> {
        Scene::new(self.rig, window, self.store(), wire_of(window.argv()))
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

/// Whether a process of the machine has the long command's mark in its command line.
fn command_running() -> bool {
    LONG.named().is_ok_and(|found| !found.is_empty())
}

/// Ends the long command if the window started it again, and counts it.
fn end_long(restarts: &mut u32) {
    if command_running() {
        LONG.end();
        *restarts += 1;
    }
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
    let mut done = Done {
        account: Account::new(),
        outcomes: Vec::new(),
    };
    let folder = format!("{FOLDER}-{}", case.name());
    let home = run.rig.var("HOME").unwrap_or_default();
    match live_folder_in(Path::new(home), &folder) {
        Ok(workspace) => {
            let outcome = drive(run, case, &folder, &workspace, &mut done);
            // What a killed window leaves running is ended for good, whatever came of the case.
            LONG.end();
            if let Err(failed) = outcome {
                done.account.fail(failed);
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
    let first = Member::new("worker", ROLE).first_message(&case.first_message(&tag));
    let cut = run.window(folder, folder, first, None)?;
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

    // The daemon that starts again: the window on the conversation, as it opens it.
    let again = run.window(
        folder,
        &format!("{folder}-again"),
        case.member_again(),
        Some(&session),
    )?;
    run.log
        .say(format!("{}: its window is open again", case.name()));
    done.account
        .say(format!("opened again: {}", launch_line(&again)));
    let mut scene = run.scene(&again);
    let mut restarts = 0;
    come_back(case, &mut scene, &mut restarts, &mut done.account)?;
    if !at_rest(&mut scene, &mut restarts, &mut done.account) {
        return Ok(());
    }
    ask_all(run, case, &mut scene, &place, done)
}

/// The window's first turn: a file written with its shell, and then, where it is cut
/// inside a command, the long one running. Whether it could be had.
fn first_turn(case: Case, scene: &mut Scene, place: &Place, said: &mut Account) -> Result<bool> {
    let window = scene.window;
    if case.in_command() {
        let started = Instant::now();
        let running = until(WORK, Duration::from_millis(500), || {
            scene.look();
            let _ = window.record();
            (command_running() || window.closed()).then_some(())
        });
        if running.is_none() || window.closed() {
            said.fail(format!(
                "the long command was never run in {} s: {}",
                secs(started.elapsed()),
                window.tail()
            ));
            return Ok(false);
        }
    } else {
        let turn = scene.wait_turn(&marker_of(1), WORK, |_, _, _| Ok(()))?;
        if !turn.ended {
            said.fail(turn_line("its first turn did not end", &turn));
            return Ok(false);
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
        said.fail(format!("no conversation was recorded: {}", window.tail()));
        return None;
    };
    let before = scene.watch.sightings.len();
    window.kill();
    until(Duration::from_secs(30), Duration::from_millis(100), || {
        window.closed().then_some(())
    });
    LONG.end();
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

/// The window opened again: what it does by itself (it is watched for a while), or
/// with the note that its turn was cut, which is its first message in its prompt file
/// or is pasted into it, as the case has it. What came of it is said.
fn come_back(case: Case, scene: &mut Scene, restarts: &mut u32, said: &mut Account) -> Result<()> {
    let window = scene.window;
    let before = window.recorded().len();
    if case == Case::AfterNote {
        let turn = scene.wait_turn(&marker_of(NOTE), SETTLE, |_, _, _| {
            end_long(restarts);
            Ok(())
        })?;
        said.say(turn_line(
            "its first message was the note, in its prompt file",
            &turn,
        ));
    } else {
        let opened = Instant::now();
        let drew = window.still(READY);
        said.say(format!(
            "the window {} {} s after it opened",
            if drew {
                "drew its screen"
            } else {
                "did not hold still"
            },
            secs(opened.elapsed())
        ));
        let watched = Instant::now();
        while watched.elapsed() < WATCH {
            sleep(Duration::from_millis(500));
            scene.look();
            end_long(restarts);
        }
        said.say(format!(
            "watched {} s: its screen: {}",
            WATCH.as_secs(),
            window.tail()
        ));
    }
    let record = window.record();
    said.say(format!(
        "its record: {} → {} items; {}",
        before,
        record.as_ref().map_or(0, |record| record.items.len()),
        reading(record.as_ref())
    ));
    if case == Case::AfterNotePasted {
        let text = window.given(&cut_note(NOTE, TASK))?;
        let turn = deliver(scene, NOTE, &text, SETTLE, |_| {
            end_long(restarts);
            Ok(())
        })?;
        said.say(turn_line("the note, pasted", &turn));
        said.say(format!("what we did to it: {}", scene.trail.since(0)));
    }
    if *restarts > 0 {
        said.say(format!(
            "it ran the long command again {restarts} time(s), and each was ended"
        ));
    }
    said.say(format!(
        "coming back: {}",
        sightings_said(&scene.watch.sightings)
    ));
    Ok(())
}

/// Waits for the window to come to rest after it came back: it has printed nothing
/// for a while, and the long command is not running (it is ended where the window
/// started it again). Whether it did; if it did not, the case made no ask, and says why.
fn at_rest(scene: &mut Scene, restarts: &mut u32, said: &mut Account) -> bool {
    let window = scene.window;
    let rested = until(SETTLE, Duration::from_millis(500), || {
        scene.look();
        end_long(restarts);
        (scene.quiet() || window.closed()).then_some(())
    });
    if rested.is_none() || window.closed() {
        said.say(format!(
            "no ask was made: the window {}: {}",
            if window.closed() {
                "ended"
            } else {
                "never came to rest"
            },
            window.tail()
        ));
        return false;
    }
    true
}

/// The asks, one at a time while the window comes to rest after each: what each
/// came to is said, and a window that is held on one is asked no more.
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
        let number = FIRST_ASK + u32::try_from(at).unwrap_or_default();
        run.log
            .say(format!("{}: asking it for {}", case.name(), ask.name()));
        let outcome = put(scene, place, ask, number)?;
        for line in outcome.lines() {
            done.account.say(line);
        }
        held = !outcome.turn.ended;
        done.outcomes.push(outcome);
    }
    done.account
        .say(format!("its screen at the end: {}", scene.window.tail()));
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
    fn a_tag_is_six_hex_digits_and_a_new_one_each_time() {
        let (first, second) = (new_tag(), new_tag());
        assert_eq!(first.len(), 6);
        assert!(first.chars().all(|digit| digit.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
