//! A strategy's run in one window: a first message and an Escape at once, the
//! strategy's keys once Claude reads idle, a second message that must arrive
//! alone, and, when it did, the same keys at the empty box of a window at rest
//! before a third. What it comes to is an [`Outcome`], and what the run makes of
//! that a check ([`check_of`]).

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live;
use cf_e2e::live::claude::{self, Status};
use cf_e2e::live::report::Check;
use cf_e2e::live::window::{Window, ANSWER};
use cf_e2e::{Error, Result};

use super::record::{
    alone, arrival_line, escape_line, idle_after, quoted, stopped_at_once, taken, users, Read,
};
use super::strategy::{Step, Strategy, ESCAPE};

/// How long Claude may take to read idle after the Escape, how long a message may
/// take to reach its record, and how often each is looked at.
const IDLE: Duration = Duration::from_secs(30);
const ARRIVES: Duration = Duration::from_secs(60);
const STATUS_LOOK: Duration = Duration::from_millis(100);
const ARRIVES_LOOK: Duration = Duration::from_millis(250);
const ANSWERED_LOOK: Duration = Duration::from_millis(500);

/// How long Claude is left after the Escape for its record to settle, and after
/// the keys of a strategy for the box to take them.
const SETTLE: Duration = Duration::from_secs(2);
const KEYED: Duration = Duration::from_millis(700);

/// What a strategy that clears the box and is harmless at an empty one is said to do.
const CLEARS: &str = "clears the box and is harmless at an empty one";

/// How the messages a window is sent are made: the run's own, or a test's.
pub type Message = fn(u32, &str) -> String;

/// What a strategy came to: whether it clears the box and is harmless at an
/// empty one, and what was seen on the way.
pub struct Outcome {
    pub ok: bool,
    pub lines: Vec<String>,
}

impl Outcome {
    /// What it is said to do.
    pub fn said(&self) -> &'static str {
        if self.ok {
            CLEARS
        } else {
            "does not"
        }
    }
}

/// The conversation a Claude window is opened on.
fn session_of(window: &Window) -> Result<String> {
    window.session().ok_or_else(|| {
        Error::Daemon("a Claude Code window is opened on a conversation of its own".to_owned())
    })
}

/// Presses the steps of a strategy into the window.
fn press(window: &Window, steps: &[Step]) -> Result {
    for step in steps {
        match step {
            Step::Keys(keys) => {
                window.press(keys)?;
            }
            Step::Pause(pause) => sleep(*pause),
        }
    }
    Ok(())
}

/// Pastes the message `n`, which asks for `word`, as the daemon pastes, and says
/// what arrived in Claude's record and whether the window answered.
fn paste_and_read(window: &Window, make: Message, n: u32, word: &str) -> Result<Read> {
    let (config, session) = (window.claude_config(), session_of(window)?);
    window.paste(&window.given(&make(n, word))?)?;
    let arrived = live::until(ARRIVES, ARRIVES_LOOK, || {
        let record = claude::record(&config, &session);
        let found = users(&record)
            .into_iter()
            .find(|entry| entry.text.contains(&format!("m-{n} ")))
            .cloned();
        found
    });
    let answered = live::until(ANSWER, ANSWERED_LOOK, || {
        claude::record(&config, &session)
            .iter()
            .any(|entry| entry.kind == "assistant" && entry.text.contains(word))
            .then_some(())
    });
    Ok(Read {
        text: arrived.map(|entry| entry.text),
        answered: answered.is_some(),
    })
}

/// Runs `strategy` in the open `window`, its messages made by `make`.
pub fn flow(window: &Window, strategy: &Strategy, make: Message) -> Result<Outcome> {
    let mut lines = Vec::new();
    if let Err(screen) = window.ready() {
        lines.push(format!("FAIL it did not open: {screen}"));
        return Ok(Outcome { ok: false, lines });
    }
    let (config, session) = (window.claude_config(), session_of(window)?);
    let status = Status::watch(&config, &session);
    window.paste(&window.given(&make(1, "FIRST"))?)?;
    let pressed = jiff::Timestamp::now().as_millisecond();
    window.press(&[ESCAPE])?;
    let idle = live::until(IDLE, STATUS_LOOK, || idle_after(&status.samples(), pressed));
    sleep(SETTLE);
    drop(status);
    let record = claude::record(&config, &session);
    lines.push(escape_line(idle.as_ref(), pressed, &record));
    lines.push(format!("its screen: {}", window.tail()));
    if !stopped_at_once(idle.as_ref(), &record) {
        lines.push(
            "FAIL Claude did not stop before it wrote anything: this is not the shape the strategy is for"
                .to_owned(),
        );
        return Ok(Outcome { ok: false, lines });
    }
    press(window, strategy.steps)?;
    sleep(KEYED);
    lines.push(format!("after {}: {}", strategy.name, window.tail()));
    let second = paste_and_read(window, make, 2, "SECOND")?;
    lines.push(arrival_line(&second, alone(second.text.as_deref(), 2)));
    let cleared = taken(&second, 2);
    let mut harmless = true;
    if cleared {
        window.still(ANSWER);
        press(window, strategy.steps)?;
        sleep(KEYED);
        let third = paste_and_read(window, make, 3, "THIRD")?;
        harmless = taken(&third, 3);
        lines.push(if harmless {
            "ok   the same keys at the empty box of a window at rest: the third message arrived alone and was answered".to_owned()
        } else {
            format!(
                "FAIL the same keys at the empty box of a window at rest: the third message did not: {}; its screen: {}",
                quoted(third.text.as_deref(), 160),
                window.tail()
            )
        });
    }
    Ok(Outcome {
        ok: cleared && harmless,
        lines,
    })
}

