//! How a window opened again comes back: what it does by itself on its conversation,
//! what the case does to it (the keys that interrupt a turn, the command that switches
//! it to full permission, the note that its turn was cut, the daemon's readiness) and
//! whether it is then at rest, to be asked. What each step did to it is said as it is
//! taken, with what its terminal, its wire and its screen said of the mode it is in,
//! and for a command that switches it, what that did to its record and its wire: no
//! turn, no item, nothing the daemon's reader takes for an answer.

use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::devin_store::Message;
use cf_e2e::live::ask;
use cf_e2e::live::ask::Record;
use cf_e2e::live::members::Account;
use cf_e2e::live::window::READY;
use cf_e2e::live::{secs, until};
use cf_e2e::Result;

use crate::again::Again;
use crate::asks::deliver;
use crate::case::{Case, Opening, Step, BYPASS, NOTE};
use crate::drive::{command_running, wire_of, Run};
use crate::outcome::{sightings_said, turn_line};
use crate::scene::{Dialog, Scene, LOOK};
use crate::turn::{read_header_first, reading, stored_fault, stored_shape, tail};
use crate::words::marker_of;
use crate::{LONG, SETTLE, WATCH};

/// How long the window is looked at after the keys that interrupt a turn: the third of
/// them is a second after the others, and what they open is closed by it.
const AFTER_INTERRUPT: Duration = Duration::from_secs(4);

/// How long the wire is waited for to say the mode after the command that switches it,
/// and how long the window is looked at after.
const SWITCH: Duration = Duration::from_secs(20);
const AFTER_SWITCH: Duration = Duration::from_secs(3);

/// How long the daemon's readiness is driven: as long as the product gives a window to
/// leave the normal mode (30 s), and a little more, so that its own end shows.
const READINESS: Duration = Duration::from_secs(35);

/// How long Devin's store is read again for the first message, while its writer has it.
const STORED: Duration = Duration::from_secs(5);

/// How long a dialog is given to go once the app's interrupt was pressed into it.
const RELEASE: Duration = Duration::from_secs(20);

/// How long a window that was brought back is given to come to rest before the asks: a
/// whole run of the long command it started again, and a minute more for the rest of its
/// turn. What held it otherwise was waited for by the steps that brought it back.
const REST: Duration = Duration::from_secs(LONG.seconds() + 60);

/// What came of the wait for the window to be at rest.
enum Rest {
    Rested,
    /// A permission dialog waits for an answer: its terminal says so, or its screen
    /// shows the options.
    Dialog(String),
    Never,
    Closed,
}

/// How long the window is to have been at rest, with nothing of the long command running,
/// before an ask goes in: a window whose run of the long command was ended looked at rest
/// for a moment and ran it again, and an ask pasted then went into that turn and was
/// cancelled with it (zeewin, 2026-10-16).
const STILL: Duration = Duration::from_secs(5);

/// Whether an ask may go in, where the window runs the long command (`running`) and has
/// been at rest with nothing of the command running for `still`: a run of the command is
/// waited for to end by itself, and the ask goes in once the window has been at rest for
/// [`STILL`] after. Nothing of the command is ended and the window is not interrupted for
/// it: an interrupt cancelled the turn of the ask the window was given (2026-10-16).
pub fn may_ask(running: bool, still: Duration) -> bool {
    !running && still >= STILL
}

/// Whether the window comes to rest within [`REST`], to be asked, and how many times it
/// ran the long command again meanwhile, each let run to its end ([`may_ask`]).
pub fn settled(scene: &mut Scene) -> (bool, u32) {
    let mut again = Again::default();
    let rested = matches!(rest(scene, &mut again), Rest::Rested);
    (rested, again.runs())
}

