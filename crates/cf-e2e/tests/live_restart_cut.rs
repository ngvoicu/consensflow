//! Live: a chief the app's quit cut in a turn, through the product. It is opt-in
//! (`cargo xtask live restart-cut`, `npm run live:restart-cut`): it spends the
//! machine's quota on a chief of each harness named, and needs each one installed
//! and logged in, so it is ignored unless asked for and is no part of `check` or
//! of CI.
//!
//! A real Claude Code is the chief of a project on the native daemon, and its one
//! member is the stand-in (no model). The human asks the chief to run a long
//! command; the app quits with the chief inside it (the daemon dies, then the pane
//! host, which ends every window); the app starts again on the same home, and the
//! daemon resumes the project: the chief's window opens on `--resume` of the
//! conversation the quit cut. Claude resumed waits at its prompt, status idle, and
//! does not go on with the turn (`cargo xtask live resume-cut`), so the daemon has
//! to read that window at rest: the lane reads idle, and what waits for the chief
//! goes in. A task is then given to the member, as the chief would give it, and its
//! result must reach the chief, which answers it.
//!
//! ```text
//! cargo xtask live restart-cut                     Claude is the chief
//! cargo xtask live restart-cut --chief pi          Pi is
//! cargo xtask live restart-cut claude pi           both, one after the other
//! ```
//!
//! The run fails when a chief's lane never read idle after the restart, or the
//! result of the task was not delivered to it.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{
    find, member_as, stand_in_worker, Account, LongCommand, Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, installed, last_lines, seconds, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::Value;

/// The stand-in for the member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command the chief is cut inside of.
const LONG: LongCommand = LongCommand::new("cf-restart-cut-long-command");

/// How long the chief may take to be at its command, to read idle after the
/// restart, and to be given a result.
const WORK: Duration = Duration::from_secs(180);
const IDLE: Duration = Duration::from_secs(60);
const DELIVERED: Duration = Duration::from_secs(180);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the human asks the chief: to run the long command and wait for it.
fn the_ask() -> String {
    format!(
        "Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n{}",
        LONG.text()
    )
}

/// The chiefs to run: those `--chief` and the harnesses named ask for, Claude
/// when none is, each Claude or Pi and once.
fn chiefs(named: &[String]) -> Result<Vec<Harness>> {
    if named.is_empty() {
        return Ok(vec![Harness::Claude]);
    }
    let mut chiefs: Vec<Harness> = Vec::new();
    for word in named {
        let chief = match Harness::from_name(word) {
            Some(chief @ (Harness::Claude | Harness::Pi)) => chief,
            _ => return Err(Error::Daemon(format!("no such chief: {word}"))),
        };
        if !chiefs.contains(&chief) {
            chiefs.push(chief);
        }
    }
    Ok(chiefs)
}

/// What the check opens: a project whose chief is the real `chief`, on its harness's
/// cheap model (Pi on its cheap eval model: the bench's own Pi model was one that
/// hangs), and whose one member is the stand-in agent, which has no model.
fn plan_of(chief: Harness) -> Plan {
    Plan::new("restart-cut", member_as(chief, "chief")).with(stand_in_worker())
}

