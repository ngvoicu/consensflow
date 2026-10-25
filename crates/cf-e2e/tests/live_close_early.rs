//! Live: the human's Close in the first moments of a worker's window, while its task's
//! brief is on its way to it, fails nothing, and the Resume delivers the brief once
//! (T-534, bug 2; Calliope's review of T-432, T-520). It is opt-in (`cargo xtask live
//! close-early`): it spends the machine's quota on one short task for each harness and
//! moment named, and needs each installed and logged in, so it is ignored unless asked
//! for and is no part of `check` or of CI.
//!
//! The field bug (3.0.6-alpha.2, the tester's T-534, 3 of 3): a Close within about 0.5 s
//! to 2 s of a Claude Code worker's window starting failed its task. The brief's delivery
//! had begun, the project was suspended, the window closed, and the delivery failed
//! "window closed"; the task went from queued to failed, the chief and the human heard
//! it failed, and Resume brought nothing back. Codex and OpenCode were fine at 0 ms.
//!
//! The native daemon and the real pane host run in a home of the run's own; the chief is
//! the stand-in, and the check plays the chief with its token. Each harness is the one
//! worker of a project of its own, given a one-line task; once its brief is on its way
//! (the ledger's `delivering`), the run waits each moment named and closes the project
//! as the page does. Five seconds on, the brief must be back in its queue or delivered,
//! the task not failed, and nobody told it failed. Then the human's Resume: the task
//! must be done, its result at the chief with the word, and the brief confirmed once.
//!
//! ```text
//! cargo xtask live close-early                         Claude Code, Codex and OpenCode, at 0 and 500 ms
//! cargo xtask live close-early claude                  one harness
//! cargo xtask live close-early claude --at-ms 0,2000   the moments named
//! cargo xtask live close-early claude --claude-config <folder>
//!                                                      Claude on a config of the run's own
//! ```
//!
//! Codex reads its login from `CODEX_HOME`, which the run's caller points at a home of
//! the run's own.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::members::{self, delivered_result, find, is_over, Account, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, between, Harness, Log, Who};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the table says each worker was checked for.
const CHECKED: &str =
    "a Close while the brief is on its way fails nothing; the Resume delivers it once";

/// How long the brief has to be on its way, the Close to settle, and the task to be done
/// after the Resume.
const ON_ITS_WAY: Duration = Duration::from_secs(60);
const SETTLES: Duration = Duration::from_secs(5);
const WORK: Duration = Duration::from_secs(300);
const DELIVERS: Duration = Duration::from_secs(60);

/// How often the brief's state is looked at before the Close, and the board after it.
const QUICK: Duration = Duration::from_millis(50);
const LOOK: Duration = Duration::from_secs(1);

/// The harnesses the words of the command line name, or the three of T-534.
fn workers(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(vec![Harness::Claude, Harness::Codex, Harness::OpenCode]);
    }
    named
        .iter()
        .map(|name| {
            Harness::from_name(name)
                .ok_or_else(|| Error::Daemon(format!("no such harness: {name}")))
        })
        .collect()
}

/// The moments after the brief is on its way that the Close comes at, in ms: `--at-ms`,
/// or 0 and 500, the tester's.
fn moments(given: Option<String>) -> Result<Vec<u64>> {
    let Some(given) = given else {
        return Ok(vec![0, 500]);
    };
    given
        .split(',')
        .map(|ms| {
            ms.trim()
                .parse()
                .map_err(|_| Error::Daemon(format!("--at-ms: not a number of ms: {ms}")))
        })
        .collect()
}

/// The word the worker is asked to reply with, of its own for each harness and moment.
fn word(harness: Harness, at: u64) -> String {
    format!(
        "CLOSE-EARLY-{}-{at}",
        harness.name().to_uppercase().replace('-', "")
    )
}

/// The state of task `number`'s brief, its first message.
fn brief_state(thread: &Value) -> String {
    live::text(&thread["messages"][0]["state"])
}

/// How many events of `kind` the ledger holds for the message `id`.
fn events_of(members: &Members, kind: &str, id: i64) -> usize {
    members
        .events()
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .count()
}

/// The bodies of the messages to `participant` (the human's, for none) about task
/// `number` that say it failed.
fn told_failed(members: &Members, participant: Option<&str>, number: i64) -> Result<Vec<String>> {
    let failed = format!("T-{number} failed");
    Ok(members
        .inbox(participant)?
        .iter()
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains(&failed) || body.contains("did not reach"))
        .map(str::to_owned)
        .collect())
}

