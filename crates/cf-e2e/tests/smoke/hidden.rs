//! The app reads its windows as fast with its own window hidden as shown. The pane host
//! stops reading a window whose output the page has not acknowledged past a window of it
//! (1 MiB), and its agent then waits on its terminal; a page WebKit throttled once the
//! app was hidden drew one message a turn, a turn a second or so, and an agent that
//! printed faster waited on the page (T-454: 8 MiB took 266 s hidden, 6 s shown).
//!
//! The app is hidden as a person hides it (its `NSRunningApplication`, by its process id),
//! and only once its page has shown the window; a while later the window prints a flood,
//! which it counts in a file of its own as it goes. A screen that is locked is refused in
//! words before the case starts (`cargo xtask smoke`, tools/xtask/src/screen.rs): every
//! page sleeps behind it, hidden or not, and the case would measure the lock.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::files;
use cf_e2e::process::{own_var, Run};

use crate::app::App;
use crate::machine::Machine;
use crate::{bundle, patience_of, Outcome, PASTE_READER};

/// The stand-in harness of this case.
const HARNESS: &str = include_str!("../../fixtures/smoke-hidden-harness.sh");

/// The flood, in lines of 131 bytes on the terminal: 8 MiB, eight windows of unacknowledged output.
const FLOOD_LINES: usize = 64_000;

/// How long the app is hidden before its window prints: WebKit throttles a page it no
/// longer shows within seconds, and puts it to sleep behind a locked screen about 30 s in.
const HIDDEN_FOR: Duration = Duration::from_secs(40);

/// How long the app is left to come up after its page drew the window, before it is hidden.
const SETTLE: Duration = Duration::from_secs(10);

/// How long the hidden app is given to read the flood. Measured on this case (40 s hidden):
/// 1.3 s with the page writing each message as it comes (twice); with each message waiting
/// on the acknowledgement of the one before, 15.9 s and 62,000 of the lines in 60 s, and
/// even shown that page took 6 to 7 s for half the flood, so it never reads it in under
/// about 14 s. Hidden for minutes on a loaded Mac the first took 5 to 50 s, the second 15
/// to over 300 s (T-454); this case hides the app for less than a minute, where the
/// first's time is the page's own.
const DRAIN: Duration = Duration::from_secs(10);

#[test]
#[ignore = "opt-in, on a built app: cargo xtask smoke (npm run smoke)"]
fn the_built_app_reads_its_windows_while_it_is_hidden() -> Outcome {
    let bundle = bundle::locate(&bundle::requested(
        own_var("CONSENSFLOW_SMOKE_APP").as_deref(),
    ))?;
    let patience = patience_of(own_var("CONSENSFLOW_SMOKE_TIMEOUT_MS").as_deref())?;
    let machine = Machine::with_harness(Path::new(PASTE_READER), patience, HARNESS)?;
    let (go, count) = (machine.root.join("go"), machine.root.join("count"));
    let mut app = App::start(
        Run::new(&bundle.binary)
            .vars(machine.vars())
            .var("CFSMOKE_GO", &go)
            .var("CFSMOKE_COUNT", &count)
            .var("CFSMOKE_FLOOD_LINES", FLOOD_LINES.to_string())
            .cwd(&machine.root),
        &machine.root,
        &machine.state,
        patience,
    )?;
    app.wait_for("rendered")?;
    // An app still coming up shows itself again once it is active: it is hidden a few
    // seconds after its page drew the window, and found hidden still when its window prints.
    thread::sleep(SETTLE);
    assert!(hide(app.pid(), true), "the app was not hidden");
    thread::sleep(HIDDEN_FOR);
    assert!(
        is_hidden(app.pid()),
        "the app showed itself again while hidden"
    );

    files::write(&go, "")?;
    let started = Instant::now();
    let printed = loop {
        let said = files::read_string(&count).unwrap_or_default();
        if let Some(lines) = said.trim().strip_suffix(" done") {
            break lines.to_owned();
        }
        assert!(
            started.elapsed() < DRAIN,
            "the hidden app read {} of the {FLOOD_LINES} lines its window printed in {} s",
            if said.trim().is_empty() {
                "none"
            } else {
                said.trim()
            },
            DRAIN.as_secs(),
        );
        thread::sleep(Duration::from_millis(250));
    };
    println!(
        "a hidden app read {printed} lines ({:.1} MiB) in {:.1} s",
        (FLOOD_LINES * 131) as f64 / f64::from(1 << 20),
        started.elapsed().as_secs_f64()
    );
    assert_eq!(printed, FLOOD_LINES.to_string());
    assert!(
        is_hidden(app.pid()),
        "the app showed itself again while its window printed"
    );

    hide(app.pid(), false);
    app.quit();
    let ended = app.exit()?;
    assert!(ended.is_some(), "the app did not end once its input ended");
    machine.cleanup()?;
    Ok(())
}

/// Whether the app of `pid` is hidden, as AppKit says.
fn is_hidden(pid: u32) -> bool {
    asked(&format!("{}.isHidden", running(pid)))
}

/// Hides the app of `pid`, or shows it again, as a person does; whether it is hidden
/// after, once it is as asked or a few seconds have passed.
fn hide(pid: u32, hidden: bool) -> bool {
    let verb = if hidden { "hide" } else { "unhide" };
    asked(&format!("{}.{verb}; true", running(pid)));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let is = is_hidden(pid);
        if is == hidden || Instant::now() > deadline {
            return is;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// AppKit's running application of `pid`, in JavaScript for Automation.
fn running(pid: u32) -> String {
    format!("$.NSRunningApplication.runningApplicationWithProcessIdentifier({pid})")
}

/// Whether `script`, run by `osascript` with AppKit, says `true`.
fn asked(script: &str) -> bool {
    Run::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e"])
        .arg(format!("ObjC.import('AppKit'); {script}"))
        .run()
        .is_ok_and(|ran| ran.stdout.trim() == "true")
}
