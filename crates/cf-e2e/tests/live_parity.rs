//! Live: a member window has what the same harness has when the human opens it in a
//! terminal in the project's folder (T-540, Gabriel, 2026-10-25: "the testers, reviewers,
//! workers, advisors and so on have the same features as their individual harnesses,
//! when opened in the cli"). It is opt-in (`cargo xtask live parity`): it spends the
//! machine's quota on one short task of each harness named, so it is ignored unless
//! asked for and is no part of `check` or of CI.
//!
//! - **Claude Code's window** (`claude.rs`): a window opened as ConsensFlow opens a member's
//!   loads the MCP servers, plugins, skills and Claude in Chrome that the same Claude Code
//!   loads opened plainly in that folder, and its role's skill besides, on macOS and on
//!   Windows (a ConPTY of ConsensFlow's own there).
//!
//! ```text
//! cargo xtask live parity                                   both
//! cargo xtask live parity -- a_claude_member                Claude Code's alone
//! cargo xtask windows --host zeewin -- cargo xtask live parity -- a_claude_member
//! ```

use std::time::Duration;

// The parts are in `live_parity/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_parity/claude.rs"]
mod claude;
#[path = "live_parity/terminal_env.rs"]
mod terminal_env;

use cf_e2e::live::members::Members;
use cf_e2e::live::{self, last_lines, Harness, Log, Who};
use cf_e2e::{Error, Result};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long a worker has for its task, and how often it is looked at.
const WORK: Duration = Duration::from_secs(300);
const LOOK: Duration = Duration::from_secs(2);

/// The workers named (`cargo xtask live parity <harness>`), Codex and Devin where none
/// is. Each test takes the harnesses of its own.
fn workers() -> Result<Vec<Harness>> {
    let named = live::list("harness");
    if named.is_empty() {
        return Ok(vec![Harness::Codex, Harness::Devin]);
    }
    named
        .iter()
        .map(|word| match Who::named(word, "worker")? {
            Who::Harness(harness @ (Harness::Codex | Harness::Devin)) => Ok(harness),
            _ => Err(Error::Daemon(format!(
                "live parity's workers are Codex and Devin, not {word}"
            ))),
        })
        .collect()
}

/// What the product showed of a worker that did not answer: its lane of the board, what
/// its window recorded and last drew, and what the daemon warned of.
fn failure(members: &Members, harness: Harness, log: &Log) {
    let agent = live::agents::id(harness);
    let rig = members.rig();
    log.block(
        "the worker's lane of the board",
        &live::pretty(members.lane(&agent)),
    );
    log.block(
        "what the worker's window recorded",
        &live::transcript(rig, members.project_id(), 1),
    );
    if let Ok(Some(window)) = members.window_of(&agent) {
        log.block(
            "what the worker's window last drew",
            &last_lines(&rig.screen(window.id())),
        );
    }
    log.block("the daemon's log", &live::warnings(&rig.log()).join("\n"));
}

// Codex's and Devin's parts (T-548), in `live_parity/`: cargo looks for the modules of a
// test's root file beside it, where each would be a suite of its own.
#[path = "live_parity/codex_devin.rs"]
mod codex_devin;

#[test]
#[ignore = "opt-in: spends the machine's quota on two short tasks of each harness named; cargo xtask live parity codex devin"]
fn a_codex_or_devin_worker_has_what_its_harness_has_in_a_terminal_and_takes_its_interrupt_after_typing(
) -> Result {
    let log = Log::begin("live parity")?;
    codex_devin::run(&log, &workers()?)
}

#[test]
#[ignore = "opt-in: opens the machine's Claude Code twice on a copy of its own config, no message given; cargo xtask live parity -- a_claude_member"]
fn a_claude_member_window_loads_what_the_same_claude_loads_in_a_terminal() -> Result {
    let log = Log::begin("live parity (Claude Code)")?;
    claude::run(&log)
}

#[test]
#[ignore = "opt-in: spends the machine's Claude quota on one one-word task; cargo xtask live parity --claude-config <empty folder> -- a_real_claude_member"]
fn a_real_claude_member_window_has_the_terminals_lang_and_its_rcs_export() -> Result {
    let log = Log::begin("live parity (the terminal's environment)")?;
    terminal_env::run(&log)
}
