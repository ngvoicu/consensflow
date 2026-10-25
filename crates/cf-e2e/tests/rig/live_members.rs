//! What the live checks of the board's lifecycle stand on, held to what it says
//! without spending a token (the stand-in agent is the chief and the members
//! here): the project of a check, the task its chief gives its member with the
//! chief's own token, a question that goes through a hook and is judged from what
//! the daemon wrote of it, the app that is quit and started again, and a window
//! opened on a conversation the harness keeps.

use std::path::Path;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use cf_e2e::live::members::{self, find, judge, Members, Plan};
use cf_e2e::live::window::Open;
use cf_e2e::live::{agents, ask, Harness, Installed, Log};
use serde_json::json;

use cf_e2e::rig::Rig;

use crate::{after, config, rig, secs, Outcome, FAKE_AGENT, FAKE_OPENCODE};

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
    // What a check says of a window is the end of what it printed, by its pane; a pane never opened printed none.
    assert!(
        members.tail(window.id()).contains("fake agent started on"),
        "{}",
        members.tail(window.id())
    );
    assert_eq!(members.tail("no-such-pane"), "");
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
fn a_chief_that_went_is_said_to_have_gone_and_how_where_its_token_is_refused() -> Outcome {
    let log = Log::new("live members");
    let members = open("members-gone", &log)?;
    let pane = format!("p{}-chief", members.project_id());
    let frame = members.rig().open_frame(&pane, secs(30))?;
    // The chief's program is ended, as a chief that crashes is: the daemon revokes its token.
    members.rig().host(
        "pane.kill",
        json!({ "id": frame["id"], "generation": frame["generation"] }),
    )?;
    let refused = find(
        &log,
        "the chief's token to be refused",
        secs(30),
        Duration::from_millis(200),
        || Ok(members.as_chief(["task", "list"]).err()),
    )?
    .ok_or("the chief's token was never refused")?
    .to_string();
    assert!(
        refused.starts_with("cf task list: cf: this window has no ConsensFlow access"),
        "{refused}"
    );
    assert!(
        refused.contains(&format!(
            "the chief's window {pane} was opened 1 time: generation {} ended with ",
            frame["generation"]
        )),
        "{refused}"
    );
    // What the daemon wrote of it is there too, and what it warned of.
    assert!(refused.contains("the daemon wrote of it: "), "{refused}");
    // A chief nothing was said to wrote no conversation. A stand-in that is asked to leave
    // takes its status file away; Windows ends a program without asking, so its file stays.
    let status = if cfg!(windows) {
        "says idle"
    } else {
        "is gone"
    };
    assert!(
        refused.ends_with(&format!(
            "its warnings: none; its records: the conversation is gone, the status file {status}"
        )),
        "{refused}"
    );
    members.close()?;
    Ok(())
}

/// The runs of a machine share the Claude folder the stand-ins write in
/// (`projects/integration`), and a rig that ends takes the folder away when it is
/// empty (`rig::forget`: `remove_dir`). A chief hears nothing until a member says
/// something, so for a while its folder is empty, and a rig of another run that ends
/// then leaves it without. The stand-in made the folder only when it started, and
/// the first thing the chief was told after that ended it (the door check of
/// 2026-10-12: `cf answer` was refused, "this window has no ConsensFlow access", for
/// a chief that had gone). It makes its folder again where it writes, as Claude does.
#[cfg(unix)]
#[test]
fn a_chief_that_has_written_nothing_stays_when_another_rig_of_the_machine_takes_its_empty_folder(
) -> Outcome {
    use cf_e2e::rig::{Project, Rig};

    // The machine is a folder of the test's own: the rig reads its Claude from there.
    let machine = tempfile::tempdir()?;
    let shims = tempfile::tempdir()?;
    let claude = Installed::at(Harness::Claude, "/nowhere/claude", machine.path());
    let rig = Rig::start(cf_e2e::live::rig_with_stand_ins(
        &[claude],
        shims.path(),
        Path::new(FAKE_AGENT),
    )?)?;
    let work = rig.workspace().join("live-folder");
    std::fs::create_dir_all(&work)?;
    let project = Project::open(&rig, "chief", json!({ "directory": work }))?;
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    // It has written nothing: its folder is made, and empty.
    let records = machine.path().join(".claude/projects/integration");
    assert!(records.is_dir() && std::fs::read_dir(&records)?.next().is_none());

    // Another rig of the machine ends, and takes the folder, which is empty. (One daemon
    // at a time runs in a test's process: what the other rig does is done here.)
    std::fs::remove_dir(&records)?;

    // The chief hears something: it writes it, and stays.
    rig.tell(project.id(), "Reply with exactly: AGAIN")?;
    rig.wait_for("the chief to write what it heard", secs(30), || {
        Ok(!rig.exits().is_empty()
            || records
                .read_dir()
                .is_ok_and(|mut folder| folder.next().is_some()))
    })?;
    assert!(rig.exits().is_empty(), "the chief ended: {:?}", rig.exits());
    assert_eq!(project.lane("chief")?["activity"]["state"], "idle");
    rig.close()?;
    Ok(())
}

