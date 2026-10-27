//! A stand-in for the `devin` CLI in the integration tests. It speaks Devin's
//! launch arguments and its version, keeps Devin's store of its sessions and
//! writes Devin's wire log as the daemon reads them (the conversation a window
//! shows, a turn at work, how it ended), and reads its terminal raw, as a TUI
//! does: a bracketed paste followed by Enter is one message, and a first message
//! is the file the window was launched with. What it answers a message with is
//! told in `turn`.
//!
//! Escape stops no turn alone: Devin's own status line says "esc twice to
//! interrupt", and two Escapes close together stop the turn at work, as they stop
//! Devin's (`escapes`). A turn that is stopped is cancelled on the wire, and what
//! it said is kept in the store without the end of its turn, as Devin keeps it.
//! The stand-in has no rewind, which two Escapes at a Devin at rest open.
//!
//! A turn takes no time unless the window is told how long a model takes
//! (`FAKE_DEVIN_FIRST_WORD_MS`, `FAKE_DEVIN_WORK_MS`: [`turn::Pace`]), or its message
//! says (`THINK`, `SLEEP`: `turn`). A message
//! the window is handed while a turn is at work goes into its agent's queue, and
//! is run when the turn ends, one at a time in the order the messages came: its
//! row is written to the store then, and not before, as Devin's are (the user
//! rows between the replies of `tests/engine/fixtures/completion/devin/
//! native-tui.json`, each named on the wire by the step that runs it; a live run
//! of 2026-10-13, Devin 3000.11.3: a note pasted a second after another showed
//! in the record 63 s later, when the turn before it ended). Its history row is
//! written at the Enter, before then (`store::submitted`), as Devin 3000.11.92 writes
//! it. A turn's prompt is on the wire as a request of the TUI when the message is run,
//! which no real Devin writes (T-102): the daemon reads none.
//!
//! A turn can leave a command running (`BACKGROUND <seconds> <words>`: `command`):
//! its agent runs it as a tool once it has thought, answers, and the turn ends; the
//! command goes on saying what it prints on the wire for as long as it runs, as a
//! tool does whose turn has ended. And a turn can end the window's program under it
//! (`EXIT <code> <words>`: `crash`), as a harness that crashes does.
//!
//! Where `FAKE_DEVIN_STAMPS` is set, its wire log is written as Devin 3000.11 writes one
//! (`wire`): its work stamped with its time, its turns begun and ended by its word on them,
//! no request of its TUI's, and a window opened again replaying its last turn.
//!
//! A window can do wrong once, as Devin has (`faults`): drop a paste, as a TUI that
//! never took it (`FAKE_DEVIN_DROP`), or end as its agent takes a message, once the
//! message is recorded (`FAKE_DEVIN_CRASH`).
//!
//! `/new` opens a new conversation: the window shows it at once (its wire says so, and
//! its store has the session), and the turns after the ones queued go into it. The turn
//! at work and the messages queued behind it run on in the conversation left, so a case
//! can stop the daemon with a paste on its way there; Devin's own `/new` waits for the
//! turn to end before it shows the new one (T-107, probed once), which no case can time.
//!
//! It is in the permission mode Devin is in (`permission`): full permission where it
//! was opened with `--permission-mode dangerous`, but a session resumed after a quit
//! cut its first turn (its store has the turn's message and no end of any turn), which
//! comes back in the normal mode whatever the flag says, as Devin 3000.11.92 does. Its
//! wire says the mode in the configuration it opens with, and each change of it. In the
//! normal mode a call of the prompt file's turn is refused ("User skipped this tool
//! call") and a pasted turn's opens a dialog, which Enter answers (its first option,
//! approve once) and Escape leaves; `/bypass` switches it. Its store keeps the prompt a
//! message ran under `/bypass`, or the message as it was given where
//! `FAKE_DEVIN_STORES_COMMANDS` says so: how Devin stores one is not known.
//!
//! The terminal and the signals are the other stand-in's own (`fake_agent/`).

