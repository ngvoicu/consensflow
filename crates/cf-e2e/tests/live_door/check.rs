//! One harness's door, run: the member opens as a member of a project whose chief
//! is the stand-in, is given a task that has it ask which colour to use, the
//! check as the chief answers it from the board with `cf answer`, and what the
//! daemon wrote shows that the answer was received at the door, once.

use std::collections::BTreeMap;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::claude::Status;
use cf_e2e::live::members::{
    self, answer_to, clock, find, first_question, is_over, judge, last_result, ms_of, Account,
    Members, Plan, STAND_IN_WORKER,
};
use cf_e2e::live::{seconds, Harness, Log, Who};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

use crate::sequence::{is_ours, sequence_line};
use crate::status;

/// How long a member may take to ask, and to finish once its answer is in.
pub const ASK: Duration = Duration::from_secs(420);
pub const DONE: Duration = Duration::from_secs(300);

/// How often the board is looked at while a member is waited for.
const LOOK: Duration = Duration::from_millis(500);

/// The one a word on the command line names: `fake` (the stand-in Claude, with no
/// model, which asks through the hook of its settings when its brief says to), or a
/// harness.
pub fn named(word: &str) -> Result<Who> {
    Who::named(word, "door")
}

/// How long the chief takes to answer, so that the door is mid-wait when the answer
/// comes: Claude's wait is longer, for its status is what is read over it, and `given`
/// (`--hold`) says another.
pub fn hold(who: Who, given: Option<Duration>) -> Duration {
    given.unwrap_or_else(|| {
        Duration::from_secs(if who == Who::Harness(Harness::Claude) {
            20
        } else {
            8
        })
    })
}

/// What a run of the check is asked.
#[derive(Debug, Clone)]
pub struct Asked {
    /// How long the chief takes to answer (`--hold`), where not the default.
    pub hold: Option<Duration>,
    /// The answer (`--answer`), which the member's result must carry.
    pub answer: String,
    /// The model a harness's member runs on, where not its cheap one (`--model
    /// <harness>=<model>`).
    pub models: BTreeMap<String, String>,
    /// Another run's rig ends while the chief has heard nothing (`--tidy true`): it takes
    /// the folder the stand-ins write their conversations in, which is empty then.
    pub tidy: bool,
}

/// What the board holds of a task a member was given: the question it asked, or
/// the end of the task without one.
enum First {
    Question(Value),
    Ended,
}

/// The first `most` characters of the text of a message's body, as JSON writes
/// them.
fn body_of(message: Option<&Value>, most: usize) -> String {
    let body: String = message
        .and_then(|message| message["body"].as_str())
        .unwrap_or_default()
        .chars()
        .take(most)
        .collect();
    Value::String(body).to_string()
}

/// The member's row of the roster: its harness on the run's model of it
/// (`--models`, else its cheap one), or on the model `--model` gave it.
fn row_of(harness: Harness, models: &BTreeMap<String, String>) -> Value {
    let mut row = members::member(harness);
    if let (Some(model), Some(fields)) = (models.get(harness.name()), row.as_object_mut()) {
        fields.insert("model".to_owned(), json!(model));
    }
    row
}

/// What a harness with no door is told: left out, and why, and not a failure.
fn left_out(harness: Harness) -> Account {
    let name = harness.name();
    let mut case = Account::new();
    case.say(format!(
        "left out: {}{} has no question tool, so it has no door",
        name[..1].to_uppercase(),
        &name[1..]
    ));
    case
}

/// Runs the door of `who`: the case it came to, which holds when the answer was
/// received there once as it should have been.
pub fn run(who: Who, asked: &Asked, stand_in: &Path, log: &Log) -> Result<Account> {
    let log = log.scoped(who.name());
    let (row, brief) = match who {
        Who::StandIn => (
            members::stand_in_worker(),
            members::stand_in_question_brief(),
        ),
        Who::Harness(harness) => match members::question_brief(harness) {
            Some(brief) => (row_of(harness, &asked.models), brief),
            None => return Ok(left_out(harness)),
        },
    };
    let agent = row["id"].as_str().unwrap_or(STAND_IN_WORKER).to_owned();
    let plan = Plan::new(&format!("door-{}", who.name()), members::stand_in_chief()).with(row);
    let mut members = Members::open(&plan, stand_in, &log)?;
    let case = observe(&mut members, &agent, &brief, (who, asked), &log);
    let closed = members.close();
    let mut case = case?;
    case.closed(closed);
    Ok(case)
}

/// What a rig of another run does as it ends, which the runs of a machine share the
/// folder for (`rig::forget`): it takes the folder the stand-ins write their
/// conversations in when it is empty, as the chief's is until it hears something.
/// The chief makes its folder again where it writes, or it ends with the first thing it
/// is told, and the check's `cf answer` is refused (2026-10-12).
fn tidy(members: &Members, case: &mut Account) {
    let folder = members.claude_config().join("projects").join("integration");
    case.say(match std::fs::remove_dir(&folder) {
        Ok(()) => format!(
            "another run's rig ended: {} is taken, empty as the chief's folder is until it hears something",
            folder.display()
        ),
        Err(failed) => format!("{} was not taken: {failed}", folder.display()),
    });
}

