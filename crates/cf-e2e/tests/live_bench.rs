//! The live bench (VERIFY-BDC-08): the daemon, the real pane host and the real
//! harness TUIs on cheap models, driven by code. It is opt-in (`cargo xtask live
//! bench`, `npm run bench:core`): it spends the machine's quota on a chief and a
//! worker of each harness named, and needs each one installed and logged in, so it
//! is ignored unless asked for and is no part of `check` or of CI.
//!
//! The human gives the chief one task per worker: run `cf task add` for its tier;
//! the daemon picks the worker (the free one of that tier with the fewest tasks so
//! far, the earliest joined first), which the steps below lean on. The chief (OpenCode
//! on the free Muse Spark model by default; `--chief claude` for a Claude Code chief
//! on Sonnet) must run it itself; the daemon opens the worker's window with the task,
//! the worker must answer in full-permission mode, the core must record the answer as
//! the task's result and deliver it into the chief's window, and the worker must read
//! idle. Then the app restarts, the project waits suspended, and the human's Resume
//! must bring it back on its chief's own conversation. A question's answer must also
//! have been received at its harness's door (`cargo xtask live door`): claimed,
//! acknowledged, and never pasted.
//!
//! State lives in a throwaway home; the harnesses use the real logins.
//!
//! ```text
//! cargo xtask live bench                                         the baseline, the questions, a review, the restart: OpenCode, Pi and Devin
//! cargo xtask live bench opencode pi devin claude codex          every worker
//! cargo xtask live bench --chief claude                          a Claude Code chief
//! cargo xtask live bench --steps questions codex claude          the question door's alone, with no delivery baseline, review or restart
//! cargo xtask live bench --reviewer pi                           another harness reviews (Devin does by default)
//! ```
//!
//! Each step is a check, said as it is recorded; the run fails when any did not hold.

// The parts are in `live_bench/`: cargo looks for the modules of a test's root
// file beside it, where each would be a suite of its own.
#[path = "live_bench/baseline.rs"]
mod baseline;
#[path = "live_bench/bench.rs"]
mod bench;
#[path = "live_bench/question.rs"]
mod question;
#[path = "live_bench/review.rs"]
mod review;

use std::path::Path;

use cf_e2e::live::agents;
use cf_e2e::live::members::{member_as, Members, Plan, BENCH_BRIEF};
use cf_e2e::live::report::{self};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::json;

use baseline::{agent_id, Worker};
use bench::Bench;

/// The stand-in for the chief's window: no window of the bench runs it (its chief is a
/// real harness), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The workers when none is named.
const DEFAULT: [Harness; 3] = [Harness::OpenCode, Harness::Pi, Harness::Devin];

/// The id of the chief's agent in the roster.
const CHIEF: &str = "bench-chief";

/// What the command line asked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    chief: Harness,
    reviewer: Harness,
    /// Every step (`--steps all`), or the question door's alone (`--steps questions`).
    all: bool,
    workers: Vec<Harness>,
}

/// The chief, the reviewer, the steps and the workers the words ask for.
fn options(
    chief: Option<&str>,
    reviewer: Option<&str>,
    steps: Option<&str>,
    workers: &[String],
) -> Result<Options> {
    let harness = |word: &str, what: &str| {
        Harness::from_name(word)
            .ok_or_else(|| Error::Daemon(format!("unsupported bench {what}: {word}")))
    };
    let chief = harness(chief.unwrap_or("opencode"), "chief")?;
    if !matches!(chief, Harness::Claude | Harness::OpenCode) {
        return Err(Error::Daemon(format!(
            "unsupported bench chief: {}",
            chief.name()
        )));
    }
    let all = match steps.unwrap_or("all") {
        "all" => true,
        "questions" => false,
        other => {
            return Err(Error::Daemon(format!("unsupported bench steps: {other}")));
        }
    };
    let workers = if workers.is_empty() {
        DEFAULT.to_vec()
    } else {
        workers
            .iter()
            .map(|word| harness(word, "worker"))
            .collect::<Result<_>>()?
    };
    Ok(Options {
        chief,
        reviewer: harness(reviewer.unwrap_or("devin"), "reviewer")?,
        all,
        workers,
    })
}

