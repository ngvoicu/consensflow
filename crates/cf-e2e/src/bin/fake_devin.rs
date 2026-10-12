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
//! The terminal and the signals are the other stand-in's own (`fake_agent/`).

// The parts are in `fake_devin/` and `fake_agent/`: cargo looks for the modules
// of a binary's root file beside it, where each would be a binary of its own.
#[path = "fake_devin/args.rs"]
mod args;
#[path = "fake_devin/escapes.rs"]
mod escapes;
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

use cf_e2e::process::own_var;

use args::{Asked, Launch};
use escapes::Escapes;
use stop::Stop;
use store::Store;
use terminal::{Event, Keys};
use turn::Window;
use wire::Wire;

/// What Devin says to `--version`: a version the daemon takes for one that records
/// complete worker replies.
const VERSION: &str = "devin 3000.11.0 (stand-in)";

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
    let file = store::location(
        cfg!(windows),
        &home,
        own_var("APPDATA").as_deref(),
        own_var("XDG_DATA_HOME").as_deref(),
    );
    let session = launch
        .resume
        .clone()
        .unwrap_or_else(|| format!("fake-devin-{}", std::process::id()));
    let store = Store::open(&file, &session)?;
    let mut wire = Wire::open(Path::new(&wire_log), &session).map_err(said)?;
    wire.announce().map_err(said)?;
    signals::leave_on(signals, || leave(0));
    say(&format!("fake devin started on {session}\n")).map_err(said)?;

    let stop = Arc::new(Stop::default());
    let (turns, queued) = mpsc::channel::<String>();
    {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut window = Window::new(store, wire, stop, io::stdout());
            // One turn at a time, in the order the messages came.
            for text in queued {
                if let Err(failed) = window.take(&text) {
                    let _ = writeln!(io::stderr(), "fake-devin: {failed}");
                    leave(1);
                }
            }
        });
    }
    if let Some(file) = &launch.prompt_file {
        let first = std::fs::read_to_string(file).map_err(|failed| format!("{file}: {failed}"))?;
        let _ = turns.send(first);
    }

    // Raw input, like a real TUI: a bracketed paste is text, Enter outside one
    // submits, and an Escape that nothing follows is the key.
    let mut keys = Keys::start();
    let mut escapes = Escapes::default();
    loop {
        // The terminal is gone.
        let Some(events) = keys.read() else { leave(0) };
        for event in events {
            match event {
                Event::Submit(text) => {
                    let _ = turns.send(text);
                }
                // A real TUI draws what was pasted, and a paste's Enter waits for that.
                Event::Pasted(characters) => {
                    let _ = say(&format!("[pasted, {characters} characters]\n"));
                }
                Event::Escape => {
                    if escapes.press(Instant::now()) {
                        stop.press();
                    }
                }
                Event::Interrupt => leave(0),
            }
        }
    }
}
