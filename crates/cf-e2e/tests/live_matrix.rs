//! The live matrix (spec `live-matrix`, LMX-04): a real harness in every position of a board,
//! the chief driven by a scripted human through a scenario, and an oracle that reads the
//! ledger and the windows' records (`cf_e2e::matrix`). It is opt-in: it spends the quota of
//! every harness it seats, so it is ignored unless asked for and is no part of `check` or CI.
//!
//! ```text
//! cargo xtask live matrix --board 0 --scenario smoke -- --test-threads 1
//! ```
//!
//! Board 0 seats Claude Code as the chief, Codex as the worker, Pi as the reviewer, Devin as
//! the advisor and Muse Code as the tester, each on the kit's model for it. The evidence of a
//! run is under `evals/reports/live-matrix/<run-id>/`. The control below runs the same driver
//! and oracle on the rig's stand-ins, offline.

use std::path::Path;

use cf_e2e::checkout;
use cf_e2e::live::report;
use cf_e2e::live::{self, Log};
use cf_e2e::matrix::evidence::{Owner, REPORTS};
use cf_e2e::matrix::run::{run, Matrix};
use cf_e2e::matrix::scenario::Scenario;
use cf_e2e::matrix::seating::{Role, Seating};
use cf_e2e::{Error, Result};