/// What the bench opens: the folder, the chief on its own agent (its harness's cheap
/// model), a worker of each harness named and the reviewer.
fn plan_of(options: &Options) -> Plan {
    let mut plan = Plan::new("bench", member_as(options.chief, CHIEF)).told(BENCH_BRIEF);
    for harness in &options.workers {
        plan = plan.with(member_as(*harness, &agent_id(*harness)));
    }
    plan.with(member_as(options.reviewer, review::REVIEWER))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief and a worker of each harness named; cargo xtask live bench (npm run bench:core)"]
fn the_core_delivers_between_a_real_chief_and_real_workers_and_comes_back_after_the_apps_quit(
) -> Result {
    let log = Log::new("live bench");
    let options = options(
        live::option("chief").as_deref(),
        live::option("reviewer").as_deref(),
        live::option("steps").as_deref(),
        &live::list("harness"),
    )?;
    let mut members = Members::start(&plan_of(&options), Path::new(FAKE_AGENT), &log)?;
    // Each task names its worker's tier, and each bench worker has its own.
    let mut workers = Vec::new();
    for harness in &options.workers {
        let worker = Worker {
            harness: *harness,
            tier: String::new(),
        };
        let tier = members.add(&worker.agent(), &[])?;
        workers.push(Worker { tier, ..worker });
    }

    let mut bench = Bench::new(&members, &log);
    let ready = members.chief_is_idle(&log)?;
    let activity = bench.chief_activity()?;
    bench.record(
        "chief-ready",
        ready,
        &json!({ "chief": options.chief.name(), "activity": activity }),
    );
    if options.all {
        for worker in &workers {
            baseline::run(&mut bench, worker)?;
        }
    }
    // The question door, live: a worker asks through its harness's own question tool.
    for worker in workers
        .iter()
        .filter(|worker| agents::question_tool(worker.harness).is_some())
    {
        question::run(&mut bench, worker)?;
    }
    // A review, live: nothing is reviewed unless the chief asks.
    if options.all {
        review::review(&mut bench, options.reviewer)?;
    }

    // The restart: a new daemon and pane host over the same home, in the app's quit
    // order. The project waits suspended until the human resumes it, and must come
    // back on its chief's own conversation.
    let mut checks = bench.into_checks();
    if options.all {
        let before = members
            .rig()
            .open_frames()
            .into_iter()
            .find(|frame| frame["id"] == format!("p{}-chief", members.project_id()).as_str());
        members = members.restart(&log)?;
        let mut again = Bench::continued(&members, &log, checks);
        review::restarted(&mut again, before.as_ref())?;
        checks = again.into_checks();
    }
    let closed = members.close();

    let failed = checks.iter().filter(|check| !check.ok).count();
    log.say(format!(
        "{}/{} checks passed",
        checks.len() - failed,
        checks.len()
    ));
    log.block("what each check came to", &report::table(&checks));
    // A rig that would not end fails the run after what the checks came to is said.
    closed?;
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_bench_runs_every_step_on_opencode_pi_and_devin_with_an_opencode_chief_and_a_devin_reviewer(
    ) {
        let all = options(None, None, None, &[]).unwrap();
        assert_eq!(
            all,
            Options {
                chief: Harness::OpenCode,
                reviewer: Harness::Devin,
                all: true,
                workers: vec![Harness::OpenCode, Harness::Pi, Harness::Devin],
            }
        );
    }

    #[test]
    fn the_chief_the_reviewer_the_steps_and_the_workers_are_what_the_words_name() {
        let some = options(
            Some("claude"),
            Some("pi"),
            Some("questions"),
            &words(&["codex", "claude"]),
        )
        .unwrap();
        assert_eq!(
            some,
            Options {
                chief: Harness::Claude,
                reviewer: Harness::Pi,
                all: false,
                workers: vec![Harness::Codex, Harness::Claude],
            }
        );
    }

    #[test]
    fn what_the_bench_cannot_run_is_refused_in_the_words_it_always_used() {
        let refused = |result: Result<Options>| result.err().map(|failed| failed.to_string());
        assert_eq!(
            refused(options(Some("pi"), None, None, &[])).as_deref(),
            Some("unsupported bench chief: pi")
        );
        assert_eq!(
            refused(options(Some("kimi"), None, None, &[])).as_deref(),
            Some("unsupported bench chief: kimi")
        );
        assert_eq!(
            refused(options(None, Some("kimi"), None, &[])).as_deref(),
            Some("unsupported bench reviewer: kimi")
        );
        assert_eq!(
            refused(options(None, None, Some("some"), &[])).as_deref(),
            Some("unsupported bench steps: some")
        );
        assert_eq!(
            refused(options(None, None, None, &words(&["kimi"]))).as_deref(),
            Some("unsupported bench worker: kimi")
        );
    }

    #[test]
    fn the_bench_opens_its_chief_a_worker_of_each_harness_and_the_reviewer_each_on_an_agent_of_its_own(
    ) {
        let plan = plan_of(&options(None, None, None, &[]).unwrap());
        assert_eq!(plan.folder, "bench");
        assert_eq!(plan.brief, BENCH_BRIEF);
        assert_eq!(plan.chief, "bench-chief");
        let ids: Vec<&str> = plan
            .roster
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "bench-chief",
                "bench-opencode",
                "bench-pi",
                "bench-devin",
                "bench-reviewer"
            ]
        );
        // The chief and the opencode worker are the same harness on the same model: two agents.
        assert_eq!(plan.roster[0]["kind"], plan.roster[1]["kind"]);
        assert_eq!(plan.roster[0]["model"], plan.roster[1]["model"]);
        // Each worker has the tier of its own that a task that names it lands on.
        let tiers: Vec<&str> = plan
            .roster
            .iter()
            .map(|row| row["workTier"].as_str().unwrap())
            .collect();
        assert_eq!(tiers[1..4], ["light", "standard", "critical"]);
    }
}
