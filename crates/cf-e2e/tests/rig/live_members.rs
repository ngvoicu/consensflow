//! What the live checks of the board's lifecycle stand on, held to what it says
//! without spending a token (the stand-in agent is the chief and the members
//! here): the project of a check, the task its chief gives its member with the
//! chief's own token, a question that goes through a hook and is judged from what
//! the daemon wrote of it, the app that is quit and started again, and a window
//! opened on a conversation the harness keeps.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{self, find, judge, Members, Plan};
use cf_e2e::live::window::Open;
use cf_e2e::live::{agents, ask, Harness, Installed, Log};
use serde_json::json;

use crate::{after, rig, secs, Outcome, FAKE_AGENT};

/// A project of a check whose chief and member are the stand-in agent, in the
/// folder `folder` of the live folders of the rig's own home.
fn open(folder: &str, log: &Log) -> cf_e2e::Result<Members> {
    let plan = Plan::new(folder, members::stand_in_chief()).with(members::stand_in_worker());
    Members::open(&plan, Path::new(FAKE_AGENT), log)
}

/// What a look at the board finds of the task `number`, once its state is `state`.
fn reaches(members: &Members, number: i64, state: &[&str]) -> cf_e2e::Result<Option<()>> {
    let now = members.task(number)?["state"].clone();
    Ok(state.iter().any(|wanted| now == *wanted).then_some(()))
}