/// What a stand-in chief says of itself when it cannot write what it heard (the disk
/// is full, its folder is not its own): it ends itself, its project closes with it,
/// and the refusal of its token says how it ended.
#[test]
fn a_chief_that_cannot_write_what_it_heard_ends_itself_and_the_refusal_says_how_it_ended() -> Outcome
{
    let log = Log::new("live members");
    let members = open("members-records", &log)?;
    // Its conversation is where the stand-in cannot write: a folder in the place of the file.
    let config = members.claude_config().join("projects").join("integration");
    let session = members
        .rig()
        .started()
        .first()
        .map(|chief| chief.session.clone())
        .ok_or("the chief started nothing")?;
    std::fs::create_dir_all(config.join(format!("{session}.jsonl")))?;
    members
        .rig()
        .tell(members.project_id(), "Reply with exactly: RECORDS_OK")?;
    let refused = find(
        &log,
        "the chief's token to be refused",
        secs(60),
        Duration::from_millis(200),
        || Ok(members.as_chief(["task", "list"]).err()),
    )?
    .ok_or("the chief's token was never refused")?
    .to_string();
    assert!(
        refused.contains("ended with code 1, its screen ending: "),
        "{refused}"
    );
    assert!(refused.contains("fake-agent: "), "{refused}");
    members.close()?;
    Ok(())
}

/// A project whose worker has left a job that ignores the request to end running,
/// so that the pane host has to wait out its grace (ten seconds) to close the
/// worker's window: the project and the job.
#[cfg(unix)]
fn with_a_job_that_ignores_the_request_to_end(
    folder: &str,
    log: &Log,
) -> cf_e2e::Result<(Members, cf_e2e::rig::Job)> {
    let mut members = open(folder, log)?;
    members.add("worker", &[])?;
    members.give("worker", "BACKGROUND --deaf 120 started the job")?;
    members
        .rig()
        .wait_for("the worker's job to run", secs(60), || {
            Ok(members.rig().jobs("job").len() == 1)
        })?;
    let job = members
        .rig()
        .jobs("job")
        .into_iter()
        .next()
        .ok_or_else(|| cf_e2e::Error::Daemon("the worker's job is gone".to_owned()))?;
    Ok((members, job))
}

/// What a real harness's window needs, and a stand-in's never does: the host gives a
/// program that ignores the request to end ten seconds before it kills it, and the
/// rig's own close waits five for the host, so a project of harnesses is closed by one
/// that waits for the grace.
#[cfg(unix)]
#[test]
fn a_project_with_a_window_that_takes_the_hosts_whole_grace_to_leave_is_closed_once_the_host_has_waited(
) -> Outcome {
    let log = Log::new("live members");
    let (members, _job) = with_a_job_that_ignores_the_request_to_end("members-grace", &log)?;
    let closing = Instant::now();
    members.close()?;
    assert!(
        closing.elapsed() >= secs(9),
        "the host gave the job its grace: {:?}",
        closing.elapsed()
    );
    Ok(())
}

/// The failure of the close of a project of `folder` given four seconds from the moment the
/// worker's window has left, with the pane host held (stopped) for `held_up` just after the close
/// begins, as a loaded machine holds it: the failure, and the job the worker left.
#[cfg(unix)]
fn the_close_that_gives_up(
    folder: &str,
    held_up: Duration,
) -> cf_e2e::Result<(String, cf_e2e::rig::Job)> {
    let log = Log::new("live members");
    let (members, job) = with_a_job_that_ignores_the_request_to_end(folder, &log)?;
    // The host looks at what a window left running (two looks, two seconds apart) before it asks
    // the window to end: the window has left, and the job has not, some seconds after the close,
    // as many as the machine takes to get there, so the time the close is given is counted from
    // the window's leaving, and not from the close.
    members.rig().expect_the_close_to_fail();
    let host = members.rig().host_pid();
    let program = members
        .window_of("worker")?
        .and_then(|window| members.window_process(&window))
        .map(|program| program.pid)
        .ok_or_else(|| cf_e2e::Error::Daemon("the worker's window has no process".to_owned()))?;
    let failed = std::thread::scope(|scope| {
        scope.spawn(|| hold(host, held_up));
        members.close_after(
            "the worker's window to leave",
            |_| !cf_e2e::process::is_alive(program),
            Duration::from_secs(4),
        )
    })
    .err()
    .ok_or_else(|| cf_e2e::Error::Daemon("the close ended within its time".to_owned()))?
    .to_string();
    // What it left running is not the test's to leave.
    let _ = cf_e2e::process::signal(job.pid(), cf_e2e::process::Signal::Kill);
    Ok((failed, job))
}