/// The question the member asks, answered as the chief, and what the daemon
/// wrote of it.
fn observe(
    members: &mut Members,
    agent: &str,
    brief: &str,
    (who, asked): (Who, &Asked),
    log: &Log,
) -> Result<Account> {
    let mut case = Account::new();
    if let Some(trust) = members.trust() {
        case.say(format!("trust: {trust}"));
    }
    members.add(agent, &[])?;
    if asked.tidy {
        tidy(members, &mut case);
    }
    let number = members.give(agent, brief)?;
    log.say(format!(
        "T-{number} is given: the member is to ask its question"
    ));

    // The question, or the end of the task without one: a member that answers on its
    // own never used its door, and waiting for it would only run out the clock.
    let first = find(log, "the member's question on the board", ASK, LOOK, || {
        let thread = members.task(number)?;
        Ok(first_question(&thread)
            .map(First::Question)
            .or_else(|| is_over(&thread).then_some(First::Ended)))
    })?;
    let window = members.window_of(agent)?;
    let screen = |window: &Option<members::MemberWindow>| {
        window
            .as_ref()
            .map_or_else(String::new, |window| members.tail(window.id()))
    };
    let Some(First::Question(question)) = first else {
        let thread = members.task(number)?;
        case.fail(match first {
            Some(_) => format!(
                "T-{number} ended ({}) and the member never put a question on the board; its result: {}",
                thread["state"].as_str().unwrap_or("undefined"),
                body_of(last_result(&thread), 300)
            ),
            None => format!(
                "the member never put its question on the board in {} s",
                ASK.as_secs()
            ),
        });
        case.say(format!("     its window: {}", screen(&window)));
        return Ok(case);
    };
    let window = window.ok_or_else(|| {
        Error::Daemon(format!(
            "the member asked its question from no window of {agent}"
        ))
    })?;
    let session = window.session();
    // Claude's own status, read as the daemon reads it, over the whole wait of its hook.
    let status = match (who, session.as_deref()) {
        (Who::Harness(Harness::Claude), Some(session)) => {
            Some(Status::watch(&members.claude_config(), session))
        }
        _ => None,
    };
    let waits = hold(who, asked.hold);
    log.say(format!(
        "the question is on the board: the chief answers in {} s",
        waits.as_secs()
    ));
    sleep(waits);
    let id = format!("m-{}", question["id"]);
    let given = members.as_chief(["answer", id.as_str(), asked.answer.as_str()])?;
    if given.code != Some(0) {
        case.fail(format!("cf answer: {given}"));
        return Ok(case);
    }
    // Its receipt (or the paste that stands in for it), then the member's result.
    let finished = find(log, "the task to end", DONE, LOOK, || {
        Ok(is_over(&members.task(number)?).then_some(()))
    })?;
    let samples = status.map(Status::stop);
    let thread = members.task(number)?;
    let Some(answer) = answer_to(&thread, &question) else {
        case.fail("no answer in the thread after cf answer");
        return Ok(case);
    };
    let events = members.events();
    let judged = judge(&events, &thread, &question, &answer, &asked.answer);
    case.say(format!(
        "question {id} on T-{number} put at {}, answered \"{}\" {} s later; T-{number} {}",
        clock(question["createdAt"].as_str().unwrap_or_default()),
        asked.answer,
        match (ms_of(&answer["createdAt"]), ms_of(&question["createdAt"])) {
            (Some(answered), Some(asked)) => seconds(answered - asked),
            _ => "?".to_owned(),
        },
        if finished.is_some() {
            "ended".to_owned()
        } else {
            format!(
                "still {} after {} s",
                thread["state"].as_str().unwrap_or("undefined"),
                DONE.as_secs()
            )
        }
    ));
    for finding in &judged.findings {
        case.check(finding.name, finding.ok, &finding.detail);
    }
    for note in &judged.notes {
        case.say(format!("note {note}"));
    }
    case.say("     the sequence the daemon wrote (events.jsonl):");
    let (answer_id, question_id) = (answer["id"].as_i64(), question["id"].as_i64());
    for event in &events {
        if is_ours(
            event,
            (answer_id, question_id),
            thread["number"].as_i64(),
            &window.handle,
        ) {
            case.say(sequence_line(event));
        }
    }
    if !case.held() {
        case.say(format!(
            "     the member's window: {}",
            members.tail(window.id())
        ));
    }
    // The chief's window, which the check answers as: it has to be the one it was opened as.
    case.say(format!("     {}", members.chief_window()));
    if let (Some(samples), Some(session)) = (samples, session) {
        // The hook's whole wait: from its question to the answer's receipt, or to the paste that replaced it.
        let from = ms_of(&question["createdAt"]).unwrap_or_default();
        let to = judged
            .received_at
            .unwrap_or_else(|| jiff::Timestamp::now().as_millisecond());
        status::say(
            &mut case,
            &samples,
            &events,
            &window.handle,
            &session,
            (from, to),
        );
    }
    Ok(case)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// A member that answers on its own never used its door, and waiting for its question
    /// would only run out the clock: the check ends when its task does, and says what the
    /// member answered instead. (The stand-in is told to answer, not to ask.)
    #[test]
    fn a_member_that_answers_on_its_own_never_used_its_door_and_the_check_says_what_it_answered() {
        let log = Log::new("live door test");
        let plan = Plan::new("door-alone-test", members::stand_in_chief())
            .with(members::stand_in_worker());
        let mut members = Members::open(&plan, Path::new(crate::FAKE_AGENT), &log).unwrap();
        let asked = Asked {
            hold: Some(Duration::ZERO),
            answer: "blue".to_owned(),
            models: BTreeMap::new(),
            tidy: false,
        };
        let started = Instant::now();
        let case = observe(
            &mut members,
            "worker",
            "Reply with exactly: ALONE",
            (Who::StandIn, &asked),
            &log,
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "it waited for a question that never came"
        );
        assert!(!case.held());
        assert!(
            case.text().contains(
                "FAIL T-1 ended (done) and the member never put a question on the board; its result: \"ALONE\""
            ),
            "{}",
            case.text()
        );
        assert!(case.text().contains("its window: "), "{}", case.text());
        members.close().unwrap();
    }

    #[test]
    fn a_door_is_named_by_a_harness_or_by_fake_and_nothing_else() {
        assert_eq!(named("fake").unwrap(), Who::StandIn);
        assert_eq!(named("opencode").unwrap(), Who::Harness(Harness::OpenCode));
        let failed = named("kimi").unwrap_err().to_string();
        assert_eq!(
            failed,
            "no door is called kimi: fake, claude, opencode, pi, devin, codex, muse"
        );
        assert_eq!(Who::StandIn.name(), "fake");
        assert_eq!(Who::Harness(Harness::Devin).name(), "devin");
    }

    #[test]
    fn the_chief_takes_twenty_seconds_to_answer_claude_and_eight_the_others_unless_told() {
        let claude = Who::Harness(Harness::Claude);
        assert_eq!(hold(claude, None), Duration::from_secs(20));
        for other in [
            Who::StandIn,
            Who::Harness(Harness::Codex),
            Who::Harness(Harness::Pi),
        ] {
            assert_eq!(hold(other, None), Duration::from_secs(8));
        }
        // `--hold 90` (npm run live:hook-status) is the wait of any.
        assert_eq!(
            hold(claude, Some(Duration::from_secs(90))),
            Duration::from_secs(90)
        );
        assert_eq!(
            hold(Who::StandIn, Some(Duration::from_secs(1))),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_harness_with_no_question_tool_is_left_out_and_not_failed() {
        let case = left_out(Harness::Pi);
        assert!(case.held());
        assert_eq!(
            case.lines(),
            ["left out: Pi has no question tool, so it has no door"]
        );
    }

    #[test]
    fn the_words_of_a_result_are_quoted_as_json_writes_them_and_cut_where_they_are_long() {
        let thread = json!({ "messages": [
            { "id": 4, "kind": "result", "body": "first" },
            { "id": 5, "kind": "result", "body": "second" },
        ] });
        assert_eq!(body_of(last_result(&thread), 3), "\"sec\"");
        // A task that ended with no result says none, as JSON writes it.
        assert_eq!(body_of(None, 300), "\"\"");
        assert_eq!(
            body_of(Some(&json!({ "body": "a \"b\"" })), 9),
            "\"a \\\"b\\\"\""
        );
    }

    #[test]
    fn a_member_runs_on_the_model_it_is_given_over_its_cheap_one() {
        let mut models = BTreeMap::new();
        assert_eq!(
            row_of(Harness::OpenCode, &models)["model"],
            "opencode/muse-spark-1.3-contributor-free"
        );
        models.insert("opencode".to_owned(), "opencode/other".to_owned());
        assert_eq!(
            row_of(Harness::OpenCode, &models)["model"],
            "opencode/other"
        );
        assert_eq!(
            row_of(Harness::Pi, &models)["model"],
            "openrouter/meta/muse-spark-1.3"
        );
    }

    #[test]
    fn the_report_names_the_model_door_model_gave_a_member_and_not_the_one_models_gives() {
        let log = Log::begin("live door test").unwrap();
        let models = BTreeMap::from([("opencode".to_owned(), "opencode/other".to_owned())]);
        // What the daemon opens the member's window on is its row, as the roster has it.
        let rig = cf_e2e::rig::Rig::start(cf_e2e::rig::Config::new(crate::FAKE_AGENT)).unwrap();
        cf_e2e::live::agents::write_roster(&rig, &[row_of(Harness::OpenCode, &models)], &log)
            .unwrap();
        rig.close().unwrap();
        assert_eq!(
            log.ran_on().as_deref(),
            Some("the windows ran on: opencode opencode/other")
        );
    }
}
