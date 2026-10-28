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

use std::io::Write;
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
/// first's time is the page's own. That this bound is red on main's page is the
/// measurement recorded with the change (T-454's commit), not a run of this case: a run
/// is held to the bound alone. The smoke's cases run one at a time (`cargo xtask smoke`,
/// `--test-threads=1`), so the flood never lands while the other case floods an app of
/// its own, which on a release's runner (3 cores) would take the time of both.
const DRAIN: Duration = Duration::from_secs(10);

/// How many times AppKit is asked whether the app is hidden before its answer is taken,
/// and how long apart: right after a flood `osascript` can answer late or not at all.
const ASKED: usize = 5;
const ASKED_APART: Duration = Duration::from_millis(500);

#[test]
#[ignore = "opt-in, on a built app: cargo xtask smoke (npm run smoke)"]
fn the_built_app_reads_its_windows_while_it_is_hidden() -> Outcome {
    let bundle = bundle::locate(&bundle::requested(
        own_var("CONSENSFLOW_SMOKE_APP").as_deref(),
    ))?;
    let patience = patience_of(own_var("CONSENSFLOW_SMOKE_TIMEOUT_MS").as_deref())?;
    // A failed run leaves its machine behind, as the other case's does: a check that does
    // not hold says where it is as the machine goes (`Machine`'s drop), and a step that
    // could not be taken says it here.
    let machine = Machine::with_harness(Path::new(PASTE_READER), patience, HARNESS)?;
    // The app built is of the live app's identity: what is started is a copy of the smoke's.
    let bundle = bundle::smoke_copy(&bundle, &machine.root)?;
    let outcome = drive(&bundle, &machine, patience, &mut std::io::stdout());
    let keep = own_var("CONSENSFLOW_SMOKE_KEEP").as_deref() == Some("1");
    machine.say_where_kept(&mut std::io::stderr(), outcome.is_err() || keep);
    outcome?;
    if !keep {
        machine.cleanup()?;
    }
    Ok(())
}

/// The case, on the app of `bundle` started on `machine`: how long the flood took is said
/// to `out`.
fn drive(
    bundle: &bundle::Bundle,
    machine: &Machine,
    patience: Duration,
    out: &mut impl Write,
) -> Outcome {
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
    let hidden = hide(app.pid(), true);
    assert!(
        hidden == Ok(true),
        "the app was not hidden: {}",
        said(&hidden)
    );
    thread::sleep(HIDDEN_FOR);
    let hidden = is_hidden(app.pid());
    assert!(
        hidden == Ok(true),
        "the app showed itself again while hidden: {}",
        said(&hidden)
    );

    files::write(&go, "")?;
    let started = Instant::now();
    let printed = loop {
        let counted = files::read_string(&count).unwrap_or_default();
        if let Some(lines) = counted.trim().strip_suffix(" done") {
            break lines.to_owned();
        }
        assert!(
            started.elapsed() < DRAIN,
            "the hidden app read {} of the {FLOOD_LINES} lines its window printed in {} s",
            if counted.trim().is_empty() {
                "none"
            } else {
                counted.trim()
            },
            DRAIN.as_secs(),
        );
        thread::sleep(Duration::from_millis(250));
    };
    writeln!(
        out,
        "a hidden app read {printed} lines ({:.1} MiB) in {:.1} s",
        (FLOOD_LINES * 131) as f64 / f64::from(1 << 20),
        started.elapsed().as_secs_f64()
    )?;
    assert_eq!(printed, FLOOD_LINES.to_string());
    let hidden = is_hidden(app.pid());
    assert!(
        hidden == Ok(true),
        "the app showed itself again while its window printed: {}",
        said(&hidden)
    );

    let _ = hide(app.pid(), false);
    app.quit();
    let ended = app.exit()?;
    assert!(ended.is_some(), "the app did not end once its input ended");
    Ok(())
}

/// What AppKit answered, in words for a failure: that the app is not hidden, or why it
/// could not be asked.
fn said(answer: &Result<bool, String>) -> String {
    match answer {
        Ok(true) => "AppKit says it is hidden".into(),
        Ok(false) => "AppKit says it is not hidden".into(),
        Err(why) => why.clone(),
    }
}

