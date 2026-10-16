//! Live: what a paste does to the text Claude puts back into its input box, and
//! what clears it first. It is opt-in (`cargo xtask live input-box`, `npm run
//! live:input-box`): it spends the machine's quota on a window of Claude Code for
//! each strategy, and needs it installed and logged in, so it is ignored unless
//! asked for and is no part of `check` or of CI.
//!
//! Claude, interrupted before it wrote anything of its answer (an Escape within
//! the first seconds of a turn), puts the message it was given back into its
//! input box and writes no interrupt record. The daemon pastes into a window it
//! finds at rest, and a paste goes in where the cursor is: a message pasted into
//! that box would be sent with the old one in front of it. Each strategy here is
//! a window of its own: a first message, an Escape at once as the daemon presses
//! one, the keys of the strategy once Claude reads idle, then a second message
//! pasted the way the daemon pastes ([`cf_e2e::live::window::Window::paste`]).
//! What Claude's record holds of the second one says whether the old text went
//! with it. When that was right, the same keys are pressed at the empty box of a
//! window at rest, before a third message, which must arrive alone and be
//! answered: a key that harms an empty prompt (a second Escape opens Claude's
//! rewind) is no way to clear one.
//!
//! ```text
//! cargo xtask live input-box                        every strategy
//! cargo xtask live input-box --strategy ctrl-c      one
//! cargo xtask live input-box --require ctrl-c       the run fails unless it cleared
//! ```
//!
//! The strategies ([`strategy`]) are keys written into the pane as the daemon
//! writes its own (`pane.input`), `none` being what a paste does with nothing
//! pressed first. Claude on its cheap model, in a folder of its own, with no MCP
//! server and no browser; each window is killed when its strategy is done.
//!
//! Found with Claude 2.1.292 (2026-10-07): `none` pastes the second message after
//! the first, as one message; `ctrl-u` clears the cursor's line only, leaving the
//! first line of a two-line message; `esc-esc` clears the text and then, at an
//! empty box, opens Claude's rewind, which swallows the next paste; `ctrl-c`
//! clears it whole and is harmless at an empty box. It is the key the daemon
//! presses (`CLEAR_INPUT`, `crates/cf-harness/src/claude/stopped.rs`), so with no
//! `--require` the run asks for it where it is tried. A strategy that is not
//! required is a finding, not a failure: the run reports what it did and passes.

// The parts are in `live_input_box/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "live_input_box/flow.rs"]
mod flow;
#[path = "live_input_box/record.rs"]
mod record;
#[path = "live_input_box/strategy.rs"]
mod strategy;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::Open;
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::Result;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own
/// path), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code for each strategy; cargo xtask live input-box (npm run live:input-box)"]
fn what_clears_the_box_claude_puts_a_message_back_into_clears_it_whole_and_is_harmless_at_an_empty_one(
) -> Result {
    let log = Log::begin("live input-box")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let tried = strategy::chosen(&live::list("strategy"))?;
    let needed = strategy::required(&live::list("require"), &tried)?;
    let claude = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let mut checks = Vec::new();
    for strategy in &tried {
        log.say(format!("strategy {}", strategy.name));
        let pane = format!("input-box-{}", strategy.name);
        let window =
            Open::new(&rig, &claude, "input-box", &pane).open_telling(&log, strategy.name)?;
        let outcome = flow::flow(&window, strategy, record::message)?;
        drop(window);
        log.block(
            &format!("{}: {}", strategy.name, outcome.said()),
            &outcome.lines.join("\n"),
        );
        let wanted = needed.iter().any(|wanted| wanted.name == strategy.name);
        checks.push(flow::check_of(strategy, &outcome, wanted));
    }
    // A strategy the run is told to require and did not try is no strategy that cleared the box.
    for wanted in &needed {
        if !tried.iter().any(|strategy| strategy.name == wanted.name) {
            checks.push(Check::failed(
                "claude",
                wanted.name,
                "it is required, and this run did not try it",
            ));
        }
    }
    rig.close()?;
    report::conclude(&log, "what each strategy came to", &checks)
}
