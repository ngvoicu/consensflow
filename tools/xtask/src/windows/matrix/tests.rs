//! The matrix, tried on a machine that answers what the case tells it to and keeps
//! what it is asked: nothing here reaches one, or runs an eval.

use std::collections::VecDeque;

use cf_base::time::parse;

use super::*;
use crate::process::Invocation;
use crate::windows::testing::strings;

/// The time the matrix is begun at, as the folder of its logs names it.
const BEGUN: &str = "2026-10-12T10:11:12.345Z";
const FOLDER: &str = "2026-10-12T10-11-12";

struct Fixed;

impl Clock for Fixed {
    fn now_ms(&mut self) -> i64 {
        parse(BEGUN).unwrap()
    }
}

/// What a run says, and how it ends.
struct Answer {
    out: &'static str,
    err: &'static str,
    ended: Result<Outcome, Error>,
}

fn says(out: &'static str, status: i32) -> Answer {
    Answer {
        out,
        err: "",
        ended: Ok(Outcome::Exited(status)),
    }
}

/// A machine that answers each run with the next of its answers, and keeps what it was asked.
struct Scripted {
    asked: Vec<Options>,
    answers: VecDeque<Answer>,
}

impl Scripted {
    fn new(answers: Vec<Answer>) -> Self {
        Self {
            asked: Vec::new(),
            answers: answers.into(),
        }
    }
}

impl Remote for Scripted {
    fn run(&mut self, options: &Options, console: &mut Console) -> Result<Outcome, Error> {
        self.asked.push(options.clone());
        let answer = self.answers.pop_front().expect("a run nobody answered");
        console.out.write_all(answer.out.as_bytes()).unwrap();
        console.err.write_all(answer.err.as_bytes()).unwrap();
        answer.ended
    }
}

fn pair(chief: &str, staff: &str) -> Pair {
    Pair {
        chief: chief.into(),
        staff: staff.into(),
    }
}

fn matrix(scenarios: &[&str], pairs: Vec<Pair>) -> MatrixOptions {
    MatrixOptions {
        machine: Some(Machine::at("zeewin")),
        build: false,
        env: vec![],
        scenarios: strings(scenarios),
        pairs,
        claude_model: None,
        pi_model: None,
    }
}