/// Holds the process `pid` stopped for `held_up`, a moment after this is called (nothing, for
/// none), and lets it go on: a pid the rig started.
#[cfg(unix)]
fn hold(pid: u32, held_up: Duration) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    if held_up.is_zero() {
        return;
    }
    let host = Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
    std::thread::sleep(Duration::from_millis(300));
    let _ = kill(host, Signal::SIGSTOP);
    std::thread::sleep(held_up);
    let _ = kill(host, Signal::SIGCONT);
}

/// What the failure of a close that did not end in time says: the processes the rig started that
/// were left.
#[cfg(unix)]
fn says_what_was_left(failed: &str, job: &cf_e2e::rig::Job) -> Outcome {
    assert!(failed.starts_with("process did not exit"), "{failed}");
    let left = failed
        .split("the processes it started that were left: ")
        .nth(1)
        .ok_or(failed)?;
    // The pane host, waiting for the window to leave (the rig kills it once what was
    // left is said, and not before), the shell that runs the worker's job (its parent
    // gone, it is the machine's: `(1)`; a command is cut to a line) and the job that
    // would not, under it.
    assert!(left.contains("consensflow-bridge"), "{failed}");
    assert!(left.contains("(1) ") && left.contains("sh -c"), "{failed}");
    assert!(
        left.contains(&format!("{} (", job.pid())),
        "the job {} is left: {failed}",
        job.pid()
    );
    assert!(left.contains("fake-agent --background-jo"), "{failed}");
    Ok(())
}