/// Whether the app of `pid` is hidden, as AppKit says: asked again a few times while it
/// says not, or cannot be asked, and its last answer taken.
fn is_hidden(pid: u32) -> Result<bool, String> {
    settled(true, ASKED, ASKED_APART, || {
        asked(&format!("{}.isHidden", running(pid)))
    })
}

/// Hides the app of `pid`, or shows it again, as a person does; whether it is hidden
/// after, once it is as asked or a few seconds have passed.
fn hide(pid: u32, hidden: bool) -> Result<bool, String> {
    let verb = if hidden { "hide" } else { "unhide" };
    asked(&format!("{}.{verb}; true", running(pid)))?;
    settled(hidden, 50, Duration::from_millis(100), || {
        asked(&format!("{}.isHidden", running(pid)))
    })
}

/// What `ask` answers once it answers `wanted`, asked up to `times` times `apart` apart:
/// its last answer, whatever it was, when it never did.
fn settled(
    wanted: bool,
    times: usize,
    apart: Duration,
    mut ask: impl FnMut() -> Result<bool, String>,
) -> Result<bool, String> {
    let mut answer = ask();
    for _ in 1..times {
        if answer == Ok(wanted) {
            break;
        }
        thread::sleep(apart);
        answer = ask();
    }
    answer
}

/// AppKit's running application of `pid`, in JavaScript for Automation.
fn running(pid: u32) -> String {
    format!("$.NSRunningApplication.runningApplicationWithProcessIdentifier({pid})")
}

/// What `script`, run by `osascript` with AppKit, says: `true` or `false`, or why it said
/// neither, in its own words.
fn asked(script: &str) -> Result<bool, String> {
    let ran = Run::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e"])
        .arg(format!("ObjC.import('AppKit'); {script}"))
        .run()
        .map_err(|error| format!("osascript could not be run: {error}"))?;
    match ran.stdout.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        out => Err(format!(
            "osascript answered {out:?} ({}), and said {:?}",
            ran.code
                .map_or_else(|| "no exit code".into(), |code| format!("exit {code}")),
            ran.stderr.trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An asker that gives `answers` in turn, and counts how often it was asked.
    fn answering(
        answers: Vec<Result<bool, String>>,
    ) -> (
        impl FnMut() -> Result<bool, String>,
        std::rc::Rc<std::cell::Cell<usize>>,
    ) {
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = asked.clone();
        let mut answers = answers.into_iter();
        let ask = move || {
            counted.set(counted.get() + 1);
            answers
                .next()
                .unwrap_or_else(|| Err("asked once too often".into()))
        };
        (ask, asked)
    }

    #[test]
    fn whether_the_app_is_hidden_is_asked_again_while_it_says_not_or_cannot_say() {
        let failed =
            || Err::<bool, String>("osascript answered \"\" (exit 1), and said \"x\"".into());
        // Not hidden once, then unanswered, then hidden: taken as hidden, on the third asking.
        let (ask, asked) = answering(vec![Ok(false), failed(), Ok(true), Ok(false)]);
        assert_eq!(settled(true, 5, Duration::ZERO, ask), Ok(true));
        assert_eq!(asked.get(), 3);
        // Hidden at once: asked once.
        let (ask, asked) = answering(vec![Ok(true)]);
        assert_eq!(settled(true, 5, Duration::ZERO, ask), Ok(true));
        assert_eq!(asked.get(), 1);
        // Never as wanted: asked as often as it may be, and its last answer is the one said.
        let (ask, asked) = answering(vec![Ok(false), Ok(false), failed()]);
        assert_eq!(settled(true, 3, Duration::ZERO, ask), failed());
        assert_eq!(asked.get(), 3);
        // Shown again is wanted as well as hidden.
        let (ask, _) = answering(vec![Ok(true), Ok(false)]);
        assert_eq!(settled(false, 5, Duration::ZERO, ask), Ok(false));
    }

    #[test]
    fn a_failure_says_what_appkit_answered_or_why_it_could_not_be_asked() {
        assert_eq!(said(&Ok(false)), "AppKit says it is not hidden");
        assert_eq!(
            said(&Err("osascript said why".into())),
            "osascript said why"
        );
    }
}