#[test]
fn a_project_opens_with_the_stand_in_for_its_chief_and_the_chief_gives_its_member_a_task() -> Outcome
{
    let log = Log::new("live members");
    let mut members = open("members-task", &log)?;
    // No real Claude runs here, so none is asked to trust the folder; the folder is the plan's own.
    assert_eq!(members.trust(), None);
    assert_eq!(members.chief(), "chief");
    let folder = members.folder().to_path_buf();
    assert!(folder.ends_with(".consensflow-candidate/live/members-task") || cfg!(windows));
    for name in ["AGENTS.md", "CLAUDE.md"] {
        assert_eq!(
            std::fs::read_to_string(folder.join(name))?,
            members::BRIEF,
            "{name}"
        );
    }
    assert!(
        members.window_of("worker")?.is_none(),
        "no window before a task"
    );

    let tier = members.add("worker", &[])?;
    assert!(!tier.is_empty());
    assert_eq!(members.tier_of("worker"), Some(tier.as_str()));
    let refused = members.add("nobody", &[]).unwrap_err();
    assert!(
        refused.to_string().starts_with("member.add nobody: "),
        "{refused}"
    );

    let number = members.give("worker", "SLEEP 4 Reply with exactly: MEMBERS_OK")?;
    assert_eq!(number, 1);
    let window = find(
        &log,
        "the member's window",
        secs(60),
        Duration::from_millis(200),
        || members.window_of("worker"),
    )?
    .ok_or("the member never had a window")?;
    assert!(window.handle.starts_with("worker"), "{}", window.handle);
    assert_eq!(
        window.id(),
        format!("p{}-{}", members.project_id(), window.handle)
    );
    let open = window.open.clone().ok_or("the daemon opened no window")?;
    assert_eq!(window.session(), Some(after(&open, "--session-id")));
    assert_eq!(window.activity, window.lane["activity"]);

    find(
        &log,
        "the task to be done",
        secs(60),
        Duration::from_millis(200),
        || reaches(&members, number, &["done", "accepted"]),
    )?
    .ok_or("the task was never done")?;
    let thread = members.task(number)?;
    let result = thread["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|message| message["kind"] == "result"))
        .ok_or("the thread holds no result")?;
    assert_eq!(result["body"], "MEMBERS_OK");
    // The human has no note about a stop that never was.
    assert!(members.inbox(None)?.is_empty());
    assert!(members.is_open()?);
    let moved = members::moves(&members.events(), number);
    assert_eq!(moved.last().map(String::as_str), Some("done"), "{moved:?}");
    members.close()?;
    Ok(())
}

#[test]
fn a_question_through_the_hook_is_answered_as_the_chief_and_judged_received_at_its_door() -> Outcome
{
    let log = Log::new("live members");
    let mut members = open("members-door", &log)?;
    members.add("worker", &[])?;
    let number = members.give("worker", &members::stand_in_question_brief())?;
    let question = find(
        &log,
        "the member's question",
        secs(60),
        Duration::from_millis(200),
        || {
            Ok(members.task(number)?["messages"]
                .as_array()
                .and_then(|messages| {
                    messages
                        .iter()
                        .find(|message| message["kind"] == "question")
                })
                .cloned())
        },
    )?
    .ok_or("the member never asked")?;
    let asked = format!("m-{}", question["id"]);
    let answered = members.as_chief(["answer", asked.as_str(), "blue"])?;
    assert_eq!(answered.code, Some(0), "{answered}");
    find(
        &log,
        "the task to end",
        secs(60),
        Duration::from_millis(200),
        || {
            reaches(
                &members,
                number,
                &["done", "accepted", "failed", "cancelled"],
            )
        },
    )?
    .ok_or("the task never ended")?;

    // What the daemon wrote of it, judged as the door check judges it.
    let thread = members.task(number)?;
    let answer = thread["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|message| message["kind"] == "answer"))
        .cloned()
        .ok_or("the thread holds no answer")?;
    let judged = judge(&members.events(), &thread, &question, &answer, "blue");
    for finding in &judged.findings {
        assert!(finding.ok, "{}: {}", finding.name, finding.detail);
    }
    assert!(judged.received_at.is_some());
    assert!(judged.notes.is_empty(), "{:?}", judged.notes);
    members.close()?;
    Ok(())
}

#[test]
fn the_app_started_again_on_the_same_home_resumes_the_project_and_the_chief_is_a_window_of_its_own(
) -> Outcome {
    let log = Log::new("live members");
    let members = open("members-restart", &log)?;
    let pane = format!("p{}-chief", members.project_id());
    let first = members.rig().open_frame(&pane, secs(30))?;
    let session = after(&first, "--session-id");
    assert!(!session.is_empty(), "{:?}", first["argv"]);
    let project = members.project_id();
    // A chief that has a conversation: the daemon that starts again resumes it, and a chief that
    // never said a word is opened afresh on the id it had.
    members
        .rig()
        .tell(project, "Reply with exactly: BEFORE-RESTART")?;
    members
        .rig()
        .wait_for("the chief to answer", secs(30), || {
            Ok(members
                .rig()
                .transcript(&session)
                .contains("\"text\":\"BEFORE-RESTART\""))
        })?;

    let members = members.restart(&log)?;
    assert_eq!(members.project_id(), project);
    find(
        &log,
        "the project to be open again",
        secs(60),
        Duration::from_millis(200),
        || Ok(members.is_open()?.then_some(())),
    )?
    .ok_or("the project never came back")?;
    // The daemon that starts resumes the chief's own conversation, in a window of its own.
    let again = members.rig().open_frame(&pane, secs(60))?;
    assert_eq!(after(&again, "--resume"), session, "{:?}", again["argv"]);
    // The chief's token is the new window's: `cf` runs as it, and it is the newest of its pane.
    let listed = members.as_chief(["task", "list"])?;
    assert_eq!(listed.code, Some(0), "{listed}");
    members.close()?;
    Ok(())
}

#[test]
fn a_window_is_opened_on_a_conversation_the_harness_keeps_as_the_daemon_opens_one_after_a_quit(
) -> Outcome {
    let rig = rig()?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
    let first = Open::new(&rig, &install, "resume", "resume-first")
        .without_trust()
        .open()?;
    assert!(first.still(secs(60)), "{}", first.screen());
    let session = first
        .session()
        .ok_or("a new Claude window is opened on an id")?;
    assert!(
        first
            .send(&first.given("Reply with exactly: ONE")?, "ONE")?
            .shown
    );
    first.kill();
    rig.wait_for("the first window to be gone", secs(30), || {
        Ok(first.closed())
    })?;

    let again = Open::new(&rig, &install, "resume", "resume-again")
        .without_trust()
        .resume(&session)
        .open()?;
    // It is opened on `--resume` of that conversation, and not on an id of its own.
    assert_eq!(again.session().as_deref(), Some(session.as_str()));
    let argv = again.argv();
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--resume", session.as_str()]),
        "{argv:?}"
    );
    assert!(!argv.iter().any(|word| word == "--session-id"), "{argv:?}");
    assert!(again.still(secs(60)), "{}", again.screen());
    drop(first);
    drop(again);
    rig.close()?;
    Ok(())
}

#[test]
fn the_harness_code_of_the_product_opens_each_harness_on_a_kept_conversation_by_its_own_words(
) -> Outcome {
    ask::prepare()?;
    let words =
        |harness: Harness, session: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
            Ok(ask::resume(&agents::agent(harness), session)?
                .ok_or("the harness has a window of its own")?
                .args)
        };
    let pair = |args: &[String], first: &str, second: &str| {
        args.windows(2).any(|pair| pair == [first, second])
    };
    assert!(pair(&words(Harness::Claude, "s-1")?, "--resume", "s-1"));
    assert!(pair(&words(Harness::Codex, "t-1")?, "resume", "t-1"));
    // Pi creates the session the first time and resumes it after: one flag for both.
    assert!(pair(&words(Harness::Pi, "s-2")?, "--session-id", "s-2"));
    // A resumed window opens on the agent's model, as a start does: one without ran on the default.
    assert!(pair(
        &words(Harness::Claude, "s-3")?,
        "--model",
        "claude-sonnet-5-5"
    ));
    // A kind with no window of its own is none.
    assert_eq!(ask::resume(&json!({ "kind": "kimi" }), "s")?, None);
    Ok(())
}