// The parts are in `fake_devin/` and `fake_agent/`: cargo looks for the modules
// of a binary's root file beside it, where each would be a binary of its own.
#[path = "fake_devin/args.rs"]
mod args;
#[path = "fake_devin/calls.rs"]
mod calls;
#[path = "fake_devin/command.rs"]
mod command;
#[path = "fake_devin/crash.rs"]
mod crash;
#[path = "fake_devin/escapes.rs"]
mod escapes;
#[path = "fake_devin/faults.rs"]
mod faults;
#[path = "fake_devin/permission.rs"]
mod permission;
#[path = "fake_agent/signals.rs"]
mod signals;
#[path = "fake_agent/stop.rs"]
mod stop;
#[path = "fake_devin/store.rs"]
mod store;
#[path = "fake_agent/terminal.rs"]
mod terminal;
#[path = "fake_devin/turn.rs"]
mod turn;
#[path = "fake_devin/wire.rs"]
mod wire;

use std::convert::Infallible;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use cf_e2e::devin_store;
use cf_e2e::process::own_var;

use args::{Asked, Launch};
use escapes::Escapes;
use faults::Fault;
use permission::{Answer, Permission};
use stop::Stop;
use store::Store;
use terminal::{Event, Keys};
use turn::{Pace, Window};
use wire::Wire;

/// The command that opens a new conversation.
const NEW: &str = "/new";

/// What the turns' thread is given, in the order it came.
enum Given {
    /// A message, and whether it is the first of the launch, in its prompt file.
    Message(String, bool),
    /// The conversation `/new` opened, which the turns after it go into.
    New(String),
}

/// What makes the window write its wire log as Devin 3000.11 does (`Wire::stamped`): its
/// work stamped, its turns begun and ended by its word on them, a window opened again
/// replaying its last turn. Nothing in a person's environment sets it.
const STAMPS_VARIABLE: &str = "FAKE_DEVIN_STAMPS";

/// What Devin says to `--version`: the Devin whose store and TUI it plays, which writes a
/// prompt's history row at its Enter (`store::submitted`).
const VERSION: &str = "devin 3000.11.92 (stand-in)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Signals are waited for on a thread of their own, so they are blocked in
    // this one and in every thread started after it.
    let signals = signals::block();
    let own = std::env::current_exe().ok();
    match args::parse(&args, own.as_deref()) {
        Asked::Version => say(&format!("{VERSION}\n"))
            .map_or_else(|failed| fail(&failed.to_string()), |()| ExitCode::SUCCESS),
        Asked::Window(launch) => match run(&launch, signals) {
            Ok(never) => match never {},
            Err(failed) => fail(&failed),
        },
    }
}

/// Says why the window could not run, on its error output.
fn fail(failed: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "fake-devin: {failed}");
    ExitCode::FAILURE
}