/// Brings the window back as the case has it, and says what came of each step. Whether
/// it is then at rest, to be asked: where it is not, the case makes no ask, and says why.
pub fn come_back(run: &Run, case: Case, scene: &mut Scene, said: &mut Account) -> Result<bool> {
    let mut again = Again::default();
    opening(case, scene, &mut again, said)?;
    for step in case.then {
        match step {
            Step::Watch => watch(scene, &mut again, said),
            Step::Interrupt => interrupt(run, scene, said),
            Step::Bypass => bypass(scene, said)?,
            Step::Note => note(scene, &mut again, said)?,
            Step::Ready => ready(run, scene, said)?,
        }
    }
    if let Some(ran) = again.said() {
        said.say(ran);
    }
    said.say(format!(
        "coming back: {}",
        sightings_said(&scene.watch.sightings)
    ));
    said.say(format!(
        "{}; the wire said of its config and mode: {}",
        scene.mode_line(),
        scene.watch.modes.since(0)
    ));
    said.say(format!("its terminal said: {}", scene.watch.marks.since(0)));
    let mut before = Again::default();
    let rested = rest(scene, &mut before);
    if let Some(ran) = before.said() {
        said.say(format!("before the asks {ran}"));
    }
    match rested {
        Rest::Rested => Ok(true),
        Rest::Dialog(dialog) => {
            said.say(format!(
                "no ask was made: the window is {dialog}: {}",
                scene.rows()
            ));
            Ok(false)
        }
        Rest::Never => {
            said.say(format!(
                "no ask was made: the window never came to rest: {}",
                scene.rows()
            ));
            Ok(false)
        }
        Rest::Closed => {
            said.say(format!(
                "no ask was made: the window ended: {}",
                scene.window.tail()
            ));
            Ok(false)
        }
    }
}

/// What the window begins with: it opens on its conversation and is waited for to draw,
/// or its first message is the note, in its prompt file, whose turn is waited for.
fn opening(case: Case, scene: &mut Scene, again: &mut Again, said: &mut Account) -> Result<()> {
    let window = scene.window;
    let before = window.recorded().len();
    if case.opening == Opening::Nothing {
        let opened = Instant::now();
        let drew = window.still(scene.life.cap(READY));
        scene.look();
        said.say(format!(
            "the window {} {} s after it opened",
            if drew {
                "drew its screen"
            } else {
                "did not hold still"
            },
            secs(opened.elapsed())
        ));
    } else {
        let turn = scene.wait_turn(&marker_of(NOTE), SETTLE, |scene, _, _| again.look(scene))?;
        said.say(turn_line(
            "its first message was the note, in its prompt file",
            &turn,
        ));
        // How Devin stores a prompt it ran under `/bypass`, which the daemon puts a
        // resumed window's first message behind. The product's own must be stored with
        // its header first: the daemon's reader takes off a kept `/bypass` only while
        // the launch folder lasts (the limit beside `behind_bypass`). Behind the case's
        // own `/bypass` the window was given what ConsensFlow never gives one, and how
        // Devin stored it is only said. The store is read again while its writer has it.
        let header = marker_of(NOTE);
        let messages = until(scene.life.cap(STORED), LOOK, || {
            let messages = scene.watch.messages(window);
            stored_shape(&messages, &header).map(|_| messages)
        })
        .unwrap_or_default();
        said.say(format!(
            "Devin's store has the note {}",
            stored_shape(&messages, &header)
                .as_deref()
                .unwrap_or("nowhere")
        ));
        if case.opening == Opening::Note {
            let within = scene.life.cap(STORED);
            for fault in note_faults(turn.arrived, &messages, &header, within, LOOK, || {
                window.record()
            }) {
                said.fail(fault);
            }
        }
    }
    let record = window.record();
    said.say(format!(
        "its record: {} → {} items; {}",
        before,
        record.as_ref().map_or(0, |record| record.items.len()),
        reading(record.as_ref())
    ));
    said.say(format!(
        "once it was open: {}; the wire said of its config and mode: {}",
        scene.mode_line(),
        scene.watch.modes.since(0)
    ));
    Ok(())
}

