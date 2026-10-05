//! Switching the chief to another agent.

use cf_engine::testing::{Context, Made};
use cf_engine::{SwitchTo, SwitchWhen};

use crate::traces::held_to;

const SUITES: &[&str] = &["switching the chief to another agent"];

/// A test's engine with Codex among its harnesses (`withCodex`).
fn with_codex() -> Context {
    Context::made(Made {
        harnesses: vec!["claude-code", "opencode", "codex"],
    })
}

fn to(harness: &str, agent: &str) -> SwitchTo {
    SwitchTo {
        harness: harness.to_owned(),
        agent: agent.to_owned(),
    }
}

#[test]
fn switches_no_chief_of_a_project_it_does_not_know_and_says_so() {
    let context = with_codex();
    let refused = context.switch_chief(42, to("codex", "astraeus"), SwitchWhen::Now, false);
    assert_eq!(refused.unwrap_err().to_string(), "no project 42");
    held_to(
        context.close(),
        SUITES,
        "switches no chief of a project it does not know, and says so",
    );
}