/// What a matrix of `options` says and answers on `machine`, in a checkout of its own.
fn go(options: &MatrixOptions, machine: &mut Scripted) -> (i32, String, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let context = Context {
        root: root.path().to_path_buf(),
        env: cf_base::env::Env::default(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = run(
        &context,
        options,
        machine,
        &mut Fixed,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    )
    .unwrap();
    assert!(err.is_empty());
    (status, String::from_utf8(out).unwrap(), root)
}

/// The words a run on the machine was asked to run, joined.
fn eval_of(options: &Options) -> String {
    options.command.join(" ")
}

const PASSED: &str =
    "daemon: native (rust 3.0.0)\n  PASS the task was done\n  PASS the review came\n";

#[test]
fn each_scenario_for_each_pair_is_run_one_after_another_as_the_eval_on_the_machine() {
    let mut options = matrix(
        &["round-trip", "question-trip"],
        vec![pair("devin", "devin"), pair("claude", "devin,pi")],
    );
    options.env = vec![("CONSENSFLOW_TEST_DAEMON".into(), "native".into())];
    options.claude_model = Some("claude-sonnet-5".into());
    options.pi_model = Some("openai/gpt-5.6-luna".into());
    let mut machine = Scripted::new((0..4).map(|_| says(PASSED, 0)).collect());
    let (status, _, _) = go(&options, &mut machine);
    assert_eq!(status, 0);
    let asked: Vec<String> = machine.asked.iter().map(eval_of).collect();
    assert_eq!(
        asked,
        [
            "cargo xtask eval --scenario round-trip --chief devin --staff devin --timeout-min 30",
            "cargo xtask eval --scenario round-trip --chief claude --staff devin,pi --timeout-min 30 --model claude-sonnet-5 --pi-staff-model openai/gpt-5.6-luna",
            "cargo xtask eval --scenario question-trip --chief devin --staff devin --timeout-min 30",
            "cargo xtask eval --scenario question-trip --chief claude --staff devin,pi --timeout-min 30 --model claude-sonnet-5 --pi-staff-model openai/gpt-5.6-luna",
        ]
    );
    // Each run is the machine's own: its host, the variables named, the usual folder.
    for run in &machine.asked {
        assert_eq!(run.machine, Some(Machine::at("zeewin")));
        assert_eq!(
            run.env,
            [("CONSENSFLOW_TEST_DAEMON".to_owned(), "native".to_owned())]
        );
        assert_eq!((run.dir.as_str(), run.inside.as_str()), (DEFAULT_DIR, "."));
    }
}

#[test]
fn a_model_goes_only_to_the_chief_or_the_staff_it_is_the_models_of() {
    let mut options = matrix(
        &["round-trip"],
        vec![pair("devin", "claude"), pair("claude", "devin")],
    );
    options.claude_model = Some("claude-sonnet-5".into());
    options.pi_model = Some("openai/gpt-5.6-luna".into());
    let mut machine = Scripted::new(vec![says(PASSED, 0), says(PASSED, 0)]);
    go(&options, &mut machine);
    for run in &machine.asked {
        assert!(
            !eval_of(run).contains("--pi-staff-model"),
            "{}",
            eval_of(run)
        );
    }
    // A Claude chief's model; a staff that is Claude has the eval's own.
    assert!(!eval_of(&machine.asked[0]).contains("--model"));
    assert!(eval_of(&machine.asked[1]).contains("--model claude-sonnet-5"));
}

#[test]
fn the_machine_is_built_once_before_the_first_run_that_goes_and_not_a_skipped_one() {
    let mut options = matrix(
        &["chief-switch"],
        vec![pair("devin", "devin"), pair("claude", "devin")],
    );
    options.build = true;
    options.scenarios.push("round-trip".into());
    let mut machine = Scripted::new(vec![says(PASSED, 0), says(PASSED, 0), says(PASSED, 0)]);
    go(&options, &mut machine);
    let builds: Vec<bool> = machine.asked.iter().map(|run| run.build).collect();
    // devin:devin of a chief switch is skipped, so the first run that goes builds.
    assert_eq!(builds, [true, false, false]);
}

#[test]
fn a_chief_switch_goes_to_the_staffs_harness_and_is_skipped_where_chief_and_staff_are_one() {
    let options = matrix(
        &["chief-switch"],
        vec![pair("devin", "devin"), pair("claude", "devin")],
    );
    let mut machine = Scripted::new(vec![says(PASSED, 0)]);
    let (status, out, _) = go(&options, &mut machine);
    assert_eq!(status, 0, "a skipped run is not a failed one");
    assert_eq!(machine.asked.len(), 1);
    assert_eq!(
        eval_of(&machine.asked[0]),
        "cargo xtask eval --scenario chief-switch --chief claude --staff devin --timeout-min 30 --switch-to devin"
    );
    let skipped = format!(
        "{:<36} skipped: a chief switch needs another harness on the staff\n",
        "chief-switch devin:devin"
    );
    assert!(out.contains(&skipped), "{out}");
    // A skipped run is not announced: nothing ran.
    assert!(!out.contains("chief-switch devin:devin…"), "{out}");
}

#[test]
fn what_each_run_comes_to_is_a_line_and_the_status_is_1_when_any_did_not_pass() {
    let options = matrix(
        &["round-trip", "question-trip", "long-trip"],
        vec![pair("devin", "devin")],
    );
    let mut machine = Scripted::new(vec![
        says(PASSED, 0),
        says(
            "  PASS ran\n  FAIL the question reached the owner\n  FAIL no stray window\n",
            1,
        ),
        says("it broke before it began\n", 3),
    ]);
    let (status, out, root) = go(&options, &mut machine);
    assert_eq!(status, 1);
    let folder = root
        .path()
        .join("evals")
        .join("reports")
        .join("windows")
        .join(FOLDER);
    assert_eq!(
        out,
        format!(
            "round-trip devin:devin…\n  PASS 2 checks · daemon native (rust 3.0.0)\n\
             question-trip devin:devin…\n  FAIL the question reached the owner · no stray window\n\
             long-trip devin:devin…\n  no verdict (exit 3): see {log}\n\
             \nlogs: {folder}\n\
             {:<36} PASS 2 checks · daemon native (rust 3.0.0)\n\
             {:<36} FAIL the question reached the owner · no stray window\n\
             {:<36} no verdict (exit 3): see {log}\n",
            "round-trip devin:devin",
            "question-trip devin:devin",
            "long-trip devin:devin",
            folder = folder.display(),
            log = folder.join("long-trip-devin-devin.log").display(),
        )
    );
}

#[test]
fn a_matrix_in_which_every_run_passed_or_was_skipped_answers_0() {
    let options = matrix(
        &["chief-switch", "round-trip"],
        vec![pair("devin", "devin")],
    );
    let mut machine = Scripted::new(vec![says(PASSED, 0)]);
    assert_eq!(go(&options, &mut machine).0, 0);
}

#[test]
fn each_run_keeps_what_it_said_on_both_streams_in_the_order_it_said_it_in_a_folder_named_by_the_time(
) {
    struct Interleaved;
    impl Remote for Interleaved {
        fn run(&mut self, _: &Options, console: &mut Console) -> Result<Outcome, Error> {
            console.out.write_all(b"one\n").unwrap();
            console.err.write_all(b"two\n").unwrap();
            console.out.write_all(b"  PASS three\n").unwrap();
            Ok(Outcome::Exited(0))
        }
    }
    let options = matrix(&["round-trip"], vec![pair("claude", "devin,pi")]);
    let root = tempfile::tempdir().unwrap();
    let context = Context {
        root: root.path().to_path_buf(),
        env: cf_base::env::Env::default(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    run(
        &context,
        &options,
        &mut Interleaved,
        &mut Fixed,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    )
    .unwrap();
    let log = root
        .path()
        .join("evals/reports/windows")
        .join(FOLDER)
        .join("round-trip-claude-devin,pi.log");
    assert_eq!(fs::read_to_string(log).unwrap(), "one\ntwo\n  PASS three\n");
    // What the run said is the log's, and not the terminal's: the matrix says only what each came to.
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("one") && !out.contains("two"), "{out}");
}

#[test]
fn a_run_that_could_not_start_is_a_line_of_no_verdict_with_its_reason_in_its_log() {
    let options = matrix(&["round-trip"], vec![pair("devin", "devin")]);
    let mut machine = Scripted::new(vec![Answer {
        out: "",
        err: "",
        ended: Err(Error::step(
            &Invocation::new("scp", "."),
            255,
            "Connection refused",
        )),
    }]);
    let (status, out, root) = go(&options, &mut machine);
    assert_eq!(status, 1);
    assert!(out.contains("  no verdict (exit 1): see "), "{out}");
    let log = root
        .path()
        .join("evals/reports/windows")
        .join(FOLDER)
        .join("round-trip-devin-devin.log");
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "windows: `scp` ended with status 255: Connection refused\n"
    );
}

#[test]
fn a_matrix_that_is_stopped_ends_there_with_130_and_no_summary() {
    let options = matrix(
        &["round-trip", "question-trip"],
        vec![pair("devin", "devin")],
    );
    let mut machine = Scripted::new(vec![Answer {
        out: "half of it\n",
        err: "",
        ended: Ok(Outcome::Stopped),
    }]);
    let (status, out, _) = go(&options, &mut machine);
    assert_eq!(status, 130);
    assert_eq!(machine.asked.len(), 1, "no run after a stop");
    assert_eq!(out, "round-trip devin:devin…\n");
}

#[test]
fn what_an_eval_says_is_read_for_its_checks_and_the_daemon_it_ran_against() {
    let log = Path::new("run.log");
    let said = |text: &str, status: i32| {
        let verdict = verdict(text, status, log);
        (verdict.ok, verdict.line)
    };
    assert_eq!(said("PASS a\n PASS b\n", 0), (true, "PASS 2 checks".into()));
    // Whatever indent a check has; a line that only holds the word is not one.
    assert_eq!(
        said(
            "   FAIL one thing\nsomething FAIL not one\nFAILED neither\n\t FAIL two things\n",
            1
        ),
        (false, "FAIL one thing · two things".into())
    );
    // The first daemon it says, a line of its own.
    assert_eq!(
        said(" daemon: not at the line's start\ndaemon: native (rust 3.0.0)\ndaemon: second\nPASS a\n", 0),
        (true, "PASS 1 checks · daemon native (rust 3.0.0)".into())
    );
    assert_eq!(
        said("daemon: native (rust 3.0.0)\n", 7),
        (
            false,
            "no verdict (exit 7): see run.log · daemon native (rust 3.0.0)".into()
        )
    );
    // A line of a Windows machine ends as it does there.
    assert_eq!(said("PASS a\r\nFAIL b\r\n", 1), (false, "FAIL b".into()));
    assert_eq!(
        said("", 0),
        (false, "no verdict (exit 0): see run.log".into())
    );
    // A check that says nothing counts, and is no thing that failed.
    assert_eq!(said("PASS a\nFAIL \n", 0), (true, "PASS 2 checks".into()));
}

#[test]
fn the_logs_are_kept_in_a_folder_named_by_the_time_with_what_the_time_is_made_of() {
    let root = Path::new("checkout");
    assert_eq!(
        logs_in(root, &mut Fixed),
        root.join("evals")
            .join("reports")
            .join("windows")
            .join("2026-10-12T10-11-12")
    );
}
