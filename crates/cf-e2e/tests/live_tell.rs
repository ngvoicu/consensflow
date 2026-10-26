//! Live: the chief's tell to a worker inside a command, and the answer the worker's turn ends
//! with, on a real harness. It is opt-in (`cargo xtask live tell`, `npm run live:tell`): it
//! spends the machine's quota on one short task of each harness named, and needs each
//! installed and logged in, so it is ignored unless asked for and is no part of `check` or
//! of CI.
//!
//! The shape of T-442 (T-429 on 3.0.5; T-534's live runs on 3.0.6-alpha.2 on Claude Code,
//! Codex and OpenCode): a worker runs a long command, and the chief tells its window
//! `cf tell T-1 "…"`, which pauses the task, stops the command and puts the words to the
//! window as a question. The words ask the window to write one word as its answer, run a
//! short command, and end its reply with another. A window answered the question with `cf
//! answer` and went on to that final word in the same turn, and the final word reached
//! nobody: the task was paused, so it was no result, and no message carried it. OpenCode
//! answered in plain text, with both words in one reply and no `cf answer`, and lost the
//! answer to the question as well. The chief's `cf task resume` then began a turn whose
//! short follow-up became the result.
//!
//! The native daemon and the real pane host run in a home of the run's own. The chief is a
//! stand-in, and the check plays the chief with its own token. What the run judges is what
//! the chief was given: the words the turn ended with, in a note, once, before any resume
//! (the answer to the question reached the chief too, as its own message or in the words of
//! that note); the task still paused, with no result; and `cf task finish T-1` making those
//! words the result. What the window did is said besides: the lane's state, the thread and
//! the chief's inbox.
//!
//! Two cases (`--case`): `plain`, the shape above, on every harness named; and `monitor`
//! (Claude Code's alone, which has the Monitor tool; Calliope's P2 on round 2 of T-442), where
//! the turn on the tell arms a Monitor on a command, ends at once, and the monitor's expiry
//! wakes the turn that ends with the final words: a turn that ended with a monitor that said it
//! ended, which is told once a look at the window's commands finds none. The case checks the
//! record shows the Monitor armed, so a window that skipped it passes nothing. Claude runs
//! both by default.
//!
//! ```text
//! cargo xtask live tell                    Claude, the default: both cases
//! cargo xtask live tell codex opencode     the ones named, one after the other
//! cargo xtask live tell --case monitor     Claude's turn with a Monitor
//! cargo xtask live tell -- --list          the check, listed
//! ```
//!
//! The run fails when the words the turn ended with did not reach the chief before the
//! resume, reached it more than once, when the task was not left paused or a result was
//! taken for it, or when Finish did not make those words the result.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::members::{
    find, member, named, stand_in_chief, Account, LongCommand, Members, Plan, ThisMachine,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, cut, text, Harness, Log};
use cf_e2e::Result;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command the worker is inside of when the chief tells it.
const LONG: LongCommand = LongCommand::new("cf-tell-long-command");

/// What the worker is asked to write as its answer to the tell, and to end its reply with.
const ANSWER: &str = "TOLD-ANSWER-K3";
const REPORT: &str = "FINAL-RESULT-ALPHA-9Z";

/// How long the worker has to be inside its command, and the words the turn ends with to
/// reach the chief.
const WORK: Duration = Duration::from_secs(240);
const DONE: Duration = Duration::from_secs(300);

/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What a run of the check asks of the window in the turn on the tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// A short command, then the final words.
    Plain,
    /// A Monitor armed on a command, and the final words when its expiry wakes the turn.
    Monitor,
}