/// The worker of `harness`, its project closed `at` ms after its brief is on its way,
/// and resumed: what it came to.
fn run(harness: Harness, at: u64, log: &Log) -> Result<Account> {
    let log = log.scoped(format!("{} at {at} ms", harness.name()));
    let plan = Plan::new(
        &format!("close-early-{}-{at}", harness.name()),
        members::stand_in_chief(),
    )
    .with(members::member(harness));
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let account = steps(&mut members, harness, at, &log);
    let closed = members.close();
    let mut account = account?;
    account.closed(closed);
    Ok(account)
}

fn steps(members: &mut Members, harness: Harness, at: u64, log: &Log) -> Result<Account> {
    let mut account = Account::new();
    let agent = agents::id(harness);
    members.add(&agent, &[])?;
    let word = word(harness, at);
    let number = members.give(&agent, &format!("Reply with exactly one line: {word}"))?;
    let brief = members.task(number)?["messages"][0]["id"]
        .as_i64()
        .ok_or_else(|| Error::Daemon(format!("T-{number} has no brief")))?;
    find(log, "the brief on its way", ON_ITS_WAY, QUICK, || {
        let thread = members.task(number)?;
        Ok((brief_state(&thread) == "delivering").then_some(()))
    })?
    .ok_or_else(|| Error::Daemon(format!("m-{brief} never went on its way")))?;
    sleep(Duration::from_millis(at));
    let then = brief_state(&members.task(number)?);
    let closed = members.page("project.close", json!({}))?;
    account.say(format!(
        "closed {at} ms after m-{brief} was on its way, when it was {then}: {}",
        closed["ok"]
    ));
    sleep(SETTLES);
    let thread = members.task(number)?;
    let (task, message) = (live::text(&thread["state"]), brief_state(&thread));
    account.check(
        "after the Close: the brief is back in its queue, or was delivered",
        message == "queued" || message == "delivered",
        format!("m-{brief} {message}"),
    );
    account.check(
        "after the Close: the task did not fail",
        task != "failed",
        format!("T-{number} {task}"),
    );
    let mut failed = told_failed(members, Some("chief"), number)?;
    failed.extend(told_failed(members, None, number)?);
    account.check(
        "nobody is told it failed",
        failed.is_empty(),
        format!("{failed:?}"),
    );

    members.resume()?;
    let over = find(log, &format!("T-{number} to be over"), WORK, LOOK, || {
        let thread = members.task(number)?;
        Ok(is_over(&thread).then(|| live::text(&thread["state"])))
    })?;
    account.check(
        "after the Resume: the task is done",
        over.as_deref() == Some("done"),
        format!("T-{number} {over:?}"),
    );
    let result = find(log, "its result at the chief", DELIVERS, LOOK, || {
        let inbox = members.inbox(Some("chief"))?;
        Ok(delivered_result(&inbox).filter(|body| body.contains(&word)))
    })?;
    account.check(
        "its result reached the chief with the word",
        result.is_some(),
        format!("{result:?}"),
    );
    let (begun, confirmed) = (
        events_of(members, "delivery.begun", brief),
        events_of(members, "delivery.confirmed", brief),
    );
    account.check(
        "the brief was confirmed once",
        confirmed == 1,
        format!("m-{brief}: begun {begun}, confirmed {confirmed}"),
    );
    account.say(format!(
        "windows opened for the worker: {}",
        members.windows_opened_for(&agent).len()
    ));
    Ok(account)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a short task for each harness and moment named; cargo xtask live close-early"]
fn a_close_while_a_workers_brief_is_on_its_way_fails_nothing_and_the_resume_delivers_it_once(
) -> Result {
    let log = Log::begin("live close-early")?;
    let at = moments(live::option("at-ms"))?;
    let mut checks = Vec::new();
    for harness in workers(&live::list("harness"))? {
        for &ms in &at {
            let who = Who::Harness(harness);
            let before = who.version();
            let account = run(harness, ms, &log).unwrap_or_else(|failure| {
                let mut account = Account::new();
                account.fail(&failure);
                account
            });
            let version = between(before, who.version());
            let name = format!("{} at {ms} ms", harness.name());
            log.block(
                &format!(
                    "== {name} {version}: {}",
                    if account.held() { "ok" } else { "FAILED" }
                ),
                &account.text(),
            );
            checks.push(Check::new(&name, CHECKED, account.held(), version));
        }
    }
    log.report("what each worker came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_moments_are_the_testers_unless_named() {
        assert_eq!(moments(None).unwrap(), [0, 500]);
        assert_eq!(moments(Some("0, 2000".to_owned())).unwrap(), [0, 2000]);
        assert!(moments(Some("soon".to_owned())).is_err());
    }

    #[test]
    fn each_harness_and_moment_has_a_word_of_its_own() {
        assert_ne!(word(Harness::Claude, 0), word(Harness::Claude, 500));
        assert_ne!(word(Harness::Claude, 0), word(Harness::Codex, 0));
    }
}