/// A close a case begins (the app's quit: the daemon's input and the pane host's end) and does not
/// wait for, so that it can wait for an event of the close first: the daemon ends, with no wait of
/// the case's own.
#[test]
fn a_close_begun_ends_the_daemon_without_the_case_waiting_for_it() -> Outcome {
    let mut running = rig()?;
    running.begin_close();
    running.wait_for_daemon_exit(secs(20))?;
    running.close()?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_project_that_does_not_end_in_time_says_which_of_the_processes_it_started_were_left() -> Outcome
{
    let (failed, job) = the_close_that_gives_up("members-left", Duration::ZERO)?;
    says_what_was_left(&failed, &job)
}

/// A gate under load holds the pane host: its looks at the window's tree, two seconds apart,
/// and the request to end are all later, and the window left no sooner than the close's four
/// seconds were gone (T-118: the margin was the one the host takes on a machine at rest).
#[cfg(unix)]
#[test]
fn a_close_says_which_were_left_though_the_pane_host_was_held_up_before_it_asked_the_window_to_end(
) -> Outcome {
    let (failed, job) = the_close_that_gives_up("members-held-up", Duration::from_secs(5))?;
    says_what_was_left(&failed, &job)
}

#[test]
fn the_app_started_again_on_the_same_home_leaves_the_project_suspended_and_its_resume_opens_the_chief_a_window_of_its_own(
) -> Outcome {
    let log = Log::new("live members");
    let members = open("members-restart", &log)?;
    let pane = format!("p{}-chief", members.project_id());
    let first = members.rig().open_frame(&pane, secs(30))?;
    let session = after(&first, "--session-id");
    assert!(!session.is_empty(), "{:?}", first["argv"]);
    let project = members.project_id();
    // A chief that has a conversation: the human's Resume of the project, after the app started
    // again, opens it on that conversation, and a chief that never had a turn is opened afresh, on
    // a conversation of its own.
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
    // The app started again opens nothing: the project waits for the human.
    assert!(!members.is_open()?);
    assert!(members.rig().open_frames().is_empty());
    members.resume()?;
    find(
        &log,
        "the project to be open again",
        secs(60),
        Duration::from_millis(200),
        || Ok(members.is_open()?.then_some(())),
    )?
    .ok_or("the project never came back")?;
    // The Resume brings the chief's own conversation back, in a window of its own.
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
    let log = Log::begin("live members")?;
    assert_eq!(log.ran_on().as_deref(), Some("no window was launched"));
    let first = Open::new(&rig, &install, "resume", "resume-first")
        .without_trust()
        .open(&log)?;
    // What the window was launched on is the run's to report: the harness's model.
    assert_eq!(
        log.ran_on().as_deref(),
        Some("the windows ran on: claude claude-sonnet-5-5")
    );
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
        .open(&log)?;
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

#[test]
fn a_window_opened_without_the_daemon_is_never_opened_on_an_opencode_effort_it_would_not_get(
) -> Outcome {
    let rig = rig()?;
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::OpenCode, FAKE_AGENT, &home);
    let log = Log::begin("live members")?;
    let refused = Open::new(&rig, &install, "effort", "effort-opencode")
        .agent(json!({ "kind": "opencode", "model": "opencode/x", "effort": "high" }))
        .without_trust()
        .open(&log)
        .err()
        .map(|failed| failed.to_string());
    assert_eq!(
        refused.as_deref(),
        Some(
            "an OpenCode window opened without the daemon is given no effort: OpenCode takes \
             it as the variant of the first message the daemon sends to the window's server, \
             which this window has not; run opencode=opencode/x with no :high"
        )
    );
    // Nothing was opened, and the run says so.
    assert!(rig.open_frames().is_empty(), "{:?}", rig.open_frames());
    assert_eq!(log.ran_on().as_deref(), Some("no window was launched"));
    rig.close()?;
    Ok(())
}

/// A model of OpenCode's that the catalog has no row of, which `--models` names at
/// an effort. (It was the owner's Nemotron 3.5 Super VL until the catalog took it
/// as Buri, 2026-10-16; the case holds that it is in no row of the catalog.)
const MODEL: &str = "opencode/stand-in-no-catalog-row-free";
const MODEL_ID: &str = "stand-in-no-catalog-row-free";

/// The daemon's launch of a member's OpenCode window, as the stand-in OpenCode
/// was asked it: the launch on a conversation (`--session`).
fn window_launch(rig: &Rig) -> Option<Vec<String>> {
    rig.opencode_asked()
        .into_iter()
        .find(|args| args.iter().any(|word| word == "--session"))
}

/// The first message the daemon posted to an OpenCode window's server, as its body.
fn first_message(rig: &Rig) -> Option<serde_json::Value> {
    rig.opencode_requests()
        .into_iter()
        .find(|request| {
            request["target"]
                .as_str()
                .is_some_and(|target| target.contains("/prompt_async"))
        })
        .and_then(|request| serde_json::from_str(request["body"].as_str()?).ok())
}

#[test]
fn a_task_for_a_member_on_a_model_the_flag_names_and_the_catalog_has_not_is_launched_on_it(
) -> Outcome {
    let presets = std::fs::read_to_string(cf_e2e::checkout::path(
        "crates/cf-catalog/data/presets.json",
    ))?;
    assert!(
        !presets.contains(&format!("\"{MODEL}\"")),
        "the catalog has {MODEL}: the case needs a model it has not"
    );
    let models = agents::parse(Some(&format!("opencode={MODEL}:high")), None)?;
    let row = agents::row_on(models.of(Harness::OpenCode), "light");
    // The daemon takes the row as the live suites write it: a member of the human's
    // own, which needs a model and no catalog entry; the run's report says it.
    let rig = Rig::start(config().opencode(FAKE_OPENCODE))?;
    let log = Log::begin("live members")?;
    agents::write_roster(&rig, &[members::stand_in_chief(), row], &log)?;
    assert_eq!(
        log.ran_on(),
        Some(format!("the windows ran on: opencode {MODEL}, effort high"))
    );
    let project = cf_e2e::rig::Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member(&agents::id(Harness::OpenCode))?;
    assert_eq!(added["member"]["agent"], "live-opencode", "{added}");
    rig.tell(
        project.id(),
        "DISPATCH --tier light Reply with exactly: UNCATALOGUED",
    )?;
    // The window the daemon launched for the task: on that model.
    rig.wait_for("the daemon to launch the member's window", secs(30), || {
        Ok(window_launch(&rig).is_some())
    })?;
    let launch = window_launch(&rig).unwrap_or_default();
    assert!(
        launch.windows(2).any(|pair| pair == ["--model", MODEL]),
        "{launch:?}"
    );
    // Its first message, which carries the model and the effort OpenCode runs it on.
    rig.wait_for(
        "the daemon to post the task to the window",
        secs(30),
        || Ok(first_message(&rig).is_some()),
    )?;
    let message = first_message(&rig).unwrap_or_default();
    assert_eq!(
        message["model"],
        json!({ "providerID": "opencode", "modelID": MODEL_ID }),
        "{message}"
    );
    assert_eq!(message["variant"], "high", "{message}");
    rig.close()?;
    Ok(())
}