impl Turn {
    /// The name `--case` gives it.
    const fn name(self) -> &'static str {
        match self {
            Turn::Plain => "plain",
            Turn::Monitor => "monitor",
        }
    }

    /// What the chief tells the window.
    fn told(self) -> String {
        match self {
            Turn::Plain => format!(
                "Stop waiting for that command; do not run it again. First write the line {ANSWER} as text. Then, in this same turn, run `sleep 15` with your shell tool and wait for it. Then end your reply with exactly the line {REPORT}."
            ),
            Turn::Monitor => format!(
                "Stop waiting for that command; do not run it again. First answer this question with cf answer, the answer being the line {ANSWER}. Then use ToolSearch to load the Monitor tool (select:Monitor), and use Monitor to watch the command: exec sleep 611 , with timeout_ms 20000 and description 'tell-watch'. Then end your turn at once, saying only WATCHING. When a notification says the monitor expired, end your reply with exactly the line {REPORT}."
            ),
        }
    }

    /// The cases a harness runs when none is asked for: Claude both, the others the plain one.
    fn default_for(harness: Harness) -> Vec<Turn> {
        if harness == Harness::Claude {
            vec![Turn::Plain, Turn::Monitor]
        } else {
            vec![Turn::Plain]
        }
    }

    /// The cases asked for with `--case`, for `harness`: those named, or its defaults.
    fn asked(names: &[String], harness: Harness) -> Result<Vec<Turn>> {
        if names.is_empty() {
            return Ok(Self::default_for(harness));
        }
        let mut turns = Vec::new();
        for name in names {
            let turn = [Turn::Plain, Turn::Monitor]
                .into_iter()
                .find(|turn| turn.name() == name)
                .ok_or_else(|| cf_e2e::Error::Daemon(format!("no such case: {name}")))?;
            if turn == Turn::Monitor && harness != Harness::Claude {
                continue;
            }
            turns.push(turn);
        }
        Ok(turns)
    }
}

/// The project of the run: a stand-in chief, and `worker` on its cheap model, in a folder of
/// the case's own.
fn plan(worker: Harness, turn: Turn) -> Plan {
    let folder = match turn {
        Turn::Plain => "tell".to_owned(),
        Turn::Monitor => format!("tell-{}", turn.name()),
    };
    Plan::new(&folder, stand_in_chief()).with(member(worker))
}

/// The id of the plan's worker.
fn agent_of(plan: &Plan) -> String {
    plan.roster[1]["id"].as_str().unwrap_or_default().to_owned()
}

/// Whether the worker's window has its long command running.
fn in_command(members: &Members, agent: &str) -> Result<bool> {
    let own = members
        .window_of(agent)?
        .and_then(|window| members.window_process(&window));
    Ok(match own {
        Some(own) => !LONG.chosen_in(&ThisMachine, own)?.is_empty(),
        None => false,
    })
}

/// The long command ended, if the worker's window still has it: a tell's stop leaves a
/// command it did not end running, and the window waits for it.
fn end_command(members: &Members, agent: &str) {
    if let Ok(Some(window)) = members.window_of(agent) {
        if let Some(own) = members.window_process(&window) {
            let _ = LONG.end_in(own);
        }
    }
}

fn state(members: &Members, number: i64) -> String {
    members
        .task(number)
        .map(|task| text(&task["state"]))
        .unwrap_or_else(|failed| format!("unread: {failed}"))
}

/// The task's thread, one message a line: id, kind, sender → recipient, state, body.
fn thread(members: &Members, number: i64) -> String {
    let Ok(task) = members.task(number) else {
        return "unread".to_owned();
    };
    let mut lines = vec![format!(
        "T-{number} {} assignee {} requester {}",
        text(&task["state"]),
        text(&task["assignee"]),
        text(&task["requester"])
    )];
    for message in task["messages"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  m-{} {} {} → {} [{}] {}",
            message["id"],
            text(&message["kind"]),
            text(&message["sender"]),
            text(&message["recipient"]),
            text(&message["state"]),
            cut(&text(&message["body"]), 300)
        ));
    }
    lines.join("\n")
}

