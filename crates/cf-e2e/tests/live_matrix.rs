//! The live matrix (spec `live-matrix`, LMX-04 to LMX-08, LMX-12): a harness in every position of
//! a board, the chief driven by a scripted human through a scenario, and an oracle that reads
//! the ledger and the windows' records (`cf_e2e::matrix`). Two layers share the driver and the
//! oracle (Gabriel, 2026-11-03: real models only before a release):
//!
//! - the **real layer**, opt-in (ignored): it spends the quota of every harness it seats, and
//!   runs in a release's checks;
//! - the **stand-in layer**, the cases of `mod tests` below: every scenario on the rig's
//!   stand-in agents, offline, in CI on every push with the other live suites' offline cases.
//!
//! ```text
//! cargo xtask live matrix --board 3 --scenario core -- --test-threads 1      one board, 0 to 4
//! cargo xtask live matrix --seats chief=claude,worker=devin,reviewer=codex,advisor=pi,tester=muse
//! cargo xtask live matrix --plan cells                                       the 50 cells, nothing run
//! cargo xtask live matrix --replay evals/reports/live-matrix/<run>/manifest.json
//! ```
//!
//! The evidence of a run is under `evals/reports/live-matrix/<run-id>/`.

use std::path::Path;

use cf_e2e::checkout;
use cf_e2e::live::report;
use cf_e2e::live::{self, Log};
use cf_e2e::matrix::evidence::{Owner, REPORTS};
use cf_e2e::matrix::run::{replay, run, Matrix, Outcome};
use cf_e2e::matrix::scenario::Scenario;
use cf_e2e::matrix::seating::{self, Role, Seating};
use cf_e2e::{Error, Result};

/// The stand-in agent's program: the stand-in seats' windows, and the one a rig has where no
/// window runs it.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The seats and the scenario the words ask for: `--board` (0 to 4, 0 where nothing names the
/// seats) or `--seats`, never both; `--scenario` (`smoke` where none is named); and `--stand-in`,
/// the members' positions the rig's stand-in plays where this machine cannot seat the harness
/// today, which the run says it covers none of. A harness named besides is refused.
fn asked(
    board: Option<&str>,
    seats: Option<&str>,
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
    let seating =
        match (board, seats) {
            (Some(_), Some(_)) => return Err(Error::Daemon(
                "--board names a row of the Latin square and --seats names the seats: one of them"
                    .to_owned(),
            )),
            (_, Some(seats)) => Seating::named(seats)?,
            (board, None) => {
                let number = board.unwrap_or("0");
                Seating::board(number.parse::<u8>().map_err(|_| {
                    Error::Daemon(format!("--board takes a board's number, not {number}"))
                })?)?
            }
        };
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
        seating.with_stand_ins(&roles)?,
        Scenario::named(scenario.unwrap_or("smoke"))?,
    ))
}

/// What a run says when it is over: its time and turns, its known reds, where its evidence is.
fn said(log: &Log, outcome: &Outcome, not_covered: &[&str]) {
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
    if !outcome.known.is_empty() {
        log.report(
            "KNOWN RED, by design on today's main, each until the work named lands",
            &report::table(&outcome.known),
        );
    }
}

