//! Live: a real harness as the chief of a project whose wake-up calls the human
//! turned on a minute apart: the nudge reaches it once, in its own record, and
//! it answers; nothing goes into a turn it is in; a quiet board is not nudged
//! again; a task that moves and words that are edited bring the next, whole. It
//! is opt-in (`cargo xtask live wake-ups`, `npm run live:wake-ups`): it spends
//! the machine's quota on a chief of each harness named for about nine minutes
//! of a cheap model's idling and answering, and needs each installed and logged
//! in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own
//! (never `~/.consensflow`), one rig for each chief, one chief after another.
//! The interval is the one the human sets, a minute, the least a wake-up may
//! wait: no variable or switch shortens it. The windows run the machine's
//! harnesses and read their logins where they are; Claude Code starts with no
//! `CLAUDE_CONFIG_DIR`, through the wrapper that adds `--strict-mcp-config
//! --no-chrome`.
//!
//! What each chief is held to is read from the evidence, never from what it
//! said: the daemon's own event file (the wake-ups queued, when, and what moved
//! in the log between two of them), the harness's own record of the chief's
//! conversation as the daemon reads it (the words of the wake-up in one user
//! item under its header, and the words of the chief after it), the board, and
//! the human's inbox ([`verdict`], made of [`evidence`]; [`run`] is the order
//! they are asked in).
//!
//! ```text
//! cargo xtask live wake-ups                          Claude, the default
//! cargo xtask live wake-ups codex                    one harness
//! cargo xtask live wake-ups claude codex opencode pi devin   each, one after the other
//! cargo xtask live wake-ups fake                     the rig's stand-in as the chief: no model, no quota
//! cargo xtask live wake-ups pi --keep                leaves the run's home and project where they are
//! cargo xtask live wake-ups -- --list                lists this test
//! ```
//!
//! The run fails when a chief's wake-ups did not come, or came, as they should.
//! Words the human has typed in the chief's window and has not sent hold a
//! wake-up too, where the harness holds for them (Claude and Devin; the others'
//! native sends leave the input box alone and are not held, by decision): no
//! run here can leave any, for the human's own typing reaches a window by the
//! desktop app's input alone, so that hold is the engine's test
//! (`crates/cf-engine/tests/dispatcher/wake_ups.rs`).

// The parts are in `live_wake_ups/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "live_wake_ups/evidence.rs"]
mod evidence;
#[path = "live_wake_ups/run.rs"]
mod run;
#[path = "live_wake_ups/verdict.rs"]
mod verdict;
#[path = "live_wake_ups/words.rs"]
mod words;

use std::path::Path;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, between, Harness, Log, Who};
use cf_e2e::Result;

/// The stand-in for the chief of `fake`, and for the worker the task goes to.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the line of each chief in the table says it checked.
const CHECKED: &str = "woken once, at rest, by the setting";

/// The chiefs the words of the command line name, or Claude where it names none.
fn chiefs(named: &[String]) -> Result<Vec<Who>> {
    if named.is_empty() {
        return Ok(vec![Who::Harness(Harness::Claude)]);
    }
    named.iter().map(|word| Who::named(word, "chief")).collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named for about nine minutes each; cargo xtask live wake-ups (npm run live:wake-ups)"]
fn a_chief_of_each_harness_is_woken_once_at_rest_by_a_setting_a_minute_apart_and_never_in_a_turn(
) -> Result {
    let log = Log::begin("live wake-ups")?;
    // What a record is read by is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let whom = chiefs(&live::list("harness"))?;
    let mut checks = Vec::new();
    for who in whom {
        // A harness may update itself while it runs: the run names its version before and after.
        let before = who.version();
        let finished = run::run(who, &run::plan(who), Path::new(FAKE_AGENT), &log)
            .unwrap_or_else(|failed| run::Finished::failed(&failed));
        let version = between(before, who.version());
        log.block(
            &format!(
                "== {} {version}: {}",
                who.name(),
                if finished.case.held() { "ok" } else { "FAILED" }
            ),
            &finished.case.text(),
        );
        checks.push(Check::new(
            who.name(),
            CHECKED,
            finished.case.held(),
            format!("{version}: {}", finished.summary),
        ));
    }
    log.report("what each chief came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_chief_named_claude_is_run_and_those_named_are_run_in_the_order_given() {
        assert_eq!(chiefs(&[]).unwrap(), [Who::Harness(Harness::Claude)]);
        let some = chiefs(&["devin".to_owned(), "fake".to_owned(), "claude".to_owned()]).unwrap();
        assert_eq!(
            some,
            [
                Who::Harness(Harness::Devin),
                Who::StandIn,
                Who::Harness(Harness::Claude)
            ]
        );
        assert_eq!(
            chiefs(&["nobody".to_owned()]).unwrap_err().to_string(),
            "no chief is called nobody: fake, claude, opencode, pi, devin, codex"
        );
    }

    #[test]
    fn each_chief_has_a_project_of_its_own_and_the_stand_ins_worker_for_the_task_that_moves_the_board(
    ) {
        for who in [Who::StandIn, Who::Harness(Harness::Pi)] {
            let plan = run::plan(who);
            assert_eq!(plan.folder, format!("wake-ups-{}", who.name()));
            assert_eq!(plan.roster.len(), 2);
            assert_eq!(plan.roster[1]["id"], "worker");
        }
        assert_eq!(run::plan(Who::StandIn).chief, "chief");
        assert_eq!(run::plan(Who::Harness(Harness::Pi)).chief, "wake-chief");
        assert_eq!(
            run::plan(Who::Harness(Harness::Pi)).harnesses(),
            [Harness::Pi]
        );
    }
}
