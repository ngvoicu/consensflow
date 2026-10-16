//! What each check of a live run came to, as the run says it at its end and as it
//! fails: a line for each (`ok` or `FAIL`, whose it was, which check, and what
//! was seen), in the order they ran, and the failure of the run that names the
//! checks that did not hold. A check that did not run because one before it
//! failed is no row: say so in the run's own words.

use super::{cut, Log};
use crate::{Error, Result};

/// How much of what was seen a row shows.
const DETAIL: usize = 600;

/// One check of a run: whose it was (a harness, an agent), which it was, and
/// whether it held, with what was seen either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub who: String,
    pub what: String,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    /// A check of `who` that held.
    pub fn passed(who: &str, what: &str, detail: impl Into<String>) -> Self {
        Self::new(who, what, true, detail)
    }

    /// A check of `who` that did not.
    pub fn failed(who: &str, what: &str, detail: impl Into<String>) -> Self {
        Self::new(who, what, false, detail)
    }

    /// A check of `who` that held where `ok` says so.
    pub fn new(who: &str, what: &str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            who: who.to_owned(),
            what: what.to_owned(),
            ok,
            detail: detail.into(),
        }
    }

    /// Its line: `ok   claude  one line  sent and recorded whole`.
    fn line(&self, who: usize, what: usize) -> String {
        format!(
            "{} {:<who$}  {:<what$}  {}",
            if self.ok { "ok  " } else { "FAIL" },
            self.who,
            self.what,
            cut(&self.detail, DETAIL)
        )
    }
}

/// The checks, a line to each, their columns as wide as the widest of each.
pub fn table(checks: &[Check]) -> String {
    let who = checks.iter().map(|check| check.who.chars().count()).max();
    let what = checks.iter().map(|check| check.what.chars().count()).max();
    checks
        .iter()
        .map(|check| check.line(who.unwrap_or(0), what.unwrap_or(0)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether every check held: the checks that did not, if some did not, as the
/// error of the run. A run that made no check proved nothing, and fails as well.
pub fn verdict(checks: &[Check]) -> Result {
    if checks.is_empty() {
        return Err(Error::Daemon(
            "the run made no check: nothing was proved".to_owned(),
        ));
    }
    let failed: Vec<Check> = checks.iter().filter(|check| !check.ok).cloned().collect();
    if failed.is_empty() {
        return Ok(());
    }
    Err(Error::Daemon(format!(
        "{} of {} checks failed:\n{}",
        failed.len(),
        checks.len(),
        table(&failed)
    )))
}

/// How a run of checks ends: it says what each came to, under `title`, then what
/// the harnesses ran on where the run says it ([`Log::begin`]), and fails with the
/// ones that did not hold ([`verdict`]).
pub fn conclude(log: &Log, title: &str, checks: &[Check]) -> Result {
    log.report(title, &table(checks));
    verdict(checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Check> {
        vec![
            Check::passed(
                "claude",
                "one line",
                "sent and recorded whole, answered in 4.2 s",
            ),
            Check::failed("devin", "a ConsensFlow message", "NOT SENT: the screen"),
            Check::new("claude", "a long message (3700 characters)", true, "sent"),
        ]
    }

    #[test]
    fn each_check_is_a_line_with_its_columns_as_wide_as_the_widest() {
        assert_eq!(
            table(&sample()),
            "ok   claude  one line                          sent and recorded whole, answered in 4.2 s\n\
             FAIL devin   a ConsensFlow message             NOT SENT: the screen\n\
             ok   claude  a long message (3700 characters)  sent"
        );
        assert_eq!(table(&[]), "");
    }

    #[test]
    fn what_was_seen_is_cut_where_it_is_too_long_and_set_on_one_line() {
        let long = format!("first\nsecond {}", "x".repeat(DETAIL));
        let line = Check::failed("a", "b", long).line(1, 1);
        assert!(line.starts_with("FAIL a  b  first second xxx"), "{line}");
        assert!(line.ends_with('…'), "{line}");
        assert!(!line.contains('\n'));
    }

    #[test]
    fn a_run_holds_when_every_check_did_and_fails_with_the_ones_that_did_not() {
        let mut checks = sample();
        let failed = verdict(&checks).unwrap_err().to_string();
        assert_eq!(
            failed,
            "1 of 3 checks failed:\nFAIL devin  a ConsensFlow message  NOT SENT: the screen"
        );
        checks.remove(1);
        verdict(&checks).unwrap();
    }

    #[test]
    fn a_run_that_checked_nothing_proved_nothing() {
        let failed = verdict(&[]).unwrap_err().to_string();
        assert_eq!(failed, "the run made no check: nothing was proved");
    }

    #[test]
    fn a_run_ends_by_saying_what_each_check_came_to_and_failing_with_those_that_did_not() {
        let log = Log::new("live test");
        let mut checks = sample();
        let failed = conclude(&log, "what each check came to", &checks).unwrap_err();
        assert_eq!(
            failed.to_string(),
            verdict(&checks).unwrap_err().to_string()
        );
        checks.remove(1);
        conclude(&log, "what each check came to", &checks).unwrap();
        assert!(conclude(&log, "nothing", &[]).is_err());
    }

    #[test]
    fn the_report_of_a_run_that_begins_ends_with_what_each_of_its_windows_was_launched_on() {
        use serde_json::json;

        let checks = table(&sample());
        // A run of agents of its own says none.
        assert_eq!(Log::new("live agent").reported(&checks), checks);
        // A run that launched nothing says so.
        let asked = crate::live::agents::parse(Some("pi=openai/gpt-6-luna"), None).unwrap();
        let begun = Log::begun("live test", Ok(asked)).unwrap();
        assert_eq!(
            begun.reported(&checks),
            format!("{checks}\nno window was launched")
        );
        // What each window was launched on, in the order they were, once each: not what
        // --models gave (a suite's own flag gave Pi another model here), and never the
        // stand-in agent's. Pi's effort is its thinking, as Pi reads it.
        let project = begun.scoped("claude");
        project
            .launched(&json!({ "kind": "pi", "model": "openai/gpt-5.6-luna", "thinking": "low" }));
        begun.launched(&json!({ "kind": "codex", "model": "gpt-6.1-sol", "effort": "high" }));
        begun.launched(&json!({ "kind": "claude-code", "model": "fake-chief" }));
        project
            .launched(&json!({ "kind": "pi", "model": "openai/gpt-5.6-luna", "thinking": "low" }));
        let ran_on = "the windows ran on: pi openai/gpt-5.6-luna, effort low; codex gpt-6.1-sol, effort high";
        assert_eq!(begun.ran_on().as_deref(), Some(ran_on));
        assert_eq!(begun.reported(&checks), format!("{checks}\n{ran_on}"));
        // One project of the run says it as the run does.
        assert_eq!(project.reported(&checks), begun.reported(&checks));
        // The first line is what the flags give, which is not the report's.
        let line = crate::live::agents::parse(Some("pi=openai/gpt-6-luna"), None)
            .unwrap()
            .line();
        assert!(
            line.starts_with(
                "the models --models and the cheap ones give: claude claude-sonnet-5-5; "
            ),
            "{line}"
        );
        assert!(line.contains("; pi openai/gpt-6-luna; "), "{line}");
        // One asked for what no window could be asked for begins not at all, with what
        // is wrong as its error.
        let refused = Log::begun(
            "live test",
            Err(crate::Error::Daemon("--models names pi twice".to_owned())),
        );
        assert_eq!(
            refused.err().map(|failed| failed.to_string()).as_deref(),
            Some("--models names pi twice")
        );
    }
}