/// Writes `text` to the window's screen.
fn say(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Ends the window with `code`.
fn leave(code: i32) -> ! {
    std::process::exit(code)
}

/// A failure of the window, in words.
fn said(failed: impl std::fmt::Display) -> String {
    failed.to_string()
}

/// The window, from its start to its end, which it does not return from.
fn run(launch: &Launch, signals: signals::Signals) -> Result<Infallible, String> {
    let wire_log =
        own_var("CHISEL_PURE_ACP_WIRE_LOG").ok_or("CHISEL_PURE_ACP_WIRE_LOG is not set")?;
    let home = own_var("HOME")
        .or_else(|| own_var("USERPROFILE"))
        .ok_or("the window has no home")?;
    let file = devin_store::location(
        cfg!(windows),
        &home,
        own_var("APPDATA").as_deref(),
        own_var("XDG_DATA_HOME").as_deref(),
    );
    let mut session = launch
        .resume
        .clone()
        .unwrap_or_else(|| format!("fake-devin-{}", std::process::id()));
    let store = Store::open(&file, &session)?;
    // Full permission where it was asked for, but on a session whose first turn a quit
    // cut: Devin brings that one back in the normal mode, whatever the flag says.
    let asked_full = launch.permission_mode.as_deref() == Some("dangerous");
    let cut_in_its_first_turn = launch.resume.is_some() && !store.finished_a_turn();
    let permission = Arc::new(Permission::new(asked_full && !cut_in_its_first_turn));
    let stamped = own_var(STAMPS_VARIABLE).is_some_and(|stamps| !stamps.is_empty());
    let mut wire = Wire::open(Path::new(&wire_log), &session)
        .map_err(said)?
        .stamped(stamped);
    let mut opened = 0;
    wire.announce(permission.mode()).map_err(said)?;
    if launch.resume.is_some() {
        let (user, answer) = store.last_turn()?;
        wire.replayed(user.as_deref(), answer.as_deref())
            .map_err(said)?;
    }
    signals::leave_on(signals, || leave(0));
    say(&format!("fake devin started on {session}\n")).map_err(said)?;

    let stop = Arc::new(Stop::default());
    let pace = Pace::of(
        own_var(turn::FIRST_WORD_VARIABLE).as_deref(),
        own_var(turn::WORK_VARIABLE).as_deref(),
    );
    let (turns, queued) = mpsc::channel::<Given>();
    // The jobs of a window are in `jobs/` beside the daemon's home, where the rig looks.
    let jobs = own_var("CONSENSFLOW_HOME")
        .and_then(|home| Path::new(&home).parent().map(|folder| folder.join("jobs")));
    // Only a window opened afresh crashes: the one resumed after it goes on.
    let crash = Fault::of(
        own_var(faults::CRASH_VARIABLE)
            .as_deref()
            .filter(|_| launch.resume.is_none()),
    );
    let folder = std::env::current_dir().map_err(said)?;
    let stores_commands =
        own_var(turn::STORES_COMMANDS_VARIABLE).is_some_and(|stores| !stores.is_empty());
    {
        let (stop, permission) = (Arc::clone(&stop), Arc::clone(&permission));
        let file = file.clone();
        thread::spawn(move || {
            let mut window = Window::new(store, wire, stop, io::stdout(), pace)
                .in_mode(permission, folder)
                .crashing_on(crash)
                .storing_commands(stores_commands);
            if let Some(folder) = jobs {
                window = window.leaving_jobs_in(folder, std::process::id(), command::THINKING);
            }
            // One turn at a time, in the order the messages came.
            for given in queued {
                let done = match given {
                    Given::Message(text, prompt_file) => window.take_message(&text, prompt_file),
                    Given::New(session) => {
                        Store::open(&file, &session).map(|store| window.switch(store, &session))
                    }
                };
                if let Err(failed) = done {
                    let _ = writeln!(io::stderr(), "fake-devin: {failed}");
                    leave(1);
                }
            }
        });
    }
    if let Some(file) = &launch.prompt_file {
        let first = std::fs::read_to_string(file).map_err(|failed| format!("{file}: {failed}"))?;
        let _ = turns.send(Given::Message(first, true));
    }

    // Raw input, like a real TUI: a bracketed paste is text, Enter outside one
    // submits, and an Escape that nothing follows is the key.
    let mut keys = Keys::start();
    let mut escapes = Escapes::default();
    let mut dropping = Fault::of(own_var(faults::DROP_VARIABLE).as_deref());
    loop {
        // The terminal is gone.
        let Some(events) = keys.read() else { leave(0) };
        for event in events {
            match event {
                Event::Submit(text) if dropping.meets(&text) => {}
                // Enter at a dialog takes its first option; anywhere else it submits.
                Event::Submit(text) => {
                    if permission.answer(Answer::Approved) {
                    } else if text.trim() == NEW {
                        opened += 1;
                        session = format!("fake-devin-{}-new-{opened}", std::process::id());
                        Store::open(&file, &session)?;
                        Wire::open(Path::new(&wire_log), &session)
                            .and_then(|mut shown| shown.announce(permission.mode()))
                            .map_err(said)?;
                        let _ = turns.send(Given::New(session.clone()));
                    } else {
                        if let Err(failed) = store::submitted(Path::new(&file), &session, &text) {
                            let _ = writeln!(io::stderr(), "fake-devin: {failed}");
                        }
                        let _ = turns.send(Given::Message(text, false));
                    }
                }
                // A real TUI draws what was pasted, and a paste's Enter waits for that.
                Event::Pasted(characters) => {
                    let _ = say(&format!("[pasted, {characters} characters]\n"));
                }
                // Escape at a dialog leaves it; anywhere else, two stop a turn.
                Event::Escape => {
                    if !permission.answer(Answer::Cancelled) && escapes.press(Instant::now()) {
                        stop.press();
                    }
                }
                Event::Interrupt => leave(0),
            }
        }
    }
}