/// The stand-in agent's program: the stand-in seats' windows, and the one a rig has where no
/// window runs it.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The board and the scenario the words ask for: `--board` (0 where none is said) and
/// `--scenario` (`smoke`), with `--stand-in` naming the members' positions the rig's stand-in
/// plays where this machine cannot seat the board's harness today (`--stand-in reviewer`: Pi
/// with no login of its model's provider), which the run says it covers none of. The harnesses
/// are the board's; a harness named besides is refused.
fn asked(
    board: Option<&str>,
    scenario: Option<&str>,
    stand_in: Option<&str>,
    harnesses: &[String],
) -> Result<(Seating, Scenario)> {
    if !harnesses.is_empty() {
        return Err(Error::Daemon(format!(
            "the matrix seats the harnesses its board names, and takes none by name: {}",
            harnesses.join(" ")
        )));
    }
    let number = board.unwrap_or("0");
    let board = number
        .parse::<u8>()
        .map_err(|_| Error::Daemon(format!("--board takes a board's number, not {number}")))?;
    let roles = stand_in
        .unwrap_or_default()
        .split(',')
        .filter(|word| !word.is_empty())
        .map(|word| {
            Role::named(word).ok_or_else(|| {
                Error::Daemon(format!(
                    "--stand-in takes worker, reviewer, advisor or tester, not {word}"
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        Seating::board(board)?.with_stand_ins(&roles)?,
        Scenario::named(scenario.unwrap_or("smoke"))?,
    ))
}

#[test]
#[ignore = "opt-in: spends the quota of every harness the board seats; cargo xtask live matrix --board 0 --scenario smoke -- --test-threads 1"]
fn a_board_of_real_harnesses_goes_through_its_scenario_as_the_ledger_and_the_records_say() -> Result
{
    let log = Log::begin("live matrix")?;
    let (seating, scenario) = asked(
        live::option("board").as_deref(),
        live::option("scenario").as_deref(),
        live::option("stand-in").as_deref(),
        &live::list("harness"),
    )?;
    let not_covered: Vec<&str> = seating.not_covered().into_iter().map(Role::name).collect();
    if !not_covered.is_empty() {
        log.say(format!(
            "THE STAND-IN PLAYS {}: this run covers no harness there",
            not_covered.join(", ").to_uppercase()
        ));
    }
    let matrix = Matrix {
        seating,
        scenario,
        stand_in: Path::new(FAKE_AGENT),
        reports: checkout::path(REPORTS),
        owner: Owner::of_machine()?,
    };
    let outcome = run(&matrix, &log)?;
    log.say(format!(
        "{} s, {} turns: {:?}; the evidence is in {}{}",
        outcome.elapsed.as_secs(),
        outcome.total_turns,
        outcome.turns,
        outcome.evidence.folder.display(),
        if not_covered.is_empty() {
            String::new()
        } else {
            format!("; NOT COVERED (the stand-in): {}", not_covered.join(", "))
        }
    ));
    report::conclude(
        &log,
        "what each check of the matrix came to",
        &outcome.checks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_ask_for_board_0_and_smoke_and_nothing_else_yet() {
        let (seating, scenario) = asked(None, None, None, &[]).unwrap();
        assert_eq!((seating.board, scenario.name), (Some(0), "smoke"));
        assert!(seating.not_covered().is_empty());
        let (seating, _) = asked(Some("0"), None, Some("reviewer"), &[]).unwrap();
        assert_eq!(seating.not_covered(), [Role::Reviewer]);
        assert!(asked(None, None, Some("chief"), &[])
            .err()
            .unwrap()
            .to_string()
            .contains("not the chief's"));
        assert!(asked(None, None, Some("judge"), &[])
            .err()
            .unwrap()
            .to_string()
            .contains("not judge"));
        let refused = |board, scenario, harnesses: &[String]| {
            asked(board, scenario, None, harnesses)
                .err()
                .map(|failed| failed.to_string())
                .unwrap_or_default()
        };
        assert!(refused(Some("1"), None, &[]).contains("board 0 alone so far"));
        assert!(refused(Some("one"), None, &[]).contains("--board takes a board's number, not one"));
        assert!(refused(None, Some("core"), &[]).contains("smoke alone so far"));
        assert!(refused(None, None, &["claude".to_owned()]).contains("takes none by name: claude"));
    }

    /// The control of the run's end and of the product's refusals (T-1152): the stand-in chief,
    /// in one turn, puts the advice task on the board, cancels it, and asks to send it back,
    /// which the product refuses. The run ends at once, as a red that names the cancelled task,
    /// long before its deadline; the refusal's code is the daemon's, read from what a tooling
    /// daemon writes of each request (`api.answered`), and no line of it holds a token.
    #[test]
    fn a_cancelled_task_ends_the_run_at_once_and_the_products_refusal_is_read_by_its_code() {
        use cf_e2e::matrix::scenario::Refused;
        let log = Log::new("live matrix control: an end");
        let reports = tempfile::tempdir().unwrap();
        let mut scenario = Scenario::named("smoke").unwrap();
        scenario.steps.truncate(1);
        // A run that went on past the cancelled task would end at this deadline, red for
        // another reason, and soon.
        scenario.deadline = std::time::Duration::from_secs(90);
        scenario.steps[0].stand_in = |_| {
            "CF task add --advice --tier standard :: SLEEP 60 Reply with exactly: ADVICE_OK\nCF task cancel T-1\nCF task reopen T-1 :: Once more."
                .to_owned()
        };
        scenario.refusals = &[Refused {
            participant: "chief",
            method: "POST",
            path: "/api/tasks/1/reopen",
            code: "invalid-transition",
        }];
        let matrix = Matrix {
            seating: Seating::stand_ins(),
            scenario,
            stand_in: Path::new(FAKE_AGENT),
            reports: reports.path().to_path_buf(),
            owner: Owner::look(&[], &[]),
        };
        let outcome = run(&matrix, &log).unwrap();
        let table = report::table(&outcome.checks);
        log.report("what each check of the control came to", &table);
        let check = |who: &str, what: &str| {
            outcome
                .checks
                .iter()
                .find(|check| check.who == who && check.what.contains(what))
                .unwrap_or_else(|| panic!("no check of {who} {what}:\n{table}"))
                .clone()
        };
        let step = check("steps", "step 1 (advice) was reached");
        assert!(!step.ok, "{table}");
        assert_eq!(step.detail, "T-1, the advice task, is cancelled", "{table}");
        assert!(
            outcome.elapsed.as_secs() < 120,
            "ended at once: {} s",
            outcome.elapsed.as_secs()
        );
        let refused = check(
            "chief",
            "POST /api/tasks/1/reopen was refused with invalid-transition",
        );
        assert!(refused.ok, "{table}");
        let events = std::fs::read_to_string(outcome.evidence.folder.join("events.jsonl")).unwrap();
        let answered: Vec<&str> = events
            .lines()
            .filter(|line| line.contains("\"api.answered\""))
            .collect();
        assert!(answered.len() >= 3, "{answered:?}");
        assert!(
            answered
                .iter()
                .all(|line| !line.contains("Bearer") && !line.contains("token")),
            "{answered:?}"
        );
    }

    /// The control: the same driver and oracle, on a board whose five positions the rig's
    /// stand-in plays. The human asks the stand-in chief in its own words, which hands out the
    /// advice, the change, the review and the test and accepts them; the oracle reads the
    /// ledger and the records, and every check holds. No harness is covered by it.
    #[test]
    fn the_matrix_passes_its_scenario_on_the_rigs_stand_ins() {
        let log = Log::new("live matrix control");
        let reports = tempfile::tempdir().unwrap();
        let owned = reports.path().join("owner.json");
        std::fs::write(&owned, "{}").unwrap();
        let matrix = Matrix {
            seating: Seating::stand_ins(),
            scenario: Scenario::named("smoke").unwrap(),
            stand_in: Path::new(FAKE_AGENT),
            reports: reports.path().to_path_buf(),
            owner: Owner::look(&[], &[owned]),
        };
        let outcome = run(&matrix, &log).unwrap();
        let table = report::table(&outcome.checks);
        log.report("what each check of the control came to", &table);
        assert!(outcome.checks.iter().all(|check| check.ok), "{table}");
        // Every check the oracle has was made: the staff, the asks, the tasks, the fixture,
        // the bounds and the owner's.
        for who in [
            "staff",
            "chief",
            "steps",
            "human",
            "advice (advisor stand-in)",
            "change (worker stand-in)",
            "review (reviewer stand-in)",
            "test (tester stand-in)",
            "fixture",
            "bounds",
            "owner",
        ] {
            assert!(
                outcome.checks.iter().any(|check| check.who == who),
                "no check of {who}:\n{table}"
            );
        }
        assert!(outcome.total_turns <= 20, "{:?}", outcome.turns);
        for file in [
            "manifest.json",
            "human.json",
            "bindings.json",
            "events.jsonl",
            "checks.json",
            "verdict.txt",
            "opens.json",
            "daemon-tail.log",
        ] {
            assert!(
                outcome.evidence.folder.join(file).is_file(),
                "no {file} in {}",
                outcome.evidence.folder.display()
            );
        }
        let verdict = std::fs::read_to_string(outcome.evidence.folder.join("verdict.txt")).unwrap();
        assert!(verdict.starts_with("GREEN"), "{verdict}");
    }
}