/// The chief's inbox, one message a line.
fn chief_inbox(members: &Members) -> String {
    members
        .inbox(Some("chief"))
        .unwrap_or_default()
        .iter()
        .map(|message| {
            format!(
                "  m-{} {} from {} [{}] {}",
                message["id"],
                text(&message["kind"]),
                text(&message["sender"]),
                text(&message["state"]),
                cut(&text(&message["body"]), 300)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The messages in the chief's inbox that hold `word`, as their kind and state.
fn reached_chief(members: &Members, word: &str) -> Vec<(String, String)> {
    members
        .inbox(Some("chief"))
        .unwrap_or_default()
        .into_iter()
        .filter(|message| text(&message["body"]).contains(word))
        .map(|message| (text(&message["kind"]), text(&message["state"])))
        .collect()
}

/// The results the chief has of T-`number`, as their words.
fn results(members: &Members, number: i64) -> Vec<String> {
    members
        .inbox(Some("chief"))
        .unwrap_or_default()
        .into_iter()
        .filter(|message| message["kind"] == "result" && message["taskNumber"] == number)
        .map(|message| text(&message["body"]))
        .collect()
}

/// `cf …` as the chief, its words, code and output said in the case.
fn chief(members: &Members, case: &mut Account, words: &[&str]) -> Result<cf_e2e::process::Ran> {
    let ran = members.as_chief(words)?;
    case.say(format!(
        "$ cf {} → exit {:?}\n      out: {}\n      err: {}",
        words
            .iter()
            .map(|word| cut(word, 80))
            .collect::<Vec<_>>()
            .join(" "),
        ran.code,
        cut(&ran.stdout, 800),
        cut(&ran.stderr, 400)
    ));
    Ok(ran)
}

/// The number in `cf task add --json`'s answer, or in its text (`T-12 …`).
fn number_in(out: &str) -> Option<i64> {
    let at = out.find("T-")?;
    out[at + 2..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// The case: the chief tells the worker inside its command, the worker's turn ends with the
/// words it was asked for, and the chief finishes the task.
fn drive(mut members: Members, agent: &str, turn: Turn, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    members.add(agent, &[])?;
    let tier = members.tier_of(agent).unwrap_or("standard").to_owned();
    let brief = format!(
        "Run this exact command with your shell tool and wait for it to finish:\n{}\nThen reply with exactly one line: RESULT-ALPHA-7Q",
        LONG.text()
    );
    let mut words: Vec<String> = vec!["task".to_owned(), "add".to_owned()];
    words.extend(cf_e2e::live::members::tier_flag(&tier));
    words.push(brief);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let added = chief(&members, &mut case, &words)?;
    let Some(number) = number_in(&added.stdout) else {
        case.check("the task was added", false, added.to_string());
        case.closed(members.close());
        return Ok(case);
    };
    let reached = find(log, "the worker inside its command", WORK, LOOK, || {
        Ok(in_command(&members, agent)?.then_some(()))
    })?;
    case.check(
        "the worker reached its command",
        reached.is_some(),
        state(&members, number),
    );
    if reached.is_none() {
        case.say(thread(&members, number));
        case.closed(members.close());
        return Ok(case);
    }
    sleep(Duration::from_secs(3));

    let target = format!("T-{number}");
    chief(&members, &mut case, &["tell", &target, &turn.told()])?;
    case.check(
        "the tell paused the task",
        state(&members, number) == "paused",
        state(&members, number),
    );

    // The window answers, goes on, and ends its turn; the words it ended with reach the chief.
    let ended = find(
        log,
        "the words the turn ended with to reach the chief",
        DONE,
        LOOK,
        || {
            end_command(&members, agent);
            Ok(reached_chief(&members, REPORT)
                .iter()
                .any(|(kind, _)| kind == "note")
                .then_some(()))
        },
    )?;
    case.check(
        "the words the turn ended with reach the chief, in a note, before any resume",
        ended.is_some(),
        format!("{:?}", reached_chief(&members, REPORT)),
    );
    // A note told twice would be in by now.
    sleep(Duration::from_secs(5));
    let notes = reached_chief(&members, REPORT)
        .into_iter()
        .filter(|(kind, _)| kind == "note")
        .count();
    case.check("they are told once", notes == 1, format!("{notes} notes"));
    case.check(
        "the answer to the tell reached the chief too (a message of its own, or in the note)",
        !reached_chief(&members, ANSWER).is_empty(),
        format!("{:?}", reached_chief(&members, ANSWER)),
    );
    case.say(format!(
        "after the window's turn: T-{number} is {}",
        state(&members, number)
    ));
    case.say(thread(&members, number));
    case.say(format!("the chief's inbox:\n{}", chief_inbox(&members)));
    case.check(
        "the task is left paused, with no result taken for the chief",
        state(&members, number) == "paused" && results(&members, number).is_empty(),
        format!(
            "{}; results {:?}",
            state(&members, number),
            results(&members, number)
        ),
    );

    if turn == Turn::Monitor {
        // The record shows the turn armed a Monitor: a window that skipped it proves nothing.
        let seen = chief(
            &members,
            &mut case,
            &[
                "task",
                "get",
                &target,
                "--transcript",
                "--find",
                "tell-watch",
            ],
        )?;
        case.check(
            "the turn on the tell armed the Monitor",
            seen.stdout.contains("tell-watch"),
            cut(&seen.stdout, 300),
        );
    }
    let finished = chief(&members, &mut case, &["task", "finish", &target])?;
    case.check(
        "cf task finish answers",
        finished.code == Some(0),
        finished.to_string(),
    );
    let landed = find(
        log,
        "the result to reach the chief",
        Duration::from_secs(30),
        LOOK,
        || Ok((!results(&members, number).is_empty()).then_some(())),
    )?;
    let got = results(&members, number);
    case.check(
        "Finish makes the words the turn ended with the result",
        landed.is_some()
            && state(&members, number) == "done"
            && got.len() == 1
            && got[0].contains(REPORT),
        format!("{}; results {got:?}", state(&members, number)),
    );
    case.say(thread(&members, number));
    end_command(&members, agent);
    case.closed(members.close());
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a worker of each harness named; cargo xtask live tell (npm run live:tell)"]
fn a_workers_final_answer_after_the_chiefs_tell_reaches_the_chief_and_finish_makes_it_the_result(
) -> Result {
    let log = Log::begin("live tell")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let workers = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &[Harness::Claude],
    )?;
    let asked = live::list("case");
    let mut checks = Vec::new();
    for worker in workers {
        let version = live::version_of(worker);
        for turn in Turn::asked(&asked, worker)? {
            let plan = plan(worker, turn);
            let agent = agent_of(&plan);
            let scoped = log.scoped(format!("{} worker {}", worker.name(), turn.name()));
            let case = Members::open(&plan, Path::new(FAKE_AGENT), &scoped)
                .and_then(|members| drive(members, &agent, turn, &scoped))
                .unwrap_or_else(|failed| {
                    let mut case = Account::new();
                    case.fail(failed);
                    case
                });
            log.block(
                &format!("== {} worker, {}: {version}", worker.name(), turn.name()),
                &case.text(),
            );
            checks.push(Check::new(
                &format!("{} worker", worker.name()),
                &format!(
                    "the final answer after a tell reaches the chief ({}), and Finish makes it the result",
                    turn.name()
                ),
                case.held(),
                version.clone(),
            ));
        }
    }
    report::conclude(&log, "what each worker came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chief_tells_the_worker_to_answer_with_one_word_and_to_end_with_another() {
        for turn in [Turn::Plain, Turn::Monitor] {
            let words = turn.told();
            assert!(words.contains(ANSWER) && words.contains(REPORT));
            assert!(
                words.find(ANSWER) < words.find(REPORT),
                "the answer comes first"
            );
        }
        assert_ne!(ANSWER, REPORT);
    }

    #[test]
    fn the_monitor_turn_arms_a_monitor_that_expires_and_ends_with_the_words_its_expiry_wakes() {
        let words = Turn::Monitor.told();
        assert!(words.contains("select:Monitor") && words.contains("timeout_ms 20000"));
        assert!(words.contains("'tell-watch'"));
        assert!(words.ends_with(&format!("exactly the line {REPORT}.")));
        assert!(!Turn::Plain.told().contains("Monitor"));
    }

    #[test]
    fn claude_runs_both_cases_by_default_and_the_others_the_plain_one_and_a_monitor_asked_of_them_is_none(
    ) {
        assert_eq!(
            Turn::asked(&[], Harness::Claude).unwrap(),
            [Turn::Plain, Turn::Monitor]
        );
        assert_eq!(Turn::asked(&[], Harness::Codex).unwrap(), [Turn::Plain]);
        let monitor = ["monitor".to_owned()];
        assert_eq!(
            Turn::asked(&monitor, Harness::Claude).unwrap(),
            [Turn::Monitor]
        );
        assert_eq!(Turn::asked(&monitor, Harness::Codex).unwrap(), []);
        let nonsense = ["fast".to_owned()];
        assert_eq!(
            Turn::asked(&nonsense, Harness::Claude)
                .unwrap_err()
                .to_string(),
            "no such case: fast"
        );
    }

    #[test]
    fn the_project_is_a_stand_in_chief_and_one_worker_of_the_harness_on_its_cheap_model() {
        for harness in Harness::ALL {
            let plan = plan(harness, Turn::Plain);
            assert_eq!(plan.folder, "tell");
            assert_eq!(plan.roster.len(), 2, "{harness:?}");
            assert_eq!(plan.roster[1]["kind"], harness.kind());
            assert_eq!(plan.roster[1]["model"], live::agents::model(harness).name);
            assert_eq!(agent_of(&plan), plan.roster[1]["id"].as_str().unwrap());
        }
        assert_eq!(plan(Harness::Claude, Turn::Monitor).folder, "tell-monitor");
    }

    #[test]
    fn a_task_number_is_read_from_the_chiefs_words() {
        assert_eq!(
            number_in("T-1 is on the board for a complex worker; the first free one gets it"),
            Some(1)
        );
        assert_eq!(number_in("nothing"), None);
    }
}