#[test]
#[ignore = "opt-in: spends the quota of every harness the board seats; cargo xtask live matrix --board 0 --scenario smoke -- --test-threads 1"]
fn a_board_of_real_harnesses_goes_through_its_scenario_as_the_ledger_and_the_records_say() -> Result
{
    if live::option("plan").is_some() {
        let log = Log::begin("live matrix plan")?;
        log.block("the plan, nothing run", &seating::plan_text());
        return Ok(());
    }
    let log = Log::begin("live matrix")?;
    if let Some(manifest) = live::option("replay") {
        let outcome = replay(
            Path::new(&manifest),
            Path::new(FAKE_AGENT),
            checkout::path(REPORTS),
            Owner::of_machine()?,
            &log,
        )?;
        said(&log, &outcome, &[]);
        return report::conclude(
            &log,
            "what each check of the replay came to",
            &outcome.checks,
        );
    }
    let (seating, scenario) = asked(
        live::option("board").as_deref(),
        live::option("seats").as_deref(),
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
        replay_of: None,
    };
    let outcome = run(&matrix, &log)?;
    said(&log, &outcome, &not_covered);
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
    fn the_words_ask_for_a_board_or_seats_a_scenario_and_stand_ins_and_refuse_the_rest() {
        let (seating, scenario) = asked(None, None, None, None, &[]).unwrap();
        assert_eq!((seating.board, scenario.name), (Some(0), "smoke"));
        assert!(seating.not_covered().is_empty());
        let (seating, scenario) = asked(Some("4"), None, Some("core"), None, &[]).unwrap();
        assert_eq!((seating.board, scenario.name), (Some(4), "core"));
        assert_eq!(seating.seat(Role::Chief).who(), "muse");
        let (seating, _) = asked(
            None,
            Some("chief=claude,worker=devin,reviewer=codex,advisor=pi,tester=muse"),
            Some("field6-cancel-done"),
            Some("advisor"),
            &[],
        )
        .unwrap();
        assert_eq!(seating.not_covered(), [Role::Advisor]);
        let refused = |board, seats, scenario, stand_in, harnesses: &[String]| {
            asked(board, seats, scenario, stand_in, harnesses)
                .err()
                .map(|failed| failed.to_string())
                .unwrap_or_default()
        };
        assert!(refused(Some("5"), None, None, None, &[]).contains("boards 0 to 4"));
        assert!(refused(Some("one"), None, None, None, &[]).contains("not one"));
        assert!(refused(Some("0"), Some("chief=claude"), None, None, &[]).contains("one of them"));
        assert!(refused(
            None,
            Some("chief=claude,worker=codex,reviewer=pi,advisor=devin,tester=muse,designer=pi"),
            None,
            None,
            &[]
        )
        .contains("only Codex can be the image designer"));
        assert!(refused(None, None, Some("nothing"), None, &[])
            .contains("no scenario is called nothing"));
        assert!(refused(None, None, None, Some("chief"), &[]).contains("not the chief's"));
        assert!(refused(None, None, None, Some("judge"), &[]).contains("not judge"));
        assert!(refused(None, None, None, None, &["claude".to_owned()])
            .contains("takes none by name: claude"));
    }

    /// A test's reports folder: a fresh temporary one, kept where a person reads a real run's
    /// evidence (`evals/reports/live-matrix`, which CI uploads) when the test fails:
    /// each window's last screen, the records, the ledger, the events and the daemon's last lines
    /// of the run that went red. Two reds of the field6 pair on GitHub's Windows runner kept
    /// nothing of their windows (CI runs 41787972093 and 41792519144, T-1226).
    struct Reports(tempfile::TempDir);

    impl Reports {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl Drop for Reports {
        fn drop(&mut self) {
            if std::thread::panicking() {
                let kept = checkout::path(REPORTS);
                match copied_below(self.0.path(), &kept) {
                    Ok(names) => eprintln!(
                        "--- the failed case's evidence is kept in {}: {}",
                        kept.display(),
                        names.join(", ")
                    ),
                    Err(failed) => {
                        eprintln!("--- the failed case's evidence could not be kept: {failed}");
                    }
                }
            }
        }
    }

    /// Copies what `from` holds into `to`, folders and all: the names of what it held.
    fn copied_below(from: &Path, to: &Path) -> std::io::Result<Vec<String>> {
        std::fs::create_dir_all(to)?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copied_below(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), &target)?;
            }
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    /// A case that fails keeps its reports folder where CI uploads it from; one that passes
    /// keeps nothing there.
    #[test]
    fn a_failed_case_keeps_its_evidence_where_ci_uploads_it_and_a_passed_one_does_not() {
        let unique = format!("t1226-kept-{}", std::process::id());
        let kept = checkout::path(REPORTS).join(&unique);
        let write = |reports: &Reports, unique: &str| {
            let folder = reports.path().join(unique).join("windows");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("p1-worker.txt"), "the worker's last screen\n").unwrap();
        };
        {
            let reports = Reports::new();
            write(&reports, &unique);
        }
        assert!(
            !kept.exists(),
            "a passed case keeps nothing: {}",
            kept.display()
        );
        let failed = std::thread::spawn({
            let unique = unique.clone();
            move || {
                let reports = Reports::new();
                write(&reports, &unique);
                panic!("the case failed");
            }
        })
        .join();
        assert!(failed.is_err());
        let screen = std::fs::read_to_string(kept.join("windows").join("p1-worker.txt"));
        std::fs::remove_dir_all(&kept).ok();
        assert_eq!(screen.unwrap(), "the worker's last screen\n");
    }

    /// `scenario` on a board whose five positions the rig's stand-in plays, its evidence in
    /// `reports`.
    fn on_stand_ins(scenario: Scenario, reports: &Path) -> Outcome {
        let log = Log::new(&format!("live matrix on stand-ins: {}", scenario.name));
        let matrix = Matrix {
            seating: Seating::stand_ins(),
            scenario,
            stand_in: Path::new(FAKE_AGENT),
            // `--keep` (CONSENSFLOW_LIVE_KEEP) keeps a stand-in run's evidence where a real
            // run's goes, for a person to read.
            reports: if live::keep_requested() {
                checkout::path(REPORTS)
            } else {
                reports.to_path_buf()
            },
            owner: Owner::look(&[], &[]),
            replay_of: None,
        };
        let outcome = run(&matrix, &log).unwrap();
        log.report("what each check came to", &report::table(&outcome.checks));
        if !outcome.known.is_empty() {
            log.report("known red", &report::table(&outcome.known));
        }
        outcome
    }

    /// The check of `outcome` whose `who` is `who` and whose `what` holds `what`.
    fn check(outcome: &Outcome, who: &str, what: &str) -> report::Check {
        outcome
            .checks
            .iter()
            .chain(&outcome.known)
            .find(|check| check.who == who && check.what.contains(what))
            .unwrap_or_else(|| {
                panic!(
                    "no check of {who} {what}:\n{}",
                    report::table(&outcome.checks)
                )
            })
            .clone()
    }

    /// The control: the smoke on a board whose five positions the rig's stand-in plays. The
    /// stand-in chief hands out the advice, the change, the review and the test and accepts them;
    /// the oracle reads the ledger and the records, and every check holds. No harness is covered.
    #[test]
    fn the_matrix_passes_its_scenario_on_the_rigs_stand_ins() {
        let reports = Reports::new();
        let outcome = on_stand_ins(Scenario::named("smoke").unwrap(), reports.path());
        let table = report::table(&outcome.checks);
        assert!(
            outcome.failed().is_empty() && outcome.known.is_empty(),
            "{table}"
        );
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
            "evidence",
            "owner",
        ] {
            assert!(
                outcome.checks.iter().any(|check| check.who == who),
                "no check of {who}:\n{table}"
            );
        }
        assert!(outcome.total_turns <= 20, "{:?}", outcome.turns);
        let folder = &outcome.evidence.folder;
        for file in [
            "manifest.json",
            "human.json",
            "bindings.json",
            "events.jsonl",
            "checks.json",
            "verdict.txt",
            "opens.json",
            "daemon-tail.log",
            "ledger.db",
            "records/chief.json",
        ] {
            assert!(
                folder.join(file).is_file(),
                "no {file} in {}",
                folder.display()
            );
        }
        let verdict = std::fs::read_to_string(folder.join("verdict.txt")).unwrap();
        assert!(verdict.starts_with("GREEN"), "{verdict}");
    }

    /// The control of the run's end and of the product's refusals, and the planted red the
    /// replay is proved on: the stand-in chief puts the advice task on the board, cancels it, and
    /// asks to send it back. The run ends at once, as a red that names the cancelled task, and
    /// the refusal's code is the daemon's (`api.answered`), in a line that holds no token.
    #[test]
    fn a_cancelled_task_ends_the_run_at_once_and_the_products_refusal_is_read_by_its_code() {
        let reports = Reports::new();
        let mut scenario = Scenario::named("cancel").unwrap();
        // A run that went on past the cancelled task would end at this deadline, red for
        // another reason, and soon.
        scenario.deadline = std::time::Duration::from_secs(90);
        let outcome = on_stand_ins(scenario, reports.path());
        let table = report::table(&outcome.checks);
        let step = check(&outcome, "steps", "step 1 (advice) was reached");
        assert!(!step.ok, "{table}");
        assert_eq!(step.detail, "T-1, the advice task, is cancelled", "{table}");
        assert!(
            outcome.elapsed.as_secs() < 60,
            "ended at once: {} s",
            outcome.elapsed.as_secs()
        );
        let refused = check(
            &outcome,
            "advice",
            "the chief's first reopen of it was refused with invalid-transition",
        );
        assert!(refused.ok, "{table}");
        assert!(check(&outcome, "advice", "it is cancelled").ok, "{table}");
        // The evidence holds each window's input as the pane host held it at the end, beside its
        // screen: for the chief's, a paste that went in and nothing of the human's left unsent.
        let windows = outcome.evidence.folder.join("windows");
        let chief = std::fs::read_dir(&windows)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("p1-chief-") && name.ends_with(".input.json")
                })
            })
            .unwrap_or_else(|| panic!("no input of the chief's in {}", windows.display()));
        let input: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(chief).unwrap()).unwrap();
        assert_eq!(
            (&input["ok"], &input["unsent"], &input["pasteInFlight"]),
            (
                &serde_json::json!(true),
                &serde_json::json!(false),
                &serde_json::json!(false)
            ),
            "{input:#}"
        );
        assert!(
            input["humanWrites"]
                .as_array()
                .is_some_and(|writes| !writes.is_empty()),
            "the human's ask is among its writes: {input:#}"
        );
        // And what the stand-ins read of their terminals: the chief read the human's paste.
        let traces: Vec<String> = std::fs::read_dir(outcome.evidence.folder.join("stand-ins"))
            .unwrap()
            .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path()).ok())
            .collect();
        assert!(
            traces.iter().any(|trace| trace
                .lines()
                .any(|line| line.contains("\"pasteStart\":true"))),
            "{traces:#?}"
        );
        let events = std::fs::read_to_string(outcome.evidence.folder.join("events.jsonl")).unwrap();
        let answered: Vec<&str> = events
            .lines()
            .filter(|line| line.contains("\"api.answered\""))
            .collect();
        assert!(answered.len() >= 3, "{answered:?}");
        assert!(
            answered.iter().all(|line| !line.contains("Bearer")),
            "{answered:?}"
        );
        assert!(
            check(&outcome, "evidence", "holds a window's token").ok,
            "{table}"
        );
    }

    /// The same control where the stand-in chief is slow between its commands, as a loaded
    /// runner makes it: the run saw the task cancelled before the chief asked to send it back,
    /// ended, and read no refusal (GitHub's macOS runner, CI run 41798142126). The run waits for
    /// the call the scenario reads, and still ends long before its deadline.
    #[test]
    fn a_cancelled_task_whose_chief_asks_to_send_it_back_late_is_read_for_the_refusal_before_the_run_ends(
    ) {
        let reports = Reports::new();
        let mut scenario = Scenario::named("cancel")
            .unwrap()
            .with_vars(&[("CF_TEST_CF_GAP_MS", "5000")], &[]);
        scenario.deadline = std::time::Duration::from_secs(90);
        let outcome = on_stand_ins(scenario, reports.path());
        let table = report::table(&outcome.checks);
        let step = check(&outcome, "steps", "step 1 (advice) was reached");
        assert!(!step.ok, "{table}");
        assert_eq!(step.detail, "T-1, the advice task, is cancelled", "{table}");
        let refused = check(
            &outcome,
            "advice",
            "the chief's first reopen of it was refused with invalid-transition",
        );
        assert!(refused.ok, "{table}");
        assert!(
            outcome.elapsed.as_secs() < 60,
            "ended once the call was answered: {} s",
            outcome.elapsed.as_secs()
        );
    }

    /// A chief that cancels the task and never asks to send it back: the run ends once its turn
    /// does, and the refusal the scenario reads is red, saying no call came.
    #[test]
    fn a_cancelled_task_whose_chief_never_asks_to_send_it_back_ends_the_run_with_its_turn_red_for_the_call(
    ) {
        let reports = Reports::new();
        let mut scenario = Scenario::named("cancel").unwrap();
        scenario.deadline = std::time::Duration::from_secs(90);
        scenario.steps[0].stand_in = |_| {
            "CF task add --advice --tier standard :: SLEEP 60 Reply with exactly: ADVICE_OK\nCF task cancel T-1"
                .to_owned()
        };
        let outcome = on_stand_ins(scenario, reports.path());
        let table = report::table(&outcome.checks);
        let step = check(&outcome, "steps", "step 1 (advice) was reached");
        assert_eq!(step.detail, "T-1, the advice task, is cancelled", "{table}");
        let refused = check(
            &outcome,
            "advice",
            "the chief's first reopen of it was refused with invalid-transition",
        );
        assert!(!refused.ok, "{table}");
        assert_eq!(
            refused.detail, "the chief made 0 call(s) of POST /api/tasks/1/reopen",
            "{table}"
        );
        assert!(
            outcome.elapsed.as_secs() < 60,
            "ended with the chief's turn: {} s",
            outcome.elapsed.as_secs()
        );
    }

    /// LMX-07: a red's manifest runs again alone, in a fresh project with the same seats, its
    /// tasks bound again by what they are; the red's own folder is left as it was.
    #[test]
    fn a_red_runs_again_alone_from_its_manifest_with_the_same_seats() {
        let reports = Reports::new();
        let mut scenario = Scenario::named("cancel").unwrap();
        scenario.deadline = std::time::Duration::from_secs(90);
        let red = on_stand_ins(scenario, reports.path());
        assert!(!red.failed().is_empty());
        let manifest = red.evidence.folder.join("manifest.json");
        let before = folder_text(&red.evidence.folder);
        let log = Log::new("live matrix replay");
        let again = replay(
            &manifest,
            Path::new(FAKE_AGENT),
            reports.path().to_path_buf(),
            Owner::look(&[], &[]),
            &log,
        )
        .unwrap();
        assert_ne!(again.evidence.folder, red.evidence.folder);
        assert_eq!(
            folder_text(&red.evidence.folder),
            before,
            "the red's folder is as it was"
        );
        let read = |folder: &Path| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(folder.join("manifest.json")).unwrap())
                .unwrap()
        };
        let (first, second) = (read(&red.evidence.folder), read(&again.evidence.folder));
        assert_eq!(second["replayOf"], first["run"]);
        assert_eq!(second["seating"], first["seating"]);
        assert_eq!(second["scenario"]["name"], "cancel");
        assert_eq!(second["scenario"]["version"], first["scenario"]["version"]);
        // The same red, alone: the one scenario, its one step, ended by the same task.
        assert_eq!(
            check(&again, "steps", "step 1 (advice)").detail,
            "T-1, the advice task, is cancelled"
        );
        let reds = |outcome: &Outcome| -> Vec<(String, String)> {
            outcome
                .failed()
                .iter()
                .map(|check| (check.who.clone(), check.what.clone()))
                .collect()
        };
        assert_eq!(reds(&again), reds(&red));
        // A manifest of another version of the scenario is refused, in words.
        let mut other = first.clone();
        other["scenario"]["version"] = serde_json::json!(99);
        let file = reports.path().join("other.json");
        std::fs::write(&file, other.to_string()).unwrap();
        let refused = replay(
            &file,
            Path::new(FAKE_AGENT),
            reports.path().to_path_buf(),
            Owner::look(&[], &[]),
            &log,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(refused.contains("ran cancel version 99"), "{refused}");
    }

    /// Every file of `folder` and what it holds, in order.
    fn folder_text(folder: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = checkout::files_below(folder, &[])
            .unwrap()
            .into_iter()
            .map(|file| (file.display().to_string(), std::fs::read(&file).unwrap()))
            .collect();
        files.sort();
        files
    }

    /// The control of the chief's question in words: the stand-in chief ends its turn asking,
    /// which the human reads in its own record, answers from the fact sheet through the human's
    /// door, and sees go in. Nothing reaches the milestone, so the run ends red at its deadline.
    #[test]
    fn a_chiefs_question_in_its_record_is_answered_through_the_humans_door_and_goes_in() {
        let reports = Reports::new();
        let mut scenario = Scenario::named("smoke").unwrap();
        scenario.steps.truncate(1);
        scenario.steps[0].stand_in = |_| "Reply with exactly: Which-one?".to_owned();
        scenario.deadline = std::time::Duration::from_secs(60);
        let outcome = on_stand_ins(scenario, reports.path());
        let table = report::table(&outcome.checks);
        let human: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(outcome.evidence.folder.join("human.json")).unwrap(),
        )
        .unwrap();
        let answers: Vec<&serde_json::Value> = human
            .as_array()
            .unwrap()
            .iter()
            .filter(|said| said["step"] == "answer")
            .collect();
        assert_eq!(answers.len(), 1, "{human:#}");
        assert!(
            answers[0]["text"]
                .as_str()
                .unwrap()
                .contains("Everything I can tell you:"),
            "{human:#}"
        );
        assert!(
            answers[0]["inRecordAt"].is_string() && answers[0]["failed"].is_null(),
            "{human:#}"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(outcome.evidence.folder.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["assisted"]["answers"], 1, "{manifest:#}");
        let step = check(&outcome, "steps", "step 1");
        assert!(
            !step.ok && step.detail.contains("deadline of 60 s"),
            "{table}"
        );
    }

    /// LMX-08, the core flow, on the stand-ins: every check holds.
    #[test]
    fn the_core_flow_holds_on_the_stand_ins() {
        let reports = Reports::new();
        let outcome = on_stand_ins(Scenario::named("core").unwrap(), reports.path());
        let table = report::table(&outcome.checks);
        assert!(
            outcome.failed().is_empty() && outcome.known.is_empty(),
            "{table}"
        );
        assert!(
            check(
                &outcome,
                "A",
                "the chief's first reopen of it was refused with invalid-transition"
            )
            .ok
        );
        assert!(check(&outcome, "B", "it has two results").ok);
    }

    /// A subcase of field report 6 on the stand-ins: no red but its known ones, and each known
    /// one still red (none passes unseen: a known red that holds is a red of its own, until its
    /// mark goes when I-7 lands).
    fn holds_but_its_known_reds(name: &str) {
        let reports = Reports::new();
        let scenario = Scenario::named(name).unwrap();
        let marks = scenario.known.len();
        let outcome = on_stand_ins(scenario, reports.path());
        let table = report::table(&outcome.checks);
        assert!(
            outcome.failed().is_empty(),
            "{name}:\n{table}\nknown:\n{}",
            report::table(&outcome.known)
        );
        assert_eq!(
            outcome.known.len(),
            marks,
            "{name}: each known red is still red"
        );
        assert!(outcome
            .known
            .iter()
            .all(|known| !known.ok && known.detail.starts_with("known red until T-11")));
    }

    #[test]
    fn field6_done_predecessor() {
        holds_but_its_known_reds("field6-done-predecessor");
    }

    #[test]
    fn field6_acceptance_releases() {
        holds_but_its_known_reds("field6-acceptance-releases");
    }

    #[test]
    fn field6_cancel_done() {
        holds_but_its_known_reds("field6-cancel-done");
    }

    #[test]
    fn field6_cancel_with_waiter() {
        holds_but_its_known_reds("field6-cancel-with-waiter");
    }

    #[test]
    fn field6_accepted_bypass() {
        holds_but_its_known_reds("field6-accepted-bypass");
    }

    #[test]
    fn order_after_working() {
        holds_but_its_known_reds("order-after-working");
    }

    #[test]
    fn field2_needs_repeated() {
        holds_but_its_known_reds("field2-needs-repeated");
    }

    #[test]
    fn order_before() {
        holds_but_its_known_reds("order-before");
    }

    #[test]
    fn order_self_needs() {
        holds_but_its_known_reds("order-self-needs");
    }

    #[test]
    fn pause_resume_mid_turn() {
        holds_but_its_known_reds("pause-resume-mid-turn");
    }

    #[test]
    fn pause_resume_mid_tool() {
        holds_but_its_known_reds("pause-resume-mid-tool");
    }

    #[test]
    fn field8_tell_resume() {
        holds_but_its_known_reds("field8-tell-resume");
    }

    #[test]
    fn cancel_each_state() {
        holds_but_its_known_reds("cancel-each-state");
    }

    #[test]
    fn edit_open() {
        holds_but_its_known_reds("edit-open");
    }

    #[test]
    fn finish_each_state() {
        holds_but_its_known_reds("finish-each-state");
    }

    #[test]
    fn question_ask_answer() {
        holds_but_its_known_reds("question-ask-answer");
    }

    #[test]
    fn question_in_last_words() {
        holds_but_its_known_reds("question-in-last-words");
    }

    #[test]
    fn question_advisor_answer() {
        holds_but_its_known_reds("question-advisor-answer");
    }

    #[test]
    fn field1_tier_fallback() {
        holds_but_its_known_reds("field1-tier-fallback");
    }

    #[test]
    fn field3_accept_failed() {
        holds_but_its_known_reds("field3-accept-failed");
    }

    #[test]
    fn field4_busy_model() {
        holds_but_its_known_reds("field4-busy-model");
    }

    #[test]
    fn field5_notices() {
        holds_but_its_known_reds("field5-notices");
    }

    #[test]
    fn field7_never_began() {
        holds_but_its_known_reds("field7-never-began");
    }

    #[test]
    fn pause_keeps_work() {
        holds_but_its_known_reds("pause-keeps-work");
    }

    #[test]
    fn field_stale_transcript() {
        holds_but_its_known_reds("field-stale-transcript");
    }

    #[test]
    fn field_retitle() {
        holds_but_its_known_reds("field-retitle");
    }

    #[test]
    fn field_draft_kept() {
        holds_but_its_known_reds("field-draft-kept");
    }

    #[test]
    fn field_retyped_under_load() {
        holds_but_its_known_reds("field-retyped-under-load");
    }

    #[test]
    fn field_late_record() {
        holds_but_its_known_reds("field-late-record");
    }

    #[test]
    fn field_recap_not_typed() {
        holds_but_its_known_reds("field-recap-not-typed");
    }

    #[test]
    fn field_long_paste() {
        holds_but_its_known_reds("field-long-paste");
    }

    #[test]
    fn field_image_result() {
        holds_but_its_known_reds("field-image-result");
    }

    #[test]
    fn field_question_not_retyped() {
        holds_but_its_known_reds("field-question-not-retyped");
    }

    /// Field report 5's nudges (T-1113): five minutes of wake-up calls a minute apart, too long
    /// for CI's step, run on request:
    /// `cargo test -p cf-e2e --test live_matrix -- --ignored field5_nudges`.
    #[test]
    #[ignore = "on request: five minutes of wake-up calls; cargo test -p cf-e2e --test live_matrix -- --ignored field5_nudges"]
    fn field5_nudges() {
        holds_but_its_known_reds("field5-nudges");
    }
}