/// What fails a case whose first message is the product's own (the note, in its prompt
/// file): how Devin stored it among `messages`, whether or not the wait for its turn saw
/// it arrive (`arrived`), and, where it arrived or Devin stored it, how the daemon's
/// reader has it, by `read`, read every `step` within `within` while it reads no record.
/// The wait's last read of the record, and the reader's, meet the lock Devin's writer
/// holds on its store at times, and read as none: a lock at the wait's end skipped the
/// store's check, and a note kept behind `/bypass` passed (both reviewers of
/// 2026-10-18); and a lock on the one read of the reader failed a case Devin stored
/// right (Astra, the same day).
pub fn note_faults(
    arrived: bool,
    messages: &[Message],
    header: &str,
    within: Duration,
    step: Duration,
    mut read: impl FnMut() -> Option<Record>,
) -> Vec<String> {
    let mut faults: Vec<String> = stored_fault(messages, header).into_iter().collect();
    if arrived || stored_shape(messages, header).is_some() {
        // Read again until a record holds the note: one a beat behind the store has it
        // nowhere yet (the review of c2e29f2d).
        let mut last = None;
        let holding = until(within, step, || {
            let record = read()?;
            let holds = record
                .items
                .iter()
                .any(|item| item.role == "user" && item.text.contains(header));
            if holds {
                return Some(record);
            }
            last = Some(record);
            None
        });
        match (holding, last) {
            (Some(record), _) if read_header_first(Some(&record), header) => {}
            (Some(record), _) => faults.push(format!(
                "the daemon's reader has the note behind other words: {}",
                tail(Some(&record), 4)
            )),
            (None, Some(record)) => faults.push(format!(
                "the daemon's reader has the note nowhere in {} s: {}",
                secs(within),
                tail(Some(&record), 4)
            )),
            (None, None) => faults.push(format!(
                "the daemon's reader read no record of the note in {} s",
                secs(within)
            )),
        }
    }
    faults
}

/// The window is left alone for [`WATCH`], to be seen doing what it does by itself: a run
/// of the long command it starts again is seen ([`Again`]), and runs to its end.
fn watch(scene: &mut Scene, again: &mut Again, said: &mut Account) {
    let marks = scene.watch.marks.len();
    let watched = Instant::now();
    while watched.elapsed() < scene.life.cap(WATCH) {
        sleep(LOOK);
        scene.look();
        let _ = again.look(scene);
    }
    said.say(format!(
        "watched {} s: its terminal said: {}; its screen: {}",
        WATCH.as_secs(),
        scene.watch.marks.since(marks),
        scene.rows()
    ));
}

/// Looks at the window for `how long`.
fn look_for(scene: &mut Scene, how_long: Duration) {
    let started = Instant::now();
    while started.elapsed() < scene.life.cap(how_long) {
        sleep(LOOK);
        scene.look();
    }
}

/// The keys that interrupt a turn are pressed as the app presses them, and the window is
/// looked at after.
fn interrupt(run: &Run, scene: &mut Scene, said: &mut Account) {
    let (marks, modes) = (scene.watch.marks.len(), scene.watch.modes.len());
    if let Err(failed) = scene.interrupt(&run.interrupt) {
        said.fail(format!(
            "the keys that interrupt a turn were not pressed: {failed}"
        ));
        return;
    }
    look_for(scene, AFTER_INTERRUPT);
    said.say(format!(
        "after the interrupt ({}): its terminal said: {}; the wire said: {}; {}; its screen: {}",
        scene.trail.since(scene.trail.len().saturating_sub(1)),
        scene.watch.marks.since(marks),
        scene.watch.modes.since(modes),
        scene.mode_line(),
        scene.rows()
    ));
}

/// What the window's record and its wire hold now: the items of the record, and the
/// prompts its TUI put to its agent.
fn held(scene: &Scene) -> (usize, usize) {
    let items = scene.window.record().map_or(0, |record| record.items.len());
    (items, scene.watch.prompts())
}

