//! Live: a question a member asks through its harness's door, answered from the
//! board, is received there, once (the receipt and stop design, §1.2, §1.6, §2.5,
//! §2.7). It is opt-in (`cargo xtask live door`, `npm run live:door`): it spends
//! the machine's quota on a member of each harness named, and needs each one
//! installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! Each harness whose member asks through a door opens as a member of a project
//! on the native daemon, one after another: Claude Code through its hook (`cf hook
//! claude`), Codex through the broker, OpenCode through its extension
//! (`hosts/lib/question-door.js`), Devin through its hook (`cf hook devin`). Pi has
//! no question tool, so no door: it is left out, which is no failure. The member is
//! given a task that has it ask which colour to use; the check, as the chief,
//! answers it from the board with `cf answer`, and shows from what the daemon wrote
//! that the answer was
//!
//! - claimed by the door (`delivery.claimed`),
//! - acknowledged `received: true` (`message.read` with the receipt `{"door": true}`:
//!   only an acknowledged claim leaves that proof),
//! - never pasted (no `delivery.begun` or `delivery.confirmed` for it, no attempts,
//!   no second answer),
//!
//! and that its task went from waiting back to working, and the result carries the
//! answer ([`cf_e2e::live::members::judge`]). An answer the door fails to
//! acknowledge comes back as text: the paste is the failure this check tells apart
//! from a receipt, whichever harness's door it is.
//!
//! Claude's run also reads the window's status the way the daemon reads it
//! (`~/.claude/sessions/<pid>.json`) over the whole wait of the hook, which `--hold`
//! stretches: if it ever read idle there, the daemon's look at rest would let an
//! answer be pasted into a window that is mid-hook ([`status`]).
//!
//! The chief is the stand-in, and the runs of a machine share the Claude folder it
//! writes in, which a rig that ends takes when it is empty. `--tidy true` is that rig
//! ending while the chief has heard nothing: the stand-in has to write again where it
//! writes, or it ends with the question, and the answer is refused (2026-10-12).
//!
//! ```text
//! cargo xtask live door                         Claude, Codex, OpenCode, Devin, and Pi's line
//! cargo xtask live door opencode --hold 10
//! cargo xtask live door claude --hold 90        Claude's status over a 90 s wait (npm run live:hook-status)
//! cargo xtask live door claude --tidy true      another run's rig ends before the chief hears the question
//! cargo xtask live door fake                    the rig's stand-in Claude: no model, which no quota pays for
//! cargo xtask live door --model opencode=opencode/some-model
//! ```
//!
//! The run fails when a door did not receive its answer as it should have.

// The parts are in `live_door/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_door/check.rs"]
mod check;
#[path = "live_door/sequence.rs"]
mod sequence;
#[path = "live_door/status.rs"]
mod status;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::Account;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, between, Log, Who};
use cf_e2e::{Error, Result};

use check::Asked;

/// The stand-in for the chief's window, and for the member of `fake`.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The harnesses run when none is named, in the order they are run in.
const DEFAULT: [&str; 5] = ["claude", "codex", "opencode", "devin", "pi"];

/// The doors the words of the command line name, or all there are.
fn doors(named: &[String]) -> Result<Vec<Who>> {
    if named.is_empty() {
        return DEFAULT.iter().map(|name| check::named(name)).collect();
    }
    named.iter().map(|name| check::named(name)).collect()
}

/// `--hold`: the seconds the chief takes to answer.
fn hold(given: Option<&str>) -> Result<Option<Duration>> {
    given
        .map(|seconds| {
            seconds
                .parse::<f64>()
                .ok()
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(Duration::from_secs_f64)
                .ok_or_else(|| Error::Daemon("--hold takes the seconds to wait".to_owned()))
        })
        .transpose()
}

/// `--model <harness>=<model>`, each: the model a harness's member runs on.
fn models(given: &[String]) -> Result<BTreeMap<String, String>> {
    given
        .iter()
        .map(|pair| {
            pair.split_once('=')
                .filter(|(harness, model)| !harness.is_empty() && !model.is_empty())
                .map(|(harness, model)| (harness.to_owned(), model.to_owned()))
                .ok_or_else(|| {
                    Error::Daemon(format!("--model takes <harness>=<model>, not {pair}"))
                })
        })
        .collect()
}

