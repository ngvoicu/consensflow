//! Live: a worker's task that was stopped (paused, or its project closed) and then
//! resumed goes back to work, on the harness named. It is opt-in
//! (`cargo xtask live task-resumed`): it spends the machine's quota on a task and a
//! resume of each harness named (Devin, the default), and needs each installed and
//! logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The report it answers (T-1107, 2026-11-02, ConsensFlow 3.0.7-alpha.1, Windows): a task
//! given to a Devin member and then resumed stays "queued" and never moves to working.
//! The task moves to working when its window has received the words that resume it
//! (`delivery.confirmed`), so the board reads "queued" for as long as those words are
//! not in the window.
//!
//! The run is a real harness as the one member of a project whose chief is the stand-in.
//! The check, as the chief or the human, gives it a task that runs a long command, waits
//! for the command, and stops the task in one of the shapes below. The task is then
//! resumed and looked at every quarter of a second: it must read "working" within
//! [`GOES_ON`] of the resume, and then end with a result. What the run says of every
//! shape is the board's series for the task, each `delivery.*` event the daemon wrote
//! after the resume, the window's lane and the end of its screen: enough to say which
//! link broke when one does.
//!
//! - `cut`          the window is ended inside its command (the app's quit does that, and a
//!   window that closes by itself); the daemon pauses the task; the chief resumes it with words
//! - `early`        the task is paused soon after it is given (`--at-ms 0,1000,…` says how soon),
//!   before its window is up or has shown a word; the chief resumes it with words
//! - `pause-chief`  the human pauses the task; the chief resumes it with words
//!   (`cf task resume T-n "…"`)
//! - `pause-human`  the human pauses the task and resumes it (`task.resume`, whose words
//!   are always "Go on where you stopped.")
//! - `close`        the human closes the project and resumes it (`project.close`,
//!   `project.resume`): the task the Close paused goes on with the project
//! - `pause-close`  the task is paused, the project closed and resumed (the task stays
//!   paused), and then the chief resumes the task with words
//! - `tell`         the chief tells the window (`cf tell T-n "…"`, which pauses the task and puts
//!   the words to its window); the window answers and is told to go on with its work in the same
//!   turn; the chief resumes the task as soon as it has the answer (T-1139, field bug 8). A
//!   harness that keeps working after the tell has its words wait behind the turn, and the
//!   task must read working while they wait; one that stops after the answer is given the words
//!   at rest as in the other shapes. What each did is in the run's report.
//!
//! ```text
//! cargo xtask live task-resumed                       Devin, every shape
//! cargo xtask live task-resumed devin --shape close
//! cargo xtask live task-resumed devin --lead          a task is done first: the window is not the first
//! cargo xtask live task-resumed devin --times 5       each shape five times
//! cargo xtask live task-resumed -- --list             lists this test
//! ```

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{
    find, is_over, last_result, member, named, stand_in_chief, Account, LongCommand, Members, Plan,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::trace::at_ms;
use cf_e2e::live::{self, cut, text, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::json;

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The command the window is asked to run: long enough to be stopped inside.
const LONG: LongCommand = LongCommand::lasting("cf-task-resumed-long-command", 75);

/// How long a window has to reach its command, a lead task to end, and a stop to take hold.
const WORKS: Duration = Duration::from_secs(240);
const STOPS: Duration = Duration::from_secs(60);
/// How long, from the resume, the task has to read "working": a resume that has the
/// words go into the window is a few seconds' work, and a lost Enter costs ten more.
const GOES_ON: Duration = Duration::from_secs(120);
/// How long the resumed task has to end with its result.
const ENDS: Duration = Duration::from_secs(240);
/// How long the chief's window, opened again by a Resume, has to be up for its `cf`.
const CHIEF_UP: Duration = Duration::from_secs(90);
/// How often the board is looked at.
const LOOK: Duration = Duration::from_millis(250);
/// How long the window has to answer a tell.
const ANSWERS: Duration = Duration::from_secs(120);
/// A short command the window is told to run, again and again, in the turn of the tell: its
/// record grows with each (T-1139).
const SHORT: LongCommand = LongCommand::lasting("cf-task-resumed-short-command", 8);

/// The first look, among `looks` (what the board read of the task, and how long after the
/// resume), that reads it `working` within `within` of the resume: the moment the task went
/// on. None where it never did, or only after the time it had.
fn first_working(looks: &[(Duration, String)], within: Duration) -> Option<Duration> {
    looks
        .iter()
        .find(|(at, state)| state == "working" && *at <= within)
        .map(|(at, _)| *at)
}

/// The moments, in milliseconds after the task is given, at which the `early` shape pauses
/// it: those `--at-ms` names (a word that is no number is left out), else six that fall in
/// the window's start, at its first word and at work.
fn moments(asked: &[String]) -> Vec<u64> {
    if asked.is_empty() {
        vec![0, 1000, 3000, 6000, 10_000, 20_000]
    } else {
        asked.iter().filter_map(|ms| ms.parse().ok()).collect()
    }
}

/// What the worker is first asked: the long command, and a word to answer with.
fn first() -> String {
    format!(
        "Run this exact command with your shell tool and wait for it to finish:\n{}\nThen reply with exactly one line: RESUMED_FIRST_DONE",
        LONG.text()
    )
}

/// What the lead task asks: a word, and nothing else.
const LEAD: &str = "Reply with exactly one line: RESUMED_LEAD_DONE";

/// The chief's tell: the answer, and the work to go on with in the same turn.
fn tell_words() -> String {
    format!(
        "Stop what you are doing and answer this question with the cf answer command, with exactly the word ok. Then, without waiting for anything, go on with your work: run this command six times, one after the other, each in a tool call of its own (never chained in one):\n{}\nThen reply with exactly one line: TELL_TURN_DONE",
        SHORT.text()
    )
}

/// The chief's words that resume the task.
const WORDS: &str =
    "Stop the command if it is still running, and reply with exactly one line: RESUMED_DONE";

/// What the run knows of one shape: the task, and where the board's looks begin.
struct Run<'a> {
    members: &'a Members,
    agent: &'a str,
    log: &'a Log,
    /// For the `early` shape: how long after the task is given the pause is made.
    at: Duration,
    /// When the shape began, on the daemon's clock: what it says of a case that failed
    /// begins here.
    begun_ms: i64,
}

/// The task given and the worker inside its long command: the task's number, and the
/// worker's handle.
fn inside_the_command(run: &Run<'_>) -> Result<(i64, String)> {
    let number = run.members.give(run.agent, &first())?;
    run.log
        .say(format!("T-{number} is given: the long command"));
    let started = Instant::now();
    loop {
        let lane = run.members.lane(run.agent)?;
        if text(&lane["activity"]["state"]) == "working" && !LONG.running()?.is_empty() {
            return Ok((number, text(&lane["participant"]["handle"])));
        }
        if started.elapsed() > WORKS {
            return Err(Error::Daemon(format!(
                "T-{number} did not reach its command within {WORKS:?}: it is {}, the lane {}",
                text(&run.members.task(number)?["state"]),
                text(&lane["activity"]["state"])
            )));
        }
        sleep(LOOK);
    }
}

/// The task, until it reads `state` or `within` is up: the moment it did.
fn becomes(run: &Run<'_>, number: i64, state: &str, within: Duration) -> Result<Option<Duration>> {
    let started = Instant::now();
    loop {
        if text(&run.members.task(number)?["state"]) == state {
            return Ok(Some(started.elapsed()));
        }
        if started.elapsed() >= within {
            return Ok(None);
        }
        sleep(LOOK);
    }
}

/// The human's pause of the task, and what came of the window: whether the task reads
/// paused and when the window's lane reads at rest.
fn paused(run: &Run<'_>, number: i64, handle: &str, case: &mut Account) -> Result<()> {
    let pause = run.members.pause(number)?;
    case.say(format!(
        "task.pause: {}",
        if pause.answer["ok"] == true {
            format!("T-{number} is {}", text(&pause.answer["task"]["state"]))
        } else {
            format!("refused: {}", pause.answer)
        }
    ));
    let paused = becomes(run, number, "paused", STOPS)?;
    case.check(
        "the task reads paused",
        paused.is_some(),
        format!("{paused:?}"),
    );
    // The window's stop is paid when its lane reads at rest; it may not within the time.
    let started = Instant::now();
    let mut at_rest = None;
    while started.elapsed() < STOPS && at_rest.is_none() {
        if text(&run.members.lane(handle)?["activity"]["state"]) == "idle" {
            at_rest = Some(started.elapsed());
        }
        sleep(LOOK);
    }
    let running = !LONG.running()?.is_empty();
    case.say(format!(
        "the window's lane read at rest {}",
        at_rest.map_or_else(
            || format!("never in {STOPS:?}; the command runs: {running}"),
            |at| format!("{:.1} s after the task read paused", at.as_secs_f64())
        )
    ));
    Ok(())
}

/// The last task message of the thread whose words begin `Resumed:`: the words a resume of
/// the task queued for its window.
fn resume_words_of(thread: &serde_json::Value) -> Option<i64> {
    thread["messages"]
        .as_array()?
        .iter()
        .rfind(|message| {
            message["kind"] == "task"
                && message["body"]
                    .as_str()
                    .is_some_and(|body| body.starts_with("Resumed:"))
        })
        .and_then(|message| message["id"].as_i64())
}

/// Whether `event` is about message `words`: any event, where the resume queued no words.
fn is_of(event: &serde_json::Value, words: Option<i64>) -> bool {
    words.is_none_or(|words| event["data"]["message"].as_i64() == Some(words))
}

/// The events the daemon wrote since `since_ms` about deliveries, as lines.
fn deliveries_since(members: &Members, since_ms: i64) -> Vec<String> {
    members
        .events()
        .iter()
        .filter(|event| {
            text(&event["kind"]).starts_with("delivery.")
                && at_ms(event).is_some_and(|at| at >= since_ms)
        })
        .map(|event| {
            format!(
                "+{:.1} s {} {}",
                (at_ms(event).unwrap_or(since_ms) - since_ms) as f64 / 1000.0,
                text(&event["kind"]),
                format!("{}{}", event["data"], held_reason(event))
            )
        })
        .collect()
}

/// The task looked at from the resume (`since`, `since_ms`): when it first read
/// "working", the series of what it read, and what the daemon said of the deliveries.
///
/// Returns when the task first read working while the words that resumed it had not been
/// confirmed in the window, if it did: a window at work on it when it was resumed is
/// the case where it must (T-1139).
fn goes_on(
    run: &Run<'_>,
    number: i64,
    handle: &str,
    since: Instant,
    since_ms: i64,
    case: &mut Account,
) -> Result<Option<Duration>> {
    let mut series: Vec<String> = Vec::new();
    let mut working_while_waiting: Option<Duration> = None;
    // The words the resume queued, where it queued some: it is their confirmation that the
    // window took them, not a delivery to anyone else (the chief is given the answer to a tell
    // and the note of a task that moved on meanwhile).
    let resume_words = resume_words_of(&run.members.task(number)?);
    let mut last = String::new();
    let mut looks: Vec<(Duration, String)> = Vec::new();
    let mut confirmed: Option<Duration> = None;
    while since.elapsed() < GOES_ON
        && (first_working(&looks, GOES_ON).is_none() || confirmed.is_none())
    {
        let state = text(&run.members.task(number)?["state"]);
        let lane = text(&run.members.lane(handle)?["activity"]["state"]);
        let line = format!("T-{number} {state}, the lane {lane}");
        if line != last {
            series.push(format!("+{:.2} s {line}", since.elapsed().as_secs_f64()));
            last = line;
        }
        looks.push((since.elapsed(), state));
        if confirmed.is_none()
            && run.members.events().iter().any(|event| {
                event["kind"] == "delivery.confirmed"
                    && at_ms(event).is_some_and(|at| at >= since_ms)
                    && is_of(event, resume_words)
            })
        {
            confirmed = Some(since.elapsed());
        }
        // The state was read before the events: working with no confirmation after it was
        // working with the words still waiting.
        if working_while_waiting.is_none()
            && confirmed.is_none()
            && looks.last().is_some_and(|(_, state)| state == "working")
        {
            working_while_waiting = Some(since.elapsed());
        }
        sleep(LOOK);
    }
    let working = first_working(&looks, GOES_ON);
    case.say(series.join("\n"));
    let events = deliveries_since(run.members, since_ms);
    case.say(format!(
        "the daemon's deliveries since the resume:\n{}",
        events.join("\n")
    ));
    case.check(
        "after the resume, the task reads working",
        working.is_some(),
        match working {
            Some(at) => format!("working {:.1} s after the resume", at.as_secs_f64()),
            None => format!(
                "still {} {GOES_ON:?} after it",
                text(&run.members.task(number)?["state"])
            ),
        },
    );
    case.check(
        "and the daemon confirmed the words that resumed it in the window",
        confirmed.is_some(),
        match confirmed {
            Some(at) => format!(
                "delivery.confirmed {:.1} s after the resume",
                at.as_secs_f64()
            ),
            None => format!("no delivery.confirmed within {GOES_ON:?} of the resume"),
        },
    );
    if working.is_none() || confirmed.is_none() {
        if let Some(window) = run.members.window_of(handle)? {
            case.say(format!(
                "the window {} as drawn: {}\nits lane: {}",
                window.id(),
                run.members.tail(window.id()),
                window.lane
            ));
        } else {
            case.say(format!(
                "the worker has no window; its lane: {}",
                run.members.lane(handle)?
            ));
        }
        let thread = run.members.task(number)?;
        case.say(format!(
            "the thread of T-{number}: {}",
            thread["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|message| format!(
                    "m-{} {} {}",
                    message["id"],
                    text(&message["kind"]),
                    text(&message["state"])
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    Ok(working_while_waiting)
}

/// The resumed task ends with a result.
fn ends(run: &Run<'_>, number: i64, case: &mut Account) -> Result<()> {
    let over = find(
        run.log,
        &format!("T-{number} to end"),
        ENDS,
        Duration::from_secs(1),
        || {
            let thread = run.members.task(number)?;
            Ok(is_over(&thread).then(|| text(&thread["state"])))
        },
    )?;
    let thread = run.members.task(number)?;
    let done = over
        .as_deref()
        .is_some_and(|state| ["done", "accepted"].contains(&state))
        && last_result(&thread).is_some();
    case.check(
        "the resumed task ends with its result",
        done,
        format!("T-{number} is {}", text(&thread["state"])),
    );
    if !done {
        say_where_it_stands(run, number, case)?;
    }
    Ok(())
}

/// What a task that did not go on stands on: its thread, its worker's lane and windows,
/// the end of the window's screen, and every event of the shape.
fn say_where_it_stands(run: &Run<'_>, number: i64, case: &mut Account) -> Result<()> {
    let thread = run.members.task(number)?;
    let lane = run.members.lane(run.agent)?;
    case.say(format!(
        "T-{number} is {}; its thread: {}; the worker's lane: activity {}, pane {}; windows opened for it: {}",
        text(&thread["state"]),
        messages(&thread),
        lane["activity"],
        if lane["pane"].is_null() { "none".to_owned() } else { text(&lane["pane"]["id"]) },
        run.members.windows_opened_for(run.agent).len()
    ));
    if let Some(window) = run.members.window_in(lane) {
        case.say(format!(
            "the window as drawn:\n{}",
            run.members.tail(window.id())
        ));
    }
    say_events(run.members, run.begun_ms, case);
    Ok(())
}

/// What a run leaves on the machine is ended: the command is the window's.
fn tidy(run: &Run<'_>, handle: &str) {
    if let Ok(Some(window)) = run.members.window_of(handle) {
        if let Some(own) = run.members.window_process(&window) {
            let _ = LONG.end_in(own);
        }
    }
}

/// The chief's resume of the task with `WORDS`.
fn chief_resumes(run: &Run<'_>, number: i64, case: &mut Account) -> Result<(Instant, i64)> {
    // The chief's window may be the one a Resume is still opening: its `cf` has no access
    // until it is up.
    let started = Instant::now();
    let (since, since_ms, ran) = loop {
        let (since, since_ms) = (Instant::now(), jiff::Timestamp::now().as_millisecond());
        match run
            .members
            .as_chief(["task", "resume", &format!("T-{number}"), WORDS])
        {
            Ok(ran) => break (since, since_ms, ran),
            Err(failed) if started.elapsed() < CHIEF_UP => {
                run.log.say(format!(
                    "the chief's cf is not up yet: {}",
                    cut(&failed.to_string(), 120)
                ));
                sleep(Duration::from_secs(2));
            }
            Err(failed) => return Err(failed),
        }
    };
    case.say(format!(
        "cf task resume T-{number}: exit {:?}, {}",
        ran.code,
        ran.stdout.trim()
    ));
    case.require(ran.code == Some(0));
    Ok((since, since_ms))
}

/// The human's resume of the task.
fn human_resumes(run: &Run<'_>, number: i64, case: &mut Account) -> Result<(Instant, i64)> {
    let (since, since_ms) = (Instant::now(), jiff::Timestamp::now().as_millisecond());
    let resumed = run.members.page("task.resume", json!({ "task": number }))?;
    case.say(format!(
        "task.resume: {}",
        if resumed["ok"] == true {
            format!("T-{number} is {}", text(&resumed["task"]["state"]))
        } else {
            format!("refused: {resumed}")
        }
    ));
    case.require(resumed["ok"] == true);
    Ok((since, since_ms))
}

/// The human's Close of the project, until the task reads paused (a Close pauses the
/// task its windows had).
fn closes(run: &Run<'_>, number: i64, case: &mut Account) -> Result<()> {
    let closed = run.members.page("project.close", json!({}))?;
    case.say(format!("project.close: {}", closed["ok"]));
    let state = becomes(run, number, "paused", STOPS)?;
    case.say(format!(
        "after the Close T-{number} reads {}",
        text(&run.members.task(number)?["state"])
    ));
    case.check(
        "the Close paused the task",
        state.is_some(),
        format!("{state:?}"),
    );
    Ok(())
}

/// The shapes a stopped task is resumed in, and how each is run.
type Shape = fn(&Run<'_>, &mut Account) -> Result<()>;

fn pause_chief(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let (number, handle) = inside_the_command(run)?;
    paused(run, number, &handle, case)?;
    let (since, since_ms) = chief_resumes(run, number, case)?;
    goes_on(run, number, &handle, since, since_ms, case)?;
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

fn pause_human(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let (number, handle) = inside_the_command(run)?;
    paused(run, number, &handle, case)?;
    let (since, since_ms) = human_resumes(run, number, case)?;
    goes_on(run, number, &handle, since, since_ms, case)?;
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

fn close(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let (number, handle) = inside_the_command(run)?;
    tidy(run, &handle);
    closes(run, number, case)?;
    let (since, since_ms) = (Instant::now(), jiff::Timestamp::now().as_millisecond());
    run.members.resume()?;
    case.say("project.resume made");
    goes_on(run, number, &handle, since, since_ms, case)?;
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

fn pause_close(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let (number, handle) = inside_the_command(run)?;
    paused(run, number, &handle, case)?;
    tidy(run, &handle);
    closes(run, number, case)?;
    run.members.resume()?;
    case.say(format!(
        "after project.resume T-{number} reads {}",
        text(&run.members.task(number)?["state"])
    ));
    let (since, since_ms) = chief_resumes(run, number, case)?;
    goes_on(run, number, &handle, since, since_ms, case)?;
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

/// The task given and paused `run.at` after: the window may not be up, or may be up with
/// nothing yet shown. Resumed by the chief with words.
fn early(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let given_ms = jiff::Timestamp::now().as_millisecond();
    let number = run.members.give(run.agent, &first())?;
    run.log
        .say(format!("T-{number} is given; paused in {:?}", run.at));
    sleep(run.at);
    let handle = text(&run.members.lane(run.agent)?["participant"]["handle"]);
    let thread = run.members.task(number)?;
    case.say(format!(
        "when T-{number} is paused it is {}, its messages: {}; the worker has {} window(s) opened, its lane {}",
        text(&thread["state"]),
        messages(&thread),
        run.members.windows_opened_for(run.agent).len(),
        text(&run.members.lane(run.agent)?["activity"]["state"])
    ));
    let pause = run.members.pause(number)?;
    case.say(format!(
        "task.pause: {}",
        if pause.answer["ok"] == true {
            format!("T-{number} is {}", text(&pause.answer["task"]["state"]))
        } else {
            format!("refused: {}", pause.answer)
        }
    ));
    case.require(pause.answer["ok"] == true);
    let at_rest = becomes(run, number, "paused", STOPS)?;
    case.check(
        "the task reads paused",
        at_rest.is_some(),
        format!("{at_rest:?}"),
    );
    // The window's stop, given time to be paid: it may have no window to stop.
    sleep(Duration::from_secs(5));
    case.say(format!(
        "five seconds after the pause: the lane {}, the thread {}",
        text(&run.members.lane(run.agent)?["activity"]["state"]),
        messages(&run.members.task(number)?)
    ));
    let (since, since_ms) = chief_resumes(run, number, case)?;
    goes_on(run, number, &handle, since, since_ms, case)?;
    if !case.held() {
        say_events(run.members, given_ms, case);
    }
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

/// The window ended while it is inside its command, as the app's quit ends it and as a
/// window that closes by itself does (`pane.kill`), and the chief's resume of the task
/// the daemon then holds: a window opened again on the conversation.
fn window_cut(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let given_ms = jiff::Timestamp::now().as_millisecond();
    let (number, handle) = inside_the_command(run)?;
    let window = run
        .members
        .window_of(run.agent)?
        .ok_or_else(|| Error::Daemon("the worker has no window to cut".to_owned()))?;
    let own = run.members.window_process(&window);
    run.members.cut(&window.pane)?;
    // What the cut leaves on the machine is ended: the command was the window's.
    if let Some(own) = own {
        let _ = LONG.end_in(own);
    }
    let gone = Instant::now();
    while !run.members.lane(&handle)?["pane"].is_null() && gone.elapsed() < STOPS {
        sleep(LOOK);
    }
    // The task the daemon holds for a window that is gone: it pauses it, with a note.
    let paused = becomes(run, number, "paused", STOPS)?;
    case.say(format!(
        "after the cut T-{number} reads {} ({} after the window went), its messages: {}",
        text(&run.members.task(number)?["state"]),
        paused.map_or_else(
            || "never paused".to_owned(),
            |at| format!("{:.1} s", at.as_secs_f64())
        ),
        messages(&run.members.task(number)?)
    ));
    let (since, since_ms) = chief_resumes(run, number, case)?;
    goes_on(run, number, &handle, since, since_ms, case)?;
    if !case.held() {
        say_events(run.members, given_ms, case);
    }
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

/// The chief tells the task's window while it is inside its command: the window answers the
/// tell and is asked to go on with its work in the same turn, and the chief resumes the task
/// as soon as it has the answer. A window that keeps working then has the resume's words wait
/// behind its turn, and the task must read working meanwhile; one that stopped has them at rest.
fn tell(run: &Run<'_>, case: &mut Account) -> Result<()> {
    let given_ms = jiff::Timestamp::now().as_millisecond();
    let (number, handle) = inside_the_command(run)?;
    let told = run
        .members
        .as_chief(["tell", &format!("T-{number}"), &tell_words()])?;
    case.say(format!(
        "cf tell T-{number}: exit {:?}, {}",
        told.code,
        told.stdout.trim()
    ));
    case.require(told.code == Some(0));
    let answered = find(
        run.log,
        &format!("the window's answer to the tell of T-{number}"),
        ANSWERS,
        Duration::from_secs(1),
        || {
            let thread = run.members.task(number)?;
            Ok(thread["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|message| message["kind"] == "answer")
                .then_some(()))
        },
    )?;
    case.check(
        "the window answered the tell",
        answered.is_some(),
        messages(&run.members.task(number)?),
    );
    // Does the window go on with its turn after the answer? Looked at twice, three
    // seconds apart: a window at work on both is one that keeps working.
    let first = text(&run.members.lane(&handle)?["activity"]["state"]);
    sleep(Duration::from_secs(3));
    let second = text(&run.members.lane(&handle)?["activity"]["state"]);
    let keeps_working = first == "working" && second == "working";
    case.say(format!(
        "after the answer the window's lane read {first}, then {second}: it {} after the tell; T-{number} reads {}",
        if keeps_working {
            "KEEPS WORKING"
        } else {
            "STOPPED"
        },
        text(&run.members.task(number)?["state"])
    ));
    let (since, since_ms) = chief_resumes(run, number, case)?;
    let waiting = goes_on(run, number, &handle, since, since_ms, case)?;
    if keeps_working {
        case.check(
            "the window kept working after the tell, and the task read working while the words that resumed it waited",
            waiting.is_some(),
            match waiting {
                Some(at) => format!("working {:.1} s after the resume, before the words were in", at.as_secs_f64()),
                None => "it read working only once the words were in the window, or never".to_owned(),
            },
        );
    }
    if !case.held() {
        say_events(run.members, given_ms, case);
    }
    ends(run, number, case)?;
    tidy(run, &handle);
    Ok(())
}

/// Why a delivery is held, where an event of the trace says: `delivery.held` keeps it beside
/// its data, not in it (T-1139: a Muse window at rest held the resume's words for good).
fn held_reason(event: &serde_json::Value) -> String {
    match event["reason"].as_str() {
        Some(reason) => format!(" ({reason})"),
        None => String::new(),
    }
}

/// The messages of a thread, as `m-id kind state`.
fn messages(thread: &serde_json::Value) -> String {
    thread["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|message| {
            format!(
                "m-{} {} {}",
                message["id"],
                text(&message["kind"]),
                text(&message["state"])
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Every event the daemon wrote since `since_ms` that is about a window, a delivery or a
/// stop, cut short: what happened, in order, when a case did not come out.
fn say_events(members: &Members, since_ms: i64, case: &mut Account) {
    let lines: Vec<String> = members
        .events()
        .iter()
        .filter(|event| at_ms(event).is_some_and(|at| at >= since_ms))
        .filter(|event| {
            let kind = text(&event["kind"]);
            ["delivery.", "window.", "stop", "pane.", "task.", "agent."]
                .iter()
                .any(|prefix| kind.starts_with(prefix))
        })
        .take(120)
        .map(|event| {
            format!(
                "+{:.1} s {} {}",
                (at_ms(event).unwrap_or(since_ms) - since_ms) as f64 / 1000.0,
                text(&event["kind"]),
                cut(&format!("{}{}", event["data"], held_reason(event)), 300)
            )
        })
        .collect();
    case.say(format!(
        "the daemon's events since the task was given:\n{}",
        lines.join("\n")
    ));
}

const SHAPES: [(&str, &str, Shape); 7] = [
    (
        "cut",
        "its window ended, then resumed by the chief with words",
        window_cut,
    ),
    (
        "early",
        "paused soon after it was given, resumed by the chief with words",
        early,
    ),
    (
        "pause-chief",
        "paused, resumed by the chief with words",
        pause_chief,
    ),
    ("pause-human", "paused, resumed by the human", pause_human),
    ("close", "the project closed and resumed", close),
    (
        "pause-close",
        "paused, the project closed and resumed, then the chief's resume",
        pause_close,
    ),
    (
        "tell",
        "told by the chief, the window's answer, then the chief's resume",
        tell,
    ),
];

/// The run of one harness in one shape: `lead` has a task done first, so that the window
/// of the shape's task is not the first of the project.
fn run_of(harness: Harness, shape: Shape, at: Duration, lead: bool, log: &Log) -> Result<Account> {
    let plan = Plan::new("task-resumed", stand_in_chief()).with(member(harness));
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let mut case = Account::new();
    let run = Run {
        members: &members,
        agent: &agent,
        log,
        at,
        begun_ms: jiff::Timestamp::now().as_millisecond(),
    };
    let seen = (|| {
        if lead {
            let number = members.give(&agent, LEAD)?;
            let over = find(
                log,
                &format!("the lead task T-{number} to end"),
                WORKS,
                LOOK,
                || {
                    let thread = members.task(number)?;
                    Ok(is_over(&thread).then_some(()))
                },
            )?;
            case.say(format!(
                "the lead task T-{number} {}",
                if over.is_some() {
                    "ended"
                } else {
                    "did not end"
                }
            ));
            case.require(over.is_some());
        }
        shape(&run, &mut case)
    })();
    if let Err(failed) = &seen {
        case.fail(failed);
    }
    if let Ok(Some(window)) = members.window_of(&agent) {
        if let Some(own) = members.window_process(&window) {
            let _ = LONG.end_in(own);
        }
    }
    let closed = members.close();
    case.closed(closed);
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a task and a resume of each harness named; cargo xtask live task-resumed"]
fn a_workers_task_that_was_stopped_and_resumed_goes_back_to_working() -> Result {
    let log = Log::begin("live task resumed")?;
    let harnesses = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &[Harness::Devin],
    )?;
    let lead = live::option("lead").is_some();
    let times: usize = live::option("times")
        .and_then(|times| times.parse().ok())
        .unwrap_or(1);
    let asked = live::list("shape");
    let mut checks = Vec::new();
    for harness in harnesses {
        let scoped = log.scoped(harness.name());
        let version = live::version_of(harness);
        for (word, what, shape) in SHAPES {
            if !asked.is_empty() && !asked.iter().any(|asked| asked == word) {
                continue;
            }
            // The early shape is run at each moment asked for; every other is run once a time.
            let moments: Vec<u64> = if word == "early" {
                moments(&live::list("at-ms"))
            } else {
                vec![0]
            };
            for (time, ms) in (1..=times).flat_map(|time| moments.iter().map(move |ms| (time, *ms)))
            {
                let name = format!(
                    "{word}{} #{time}{}",
                    if word == "early" {
                        format!(" at {ms} ms")
                    } else {
                        String::new()
                    },
                    if lead { " after a lead task" } else { "" }
                );
                let case = run_of(
                    harness,
                    shape,
                    Duration::from_millis(ms),
                    lead,
                    &scoped.scoped(name.clone()),
                )
                .unwrap_or_else(|failed| {
                    let mut case = Account::new();
                    case.fail(&failed);
                    case
                });
                log.block(
                    &format!(
                        "== {} {version}, {what}, {name}: {}",
                        harness.name(),
                        if case.held() { "ok" } else { "FAILED" }
                    ),
                    &case.text(),
                );
                checks.push(Check::new(
                    harness.name(),
                    &format!("{what} ({name}): the task reads working after the resume"),
                    case.held(),
                    version.clone(),
                ));
            }
        }
    }
    report::conclude(&log, "what each worker came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looks(list: &[(u64, &str)]) -> Vec<(Duration, String)> {
        list.iter()
            .map(|(ms, state)| (Duration::from_millis(*ms), (*state).to_owned()))
            .collect()
    }

    #[test]
    fn the_task_went_on_at_the_first_look_that_read_it_working() {
        let seen = looks(&[
            (30, "queued"),
            (280, "queued"),
            (2060, "working"),
            (2400, "working"),
        ]);
        assert_eq!(
            first_working(&seen, GOES_ON),
            Some(Duration::from_millis(2060))
        );
    }

    #[test]
    fn a_task_that_only_read_queued_or_paused_never_went_on() {
        let seen = looks(&[
            (30, "queued"),
            (5000, "queued"),
            (60_000, "paused"),
            (119_000, "queued"),
        ]);
        assert_eq!(first_working(&seen, GOES_ON), None);
        assert_eq!(first_working(&[], GOES_ON), None);
    }

    #[test]
    fn a_working_look_after_the_time_it_had_is_too_late() {
        let late = GOES_ON.as_millis() as u64 + 1000;
        let seen = looks(&[(30, "queued"), (late, "working")]);
        assert_eq!(first_working(&seen, GOES_ON), None);
        let on_time = GOES_ON.as_millis() as u64;
        assert_eq!(
            first_working(&looks(&[(on_time, "working")]), GOES_ON),
            Some(GOES_ON)
        );
    }

    #[test]
    fn the_early_shape_pauses_at_six_moments_unless_some_are_named_and_a_word_that_is_no_number_is_left_out(
    ) {
        assert_eq!(moments(&[]), [0, 1000, 3000, 6000, 10_000, 20_000]);
        let named: Vec<String> = ["500", "soon", "2000"]
            .iter()
            .map(|ms| (*ms).to_owned())
            .collect();
        assert_eq!(moments(&named), [500, 2000]);
    }

    #[test]
    fn the_tell_asks_for_its_answer_by_cf_answer_and_six_short_commands_each_in_a_tool_call_of_its_own(
    ) {
        let words = tell_words();
        assert!(words.contains("cf answer command, with exactly the word ok"));
        assert!(words.contains("run this command six times"));
        assert!(words.contains("each in a tool call of its own"));
        assert!(words.contains(&SHORT.text()));
        assert!(words.ends_with("reply with exactly one line: TELL_TURN_DONE"));
    }

    #[test]
    fn the_words_a_resume_queued_are_the_last_task_message_that_begins_resumed() {
        let thread = json!({ "messages": [
            { "id": 1, "kind": "task", "body": "Run the command" },
            { "id": 4, "kind": "task", "body": "Resumed: Go on" },
            { "id": 5, "kind": "note", "body": "Resumed: not a task" },
            { "id": 6, "kind": "task", "body": "Resumed: Go on again" },
        ] });
        assert_eq!(resume_words_of(&thread), Some(6));
        assert_eq!(resume_words_of(&json!({ "messages": [] })), None);
        assert_eq!(resume_words_of(&json!({})), None);
    }

    #[test]
    fn a_confirmation_is_the_resumes_only_where_it_names_the_message_of_the_words() {
        let confirmed =
            |message: i64| json!({ "kind": "delivery.confirmed", "data": { "message": message } });
        assert!(is_of(&confirmed(4), Some(4)));
        assert!(!is_of(&confirmed(5), Some(4)));
        assert!(is_of(&confirmed(5), None));
    }

    #[test]
    fn a_thread_is_told_as_the_kind_and_state_of_each_message() {
        let thread = json!({ "messages": [
            { "id": 1, "kind": "task", "state": "delivered" },
            { "id": 2, "kind": "note", "state": "queued" },
        ] });
        assert_eq!(messages(&thread), "m-1 task delivered; m-2 note queued");
        assert_eq!(messages(&json!({})), "");
    }
}