/// What a command that switches the window did to its record and its wire, from what
/// they held before it (`before`, [`held`]): a turn would be a prompt, and an item of
/// its own in the record, which the daemon's reader would read as an answer.
fn left(scene: &Scene, before: (usize, usize)) -> String {
    let (items, prompts) = held(scene);
    format!(
        "its record: {} → {items} items; the wire: {} prompt(s) to its agent since; {}",
        before.0,
        prompts.saturating_sub(before.1),
        reading(scene.window.record().as_ref())
    )
}

/// The command that switches the window to full permission is pasted, and the wire is
/// waited for to say the mode.
fn bypass(scene: &mut Scene, said: &mut Account) -> Result<()> {
    let (marks, modes) = (scene.watch.marks.len(), scene.watch.modes.len());
    let before = held(scene);
    let text = scene.window.given(BYPASS)?;
    scene.paste(&text, format!("paste {BYPASS}"))?;
    let switched = until(scene.life.cap(SWITCH), LOOK, || {
        scene.look();
        (scene.watch.mode.as_deref() == Some("bypass")).then_some(())
    });
    look_for(scene, AFTER_SWITCH);
    said.say(format!(
        "{BYPASS} was pasted: the wire {} bypass; its terminal said: {}; the wire said: {}; {}; {}; its screen: {}",
        if switched.is_some() {
            "says"
        } else {
            "did NOT say"
        },
        scene.watch.marks.since(marks),
        scene.watch.modes.since(modes),
        left(scene, before),
        scene.mode_line(),
        scene.rows()
    ));
    Ok(())
}

/// The daemon's readiness, as it is before the daemon gives the window anything: the
/// product's own (`ask::devin_readiness`), which reads the window's wire and, where it
/// says the normal mode, gives the window `/bypass` itself and waits for the wire to say
/// it changed. The case gives the window nothing: what it was given is the product's.
fn ready(run: &Run, scene: &mut Scene, said: &mut Account) -> Result<()> {
    let Some(wire) = wire_of(scene.window.argv()) else {
        said.fail("the daemon's readiness: the launch names no wire log");
        return Ok(());
    };
    scene.look();
    let mode = ask::devin_ready(&wire)?
        .mode
        .unwrap_or_else(|| "no mode".to_owned());
    let (marks, before) = (scene.watch.marks.len(), held(scene));
    let started = Instant::now();
    let readiness = ask::devin_readiness(run.rig, scene.window, &wire, scene.life.cap(READINESS))?;
    let took = secs(started.elapsed());
    scene.trail.note(format!(
        "the daemon's readiness gave it: {}",
        if readiness.given.is_empty() {
            "nothing".to_owned()
        } else {
            readiness.given.join(", ")
        }
    ));
    look_for(scene, AFTER_SWITCH);
    let now = ask::devin_ready(&wire)?
        .mode
        .unwrap_or_else(|| "no mode".to_owned());
    let line = format!(
        "the daemon's readiness: the wire said {mode}; it gave the window {:?}; it answered {} in {took} s ({}); the wire says {now}; its terminal said: {}; {}; {}; its screen: {}",
        readiness.given,
        if readiness.ready { "ready" } else { "NOT ready" },
        readiness.answers.join(" → "),
        scene.watch.marks.since(marks),
        left(scene, before),
        scene.mode_line(),
        scene.rows()
    );
    if readiness.ready {
        said.say(line);
    } else {
        said.fail(line);
    }
    Ok(())
}

/// The note that its turn was cut is pasted, as the daemon pastes a message it gives a
/// window that is open, and its turn is waited for: the engine's own note.
fn note(scene: &mut Scene, again: &mut Again, said: &mut Account) -> Result<()> {
    let (message, task) = Case::note_of();
    let text = scene.window.given(&ask::cut_note(message, &[task])?)?;
    let keys = scene.trail.len();
    let turn = deliver(scene, NOTE, &text, SETTLE, |scene| again.look(scene))?;
    said.say(turn_line("the note, pasted", &turn));
    said.say(format!("what we did to it: {}", scene.trail.since(keys)));
    Ok(())
}