/// The states a lane was read in, each with when it was first read, as a line:
/// `idle from +0.0 s → working from +2.5 s`.
fn reads(samples: &[(String, Instant)], from: Instant) -> String {
    samples
        .iter()
        .map(|(state, at)| {
            format!(
                "{state} from +{} s",
                seconds(i64::try_from(at.duration_since(from).as_millis()).unwrap_or(i64::MAX))
            )
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// The state of the chief's lane, or `no lane`.
fn state_of(lane: &Value) -> String {
    lane["activity"]["state"]
        .as_str()
        .unwrap_or("no lane")
        .to_owned()
}

/// The result for the task `number` in the chief's inbox, and whether it was
/// delivered.
fn result_for(members: &Members, number: i64) -> Result<Option<Value>> {
    Ok(members
        .inbox(Some("chief"))?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

/// The chief is cut in a command, the app is quit and started again, and the
/// chief it resumes reads at rest and is given what waits for it.
fn drive(mut members: Members, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!("trust: {}", members.trust().unwrap_or("none")));
    members.add(STAND_IN_WORKER, &[])?;

    // The human asks the chief to run a long command.
    let pane = members.lane("chief")?["pane"].clone();
    members.paste(&pane, &ask::window_text(&the_ask())?)?;
    let at_command = find(
        log,
        "the chief inside its command",
        WORK,
        Duration::from_millis(200),
        || Ok((!LONG.named()?.is_empty()).then_some(())),
    )?;
    if at_command.is_none() {
        case.fail(format!(
            "the chief never ran its command: {}",
            last_lines(
                &members
                    .rig()
                    .screen(pane["id"].as_str().unwrap_or_default())
            )
        ));
        case.closed(members.close());
        return Ok(case);
    }
    sleep(Duration::from_secs(2));
    case.say(format!(
        "before the quit: the chief's lane reads {}, its command runs",
        state_of(&members.lane("chief")?)
    ));

    // The app's quit, and the app started again.
    let quit_at = Instant::now();
    let members = members.restart(log)?;
    LONG.end();
    case.say(format!(
        "quit and started again in {} s",
        seconds(i64::try_from(quit_at.elapsed().as_millis()).unwrap_or(i64::MAX))
    ));

    // The resumed chief: the lane, sampled until it reads idle or the wait is out.
    let mut samples: Vec<(String, Instant)> = Vec::new();
    let started_at = Instant::now();
    let idle = find(
        log,
        "the resumed chief's lane to read idle",
        IDLE,
        LOOK,
        || {
            let state = state_of(&members.lane("chief")?);
            if samples.last().is_none_or(|(last, _)| *last != state) {
                samples.push((state.clone(), Instant::now()));
            }
            Ok((state == "idle").then_some(()))
        },
    )?;
    case.say(format!(
        "after the restart the chief's lane reads: {}",
        reads(&samples, started_at)
    ));
    if idle.is_none() {
        case.say(format!(
            "it never read idle in {} s: {}",
            IDLE.as_secs(),
            members.lane("chief")?["activity"]
        ));
    }

    // A result for it: the member's task, given as the chief would give it.
    let number = members.give(STAND_IN_WORKER, "Reply with exactly: DONE")?;
    let given = Instant::now();
    let delivered = find(
        log,
        "the result to be delivered to the chief",
        DELIVERED,
        LOOK,
        || Ok(result_for(&members, number)?.filter(|result| result["state"] == "delivered")),
    )?;
    case.say(match &delivered {
        Some(_) => format!(
            "T-{number}'s result was delivered to the chief {} s after the task was given",
            seconds(i64::try_from(given.elapsed().as_millis()).unwrap_or(i64::MAX))
        ),
        None => format!(
            "T-{number}'s result was never delivered in {} s: its state is {}; the chief's lane reads {}",
            DELIVERED.as_secs(),
            result_for(&members, number)?
                .and_then(|result| result["state"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "none".to_owned()),
            members.lane("chief")?["activity"]
        ),
    });
    let lane = members.lane("chief")?;
    case.say(format!(
        "the chief's screen: {}",
        last_lines(
            &members
                .rig()
                .screen(lane["pane"]["id"].as_str().unwrap_or_default())
        )
    ));
    case.require(idle.is_some() && delivered.is_some());
    case.closed(members.close());
    Ok(case)
}

/// Runs the check on `chief`: the case it came to, which holds when the chief the
/// quit cut read at rest and was given what waited for it.
fn run(chief: Harness, log: &Log) -> Account {
    let log = log.scoped(chief.name());
    let opened = plan_of(chief);
    let case = Members::open(&opened, Path::new(FAKE_AGENT), &log)
        .and_then(|members| drive(members, &log));
    // What the chief was inside is ended for good, whatever came of the run.
    LONG.end();
    case.unwrap_or_else(|failed| {
        let mut case = Account::new();
        case.fail(failed);
        case
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of each harness named; cargo xtask live restart-cut (npm run live:restart-cut)"]
fn a_chief_the_quit_cut_in_a_turn_reads_at_rest_and_is_given_what_waits_for_it() -> Result {
    let log = Log::new("live restart-cut");
    ask::prepare()?;
    let mut named = live::list("chief");
    named.extend(live::list("harness"));
    let mut checks = Vec::new();
    for chief in chiefs(&named)? {
        let version = installed(chief).map_or_else(
            |failed| format!("unknown ({failed})"),
            |install| install.version(),
        );
        let case = run(chief, &log);
        log.block(&format!("== {}: {version}", chief.name()), &case.text());
        checks.push(Check::new(
            chief.name(),
            if case.held() {
                "a chief the quit cut reads at rest and is given what waits for it"
            } else {
                "the chief the quit cut was not given what waited for it"
            },
            case.held(),
            version,
        ));
    }
    log.block("what each chief came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_human_asks_the_chief_to_run_the_long_command_and_to_wait_for_it() {
        let ask = the_ask();
        assert!(ask.starts_with("Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n"));
        assert!(
            ask.ends_with("node -e \"setTimeout(() => {}, 300000)\" cf-restart-cut-long-command")
        );
    }

    #[test]
    fn the_chief_is_claude_unless_told_and_only_claude_or_pi_each_once() {
        assert_eq!(chiefs(&[]).unwrap(), [Harness::Claude]);
        let both = ["pi", "claude", "pi"].map(str::to_owned);
        assert_eq!(chiefs(&both).unwrap(), [Harness::Pi, Harness::Claude]);
        for wrong in ["codex", "opencode", "devin", "nobody"] {
            let failed = chiefs(&[wrong.to_owned()]).unwrap_err().to_string();
            assert_eq!(failed, format!("no such chief: {wrong}"));
        }
    }

    #[test]
    fn the_chief_is_a_real_harness_on_its_cheap_model_and_the_one_member_is_the_stand_in() {
        for chief in [Harness::Claude, Harness::Pi] {
            let plan = plan_of(chief);
            assert_eq!(plan.folder, "restart-cut");
            assert_eq!(plan.chief, "chief");
            let (cheap, _) = live::agents::cheap(chief);
            assert_eq!(plan.roster[0]["id"], "chief");
            assert_eq!(plan.roster[0]["model"], cheap, "{chief:?}");
            assert_eq!(plan.roster[0]["kind"], chief.kind());
            // The member has no model: it only notes what it is sent.
            assert_eq!(plan.roster[1]["model"], "fake");
            // Only the chief's harness is the machine's: the stand-in is the member's Claude.
            assert_eq!(plan.harnesses(), [chief]);
        }
        assert_eq!(
            live::agents::cheap(Harness::Pi).0,
            "openrouter/meta/muse-spark-1.3"
        );
    }

    #[test]
    fn what_a_lane_read_is_said_as_each_state_with_when_it_was_first_read() {
        let from = Instant::now();
        let samples = vec![
            ("closed".to_owned(), from),
            ("idle".to_owned(), from + Duration::from_millis(2_500)),
        ];
        assert_eq!(
            reads(&samples, from),
            "closed from +0.0 s → idle from +2.5 s"
        );
        assert_eq!(reads(&[], from), "");
        assert_eq!(
            state_of(&serde_json::json!({ "activity": { "state": "idle" } })),
            "idle"
        );
        assert_eq!(state_of(&Value::Null), "no lane");
    }
}