/// What the command line asked.
fn asked() -> Result<Asked> {
    Ok(Asked {
        hold: hold(live::option("hold").as_deref())?,
        answer: live::option("answer").unwrap_or_else(|| "blue".to_owned()),
        models: models(&live::list("model"))?,
        tidy: live::option("tidy").as_deref() == Some("true"),
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a member of each harness named; cargo xtask live door (npm run live:door)"]
fn a_question_answered_from_the_board_is_received_once_at_the_door_of_the_harness_that_asked(
) -> Result {
    let log = Log::new("live door");
    let asked = asked()?;
    let whom = doors(&live::list("harness"))?;
    let mut checks = Vec::new();
    for who in whom {
        // A harness may update itself while it runs: the run names its version before and after.
        let before = who.version();
        let case = check::run(who, &asked, Path::new(FAKE_AGENT), &log).unwrap_or_else(|failed| {
            let mut case = Account::new();
            case.fail(failed);
            case
        });
        let version = between(before, who.version());
        log.block(
            &format!(
                "== {} {version}: {}",
                who.name(),
                if case.held() { "ok" } else { "FAILED" }
            ),
            &case.text(),
        );
        checks.push(Check::new(
            who.name(),
            "received at its door",
            case.held(),
            version,
        ));
    }
    log.block("what each door came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_harness_named_every_door_is_run_in_the_order_of_the_proof_and_pi_last() {
        let all: Vec<&str> = doors(&[]).unwrap().iter().map(|who| who.name()).collect();
        assert_eq!(all, ["claude", "codex", "opencode", "devin", "pi"]);
        let some = doors(&["fake".to_owned(), "devin".to_owned()]).unwrap();
        assert_eq!(
            some.iter().map(|who| who.name()).collect::<Vec<_>>(),
            ["fake", "devin"]
        );
        assert!(doors(&["nobody".to_owned()]).is_err());
    }

    #[test]
    fn the_hold_is_the_seconds_the_chief_takes_and_what_is_not_a_time_is_refused() {
        assert_eq!(hold(None).unwrap(), None);
        assert_eq!(hold(Some("90")).unwrap(), Some(Duration::from_secs(90)));
        assert_eq!(hold(Some("0.5")).unwrap(), Some(Duration::from_millis(500)));
        for wrong in ["soon", "-1", "", "NaN", "inf"] {
            let failed = hold(Some(wrong)).unwrap_err().to_string();
            assert_eq!(failed, "--hold takes the seconds to wait", "{wrong:?}");
        }
    }

    #[test]
    fn a_model_is_given_for_a_harness_as_the_harness_an_equals_sign_and_the_model() {
        let given = ["opencode=opencode/some-model", "pi=openrouter/x/y=z"].map(str::to_owned);
        let found = models(&given).unwrap();
        assert_eq!(found["opencode"], "opencode/some-model");
        assert_eq!(
            found["pi"], "openrouter/x/y=z",
            "the first equals sign divides"
        );
        assert!(models(&[]).unwrap().is_empty());
        for wrong in ["opencode", "=model", "opencode="] {
            let failed = models(&[wrong.to_owned()]).unwrap_err().to_string();
            assert_eq!(
                failed,
                format!("--model takes <harness>=<model>, not {wrong}")
            );
        }
    }

    /// The whole check against the stand-in, which asks through its hook as Claude's
    /// own tool does: no harness and no quota, and the daemon is the real one, so
    /// what it proves is that the check reads what the daemon writes.
    #[test]
    fn the_check_holds_a_stand_in_that_asks_through_its_hook_and_is_answered_from_the_board() {
        let log = Log::new("live door test");
        let asked = Asked {
            hold: Some(Duration::from_millis(500)),
            answer: "blue".to_owned(),
            models: BTreeMap::new(),
            tidy: false,
        };
        let case = check::run(Who::StandIn, &asked, Path::new(FAKE_AGENT), &log).unwrap();
        assert!(case.held(), "{}", case.text());
        let text = case.text();
        for name in [
            "claimed",
            "acknowledged received: true",
            "never pasted",
            "waiting to working",
            "result carries the answer",
        ] {
            assert!(text.contains(&format!("ok   {name}: ")), "{name}: {text}");
        }
        assert!(
            text.contains("the sequence the daemon wrote (events.jsonl):"),
            "{text}"
        );
        assert!(text.contains("delivery.claimed"), "{text}");
    }

    /// Another run's rig ends while the chief has heard nothing, and takes the empty
    /// folder the stand-ins write in: what the check says of it, and that it holds.
    #[test]
    fn the_check_holds_when_the_folder_of_the_chiefs_conversation_is_taken_before_it_hears_anything(
    ) {
        let log = Log::new("live door test");
        let asked = Asked {
            hold: Some(Duration::from_millis(500)),
            answer: "blue".to_owned(),
            models: BTreeMap::new(),
            tidy: true,
        };
        let case = check::run(Who::StandIn, &asked, Path::new(FAKE_AGENT), &log).unwrap();
        assert!(case.held(), "{}", case.text());
        let text = case.text();
        assert!(
            text.contains("another run's rig ended: ")
                && text.contains("projects/integration is taken"),
            "{text}"
        );
    }
}