/// Waits for the window to come to rest, to be asked: it has printed nothing for a
/// while, no dialog stands on its screen, and nothing of the long command runs, all of it
/// for [`STILL`]. A run of the long command is waited for to end by itself ([`may_ask`]),
/// and seen by `again`. A window that waits for an answer to a permission for a while
/// (its terminal says so, or its screen shows the options) is not waited for longer.
fn rest(scene: &mut Scene, again: &mut Again) -> Rest {
    let within = scene.life.cap(REST);
    let started = Instant::now();
    let mut seen = Dialog::default();
    let mut still_since = None::<Instant>;
    loop {
        sleep(LOOK);
        scene.look();
        if scene.window.closed() {
            return Rest::Closed;
        }
        let running = command_running(scene).unwrap_or(false);
        again.saw(running);
        if running || !scene.at_rest() {
            still_since = None;
        } else if still_since.is_none() {
            still_since = Some(Instant::now());
        }
        let still = still_since.map_or(Duration::ZERO, |since| since.elapsed());
        if may_ask(running, still) {
            return Rest::Rested;
        }
        if let Some(dialog) = scene.held_by_dialog().or_else(|| seen.held(scene)) {
            return Rest::Dialog(dialog);
        }
        if started.elapsed() >= within {
            return Rest::Never;
        }
    }
}