/// What a strategy's outcome is as a check of the run: a strategy that is not
/// required is reported as what it did and does not fail the run.
pub fn check_of(strategy: &Strategy, outcome: &Outcome, required: bool) -> Check {
    let detail = if outcome.ok || required {
        outcome.said().to_owned()
    } else {
        format!("{}, and nothing here requires it to", outcome.said())
    };
    Check::new("claude", strategy.name, outcome.ok || !required, detail)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use cf_e2e::live::report;
    use cf_e2e::live::window::Open;
    use cf_e2e::live::{Harness, Installed};
    use cf_e2e::rig::{Config, Rig};

    use super::super::strategy::strategy;
    use super::*;

    const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

    /// `n` seconds.
    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_strategy_that_is_not_required_is_reported_for_what_it_did_and_does_not_fail_the_run() {
        let none = strategy("none").unwrap();
        let cleared = Outcome {
            ok: true,
            lines: vec![],
        };
        let failed = Outcome {
            ok: false,
            lines: vec![],
        };
        assert_eq!(
            check_of(none, &cleared, true),
            Check::passed(
                "claude",
                "none",
                "clears the box and is harmless at an empty one"
            )
        );
        assert_eq!(
            check_of(none, &failed, true),
            Check::failed("claude", "none", "does not")
        );
        assert_eq!(
            check_of(none, &failed, false),
            Check::passed(
                "claude",
                "none",
                "does not, and nothing here requires it to"
            )
        );
        // A run of strategies none of which is required still made a check, and passes.
        report::verdict(&[check_of(none, &failed, false)]).unwrap();
        assert!(report::verdict(&[check_of(none, &failed, true)]).is_err());
    }

    /// The messages the stand-in agent can act on: it answers `Reply with exactly:
    /// <word>`, and a first turn that says `HOOKS <seconds>` is still in the hooks of
    /// its prompt, which Claude's is when it is interrupted at once: idle, its record
    /// holding the message and nothing of an answer.
    fn stand_in_message(n: u32, word: &str) -> String {
        let hooks = if n == 1 { "HOOKS 20 " } else { "" };
        format!("[ConsensFlow m-{n} · T-1 · task from @chief]\n{hooks}Reply with exactly: {word}")
    }

    #[test]
    fn the_whole_flow_runs_against_a_window_that_stops_at_once_and_takes_each_message_alone(
    ) -> Result {
        let rig = Rig::start(Config::new(FAKE_AGENT))?;
        let home = rig.var("HOME").unwrap_or_default().to_owned();
        let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
        let window = Open::new(&rig, &install, "input-box", "input-box-ctrl-c")
            .without_trust()
            .open()?;
        let outcome = flow(&window, strategy("ctrl-c")?, stand_in_message)?;
        let said = outcome.lines.join("\n");
        assert!(outcome.ok, "{said}");
        // Claude read idle after the Escape (the stand-in's status says no time for its change),
        // with its record holding the message and no answer.
        assert!(
            said.contains("s after it, its status changed at "),
            "{said}"
        );
        assert!(
            said.contains("0 assistant records and 1 user record"),
            "{said}"
        );
        assert!(
            said.contains("the second message arrived alone, and was answered"),
            "{said}"
        );
        assert!(
            said.contains("the third message arrived alone and was answered"),
            "{said}"
        );
        drop(window);
        rig.close()
    }

    #[test]
    fn the_keys_of_a_strategy_reach_the_window_and_its_pauses_are_waited() -> Result {
        let rig = Rig::start(Config::new(FAKE_AGENT))?;
        let home = rig.var("HOME").unwrap_or_default().to_owned();
        let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
        let window = Open::new(&rig, &install, "input-box", "input-box-keys")
            .without_trust()
            .open()?;
        assert_eq!(window.ready(), Ok(()));
        // These leave the stand-in as it is (which takes no Control-U, and has no turn at
        // work for an Escape to stop); the pause between the Escapes is waited.
        let started = Instant::now();
        for name in ["none", "ctrl-u", "esc-esc"] {
            press(&window, strategy(name)?.steps)?;
        }
        assert!(started.elapsed() >= Duration::from_millis(300));
        sleep(Duration::from_secs(1));
        assert!(!window.closed());
        // The stand-in clears what is typed at a Control-C, and ends at a second one in a row:
        // the bytes of the strategy are the key's.
        let ctrl_c = strategy("ctrl-c")?;
        press(&window, ctrl_c.steps)?;
        sleep(Duration::from_secs(1));
        assert!(!window.closed(), "one Control-C does not end the window");
        press(&window, ctrl_c.steps)?;
        rig.wait_for(
            "the window to end at the second Control-C",
            secs(30),
            || Ok(window.closed()),
        )?;
        drop(window);
        rig.close()
    }
}