/// Where a permission dialog waits, after an ask nobody answered or at the end of the
/// case (its terminal says so, or its options are on the screen), the keys that interrupt
/// a turn are pressed into it, as the app's Stop presses them: what that did to the
/// dialog is the last thing said.
pub fn release(run: &Run, scene: &mut Scene, said: &mut Account) {
    if scene.window.closed() || (scene.waiting().is_none() && !scene.dialog_shown()) {
        return;
    }
    let (marks, seen) = (scene.watch.marks.len(), scene.watch.sightings.len());
    if let Err(failed) = scene.interrupt(&run.interrupt) {
        said.say(format!(
            "the keys that interrupt a turn were not pressed: {failed}"
        ));
        return;
    }
    let left = until(scene.life.cap(RELEASE), LOOK, || {
        scene.look();
        (!scene.dialog_shown()).then_some(())
    });
    look_for(scene, AFTER_INTERRUPT);
    said.say(format!(
        "the dialog was held until the app's interrupt was pressed into it: then its screen {} the dialog's options; its terminal said: {}; seen: {}; its screen: {}",
        if left.is_some() { "no longer shows" } else { "still shows" },
        scene.watch.marks.since(marks),
        sightings_said(&scene.watch.sightings[seen..]),
        scene.rows()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ask_waits_for_a_run_of_the_long_command_to_end_by_itself_and_the_window_to_be_still() {
        // The window ran the long command again: ended alone, Devin ran it again in the same
        // turn, the window looked at rest for a moment, and the ask pasted then was
        // cancelled with that turn (zeewin, 2026-10-16); and an interrupt for it cancelled
        // the turn of the ask (after-note's shell ask). The run is waited for, however long
        // the window was still before it.
        let secs = Duration::from_secs;
        assert!(!may_ask(true, secs(60)));
        // Then the window is still, with nothing of the command running, for a while.
        assert!(!may_ask(false, Duration::ZERO));
        assert!(!may_ask(false, STILL - Duration::from_millis(1)));
        assert!(may_ask(false, STILL));
        assert_eq!(STILL, secs(5));
        // A whole run of the command, and the rest of its turn, fit in the wait for rest.
        assert_eq!(REST, secs(LONG.seconds() + 60));
    }

    const HEADER: &str = "[ConsensFlow m-2 ";
    const NOTE: &str = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nGo on where you stopped.";

    fn stored(text: &str) -> Message {
        Message {
            id: "u-1".to_owned(),
            role: "user".to_owned(),
            text: text.to_owned(),
            raw: serde_json::Value::Null,
        }
    }

    fn read(text: &str) -> Record {
        Record {
            items: vec![cf_e2e::live::ask::Item {
                id: String::new(),
                role: "user".to_owned(),
                text: text.to_owned(),
                complete: true,
            }],
            settled: true,
            settlement: "settled".to_owned(),
            failed: serde_json::Value::Null,
            quota: serde_json::Value::Null,
        }
    }

    /// Faults of the note as the case finds them, with the reader read by `read`.
    fn faults(
        arrived: bool,
        messages: &[Message],
        read: impl FnMut() -> Option<Record>,
    ) -> Vec<String> {
        note_faults(
            arrived,
            messages,
            HEADER,
            Duration::from_millis(200),
            Duration::ZERO,
            read,
        )
    }

    /// Both reviewers of 2026-10-18: the store was asked only where the wait for the
    /// note's turn saw it arrive, so a lock on the record at that wait's last read skipped
    /// it, and a note stored with /bypass kept passed.
    #[test]
    fn how_devin_stored_the_note_is_asked_whether_or_not_the_wait_for_its_turn_saw_it_arrive() {
        let kept = [stored(&format!("/bypass {NOTE}"))];
        for arrived in [true, false] {
            let found = faults(arrived, &kept, || Some(read(NOTE)));
            assert!(
                found
                    .iter()
                    .any(|fault| fault.contains("with /bypass kept")),
                "arrived {arrived}: {found:?}"
            );
        }
        // Stored with its header first, and the reader has it so: nothing fails.
        assert_eq!(
            faults(false, &[stored(NOTE)], || Some(read(NOTE))),
            Vec::<String>::new()
        );
        // Stored nowhere: that fails, as it says.
        let nowhere = faults(false, &[], || None);
        assert!(nowhere
            .iter()
            .any(|fault| fault.contains("Devin's store has the note nowhere")));
    }

    /// Astra, 2026-10-18: the reader was read once after the store, and a record its
    /// store's writer held locked read as none, which failed the case.
    #[test]
    fn the_daemons_reader_is_read_again_while_the_record_cannot_be_read_and_judged_once_it_is() {
        let locked = std::cell::Cell::new(2);
        let found = faults(true, &[stored(NOTE)], || {
            if locked.get() > 0 {
                locked.set(locked.get() - 1);
                return None;
            }
            Some(read(NOTE))
        });
        assert_eq!(found, Vec::<String>::new(), "read again past the lock");
        assert_eq!(locked.get(), 0);
        // Once it is read, it is judged: a reader that has the note behind other words fails.
        let reads = std::cell::Cell::new(0);
        let behind = faults(true, &[stored(NOTE)], || {
            reads.set(reads.get() + 1);
            Some(read(&format!("Look: {NOTE}")))
        });
        assert_eq!(reads.get(), 1, "read once it can be, and holds the note");
        // The review of c2e29f2d: a record a beat behind the store, without the note yet,
        // is read again, and judged once it holds it.
        let lagging = std::cell::Cell::new(2);
        let caught_up = faults(true, &[stored(NOTE)], || {
            if lagging.get() > 0 {
                lagging.set(lagging.get() - 1);
                return Some(read("an earlier message"));
            }
            Some(read(NOTE))
        });
        assert_eq!(caught_up, Vec::<String>::new());
        let never = faults(true, &[stored(NOTE)], || Some(read("an earlier message")));
        assert!(
            never
                .iter()
                .any(|fault| fault.contains("has the note nowhere")),
            "{never:?}"
        );
        assert!(behind
            .iter()
            .any(|fault| fault.contains("behind other words")));
        // A record never read in the time is said so.
        let never = faults(true, &[stored(NOTE)], || None);
        assert_eq!(never.len(), 1, "{never:?}");
    }
}
