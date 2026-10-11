//! The steps of a project, each a whole path through the gate with real
//! harnesses: what the human asks of the chief, what the daemon holds, what the
//! human passes on or declines, and what comes of it.

use std::time::{Duration, Instant};

use cf_e2e::live::{cut, text, wait, Harness};
use cf_e2e::{files, png, Error, Result};
use serde_json::Value;

use crate::agents::{self, DESIGNER, TIER};
use crate::flow::{accepts, Ctx, ANSWERS, DELIVERS, QUIET};
use crate::plan::Step;
use crate::verdict;

/// How long the advisor's window has worked on its task before the human has the
/// chief pause it: long enough to be in the middle of the command it was asked
/// to run, which sleeps for a minute.
const MID_TURN: Duration = Duration::from_secs(15);

/// How long the chief has to pause a task, once the human has told it to, and
/// how long the window has to stop.
const PAUSES: Duration = Duration::from_secs(240);
const STOPS: Duration = Duration::from_secs(90);

/// How long the chief has to resume a task, and the words it has to hold.
const RESUMES: Duration = Duration::from_secs(240);
const RESUMED: &str = "RESUMED";

/// How long a worker has to ask its question, and the window the daemon gives
/// an answer's door to read it.
const ASKS: Duration = Duration::from_secs(480);
const READS: Duration = Duration::from_secs(120);

/// How long the image designer has to draw, and its result to be held.
const DRAWS: Duration = Duration::from_secs(600);

/// Where the brief of the designer says to save the image, from the project's
/// folder, with `/` between folders.
const IMAGE: &str = "images/honey.png";

/// A word that is this project's and this step's own, for a brief to hold and
/// a result to say.
fn marker(ctx: &Ctx, step: &str) -> String {
    format!("{step}_OK_P{}", ctx.planned.number)
}

/// A finding about Codex's results: whether the text a Codex window left for
/// the chief carries Codex's own directive markup (`::codex-…{…}`).
fn codex_markup(ctx: &Ctx, harness: Harness, what: &str, result: &Value) {
    if harness != Harness::Codex {
        return;
    }
    match verdict::codex_markup(&text(&result["body"])) {
        Some(line) => ctx.note(format!(
            "the result of Codex's {what} holds directive markup (a fix is on another branch): {line}"
        )),
        None => ctx.note(format!(
            "the result of Codex's {what} holds no :codex- directive markup"
        )),
    }
}

/// Step 1: a task for the worker, its brief and its result each held at the
/// gate until the human passes them on.
pub fn worker(ctx: &Ctx) -> Result {
    let seats = &ctx.planned.seats;
    let marker = marker(ctx, "WORKER");
    let asked = format!("Reply with exactly: {marker}");
    let given = ctx.give(
        &format!(
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --tier {TIER} \"{asked}\"\n{}",
            accepts("the number of the task that command printed")
        ),
        &marker,
        &agents::id(seats.worker),
    )?;
    ctx.open_window(&given)?;
    let result = ctx.result_held(&given, &[&marker], ANSWERS)?;
    codex_markup(ctx, seats.worker, "worker", &result);
    ctx.check(
        "the worker's window did not record the task as given",
        verdict::recorded(&ctx.record(given.task)?, &asked, Some(&marker)),
    )?;
    // The chief is at work when the result is passed on: it waits for the turn's end.
    ctx.deliver_to_a_busy_chief(given.task, &result)?;
    ctx.accepted(given.task)?;
    ctx.closes(&given)
}

/// Step 2: a task for the advisor that takes a minute, paused while it works and
/// resumed with other words, which are held at the gate too.
pub fn advisor(ctx: &Ctx) -> Result {
    let seats = &ctx.planned.seats;
    let marker = marker(ctx, "ADVISOR");
    let asked = format!("Run the shell command sleep 60, then reply with exactly: {marker}");
    let given = ctx.give(
        &format!(
            "Run exactly this command in your shell, then reply with one line and wait for my \
             next message:\ncf task add --advice --tier {TIER} \"{asked}\""
        ),
        &marker,
        &agents::id(seats.advisor),
    )?;
    let task = given.task;
    ctx.open_window(&given)?;

    // The advisor is in the middle of its turn.
    let mut working_since: Option<Instant> = None;
    wait(
        ctx.rig,
        ctx.log,
        &format!(
            "@{} to be in the middle of its turn on T-{task}",
            given.session
        ),
        ANSWERS,
        || {
            ctx.not_over(task)?;
            ctx.progress(task, &given.session)?;
            let working =
                ctx.state_of(task)? == "working" && ctx.activity_of(&given.session)? == "working";
            working_since = match (working, working_since) {
                (true, Some(since)) => Some(since),
                (true, None) => Some(Instant::now()),
                (false, _) => None,
            };
            Ok(working_since.is_some_and(|since| since.elapsed() >= MID_TURN))
        },
    )?;
    if ctx
        .human
        .gated_now(|message| message["kind"] == "result" && message["taskNumber"] == task)?
        .is_some()
    {
        return Err(Error::Daemon(format!(
            "@{} had answered T-{task} before the human could have it paused",
            given.session
        )));
    }

    // The human has the chief pause it: the task waits, and the window stops.
    ctx.human.tells(&format!(
        "Run exactly this command in your shell, then reply with one line:\ncf task pause T-{task}"
    ))?;
    wait(
        ctx.rig,
        ctx.log,
        &format!("the chief to pause T-{task}"),
        PAUSES,
        || Ok(ctx.state_of(task)? == "paused"),
    )?;
    wait(
        ctx.rig,
        ctx.log,
        &format!("the window of @{} to stop working", given.session),
        STOPS,
        || Ok(ctx.activity_of(&given.session)? != "working"),
    )?;
    ctx.log.say(format!(
        "T-{task} is paused and the window of @{} is {}",
        given.session,
        ctx.activity_of(&given.session)?
    ));

    // The human has the chief resume it with other words: they wait at the gate.
    let words = format!("Answer with the word {RESUMED} instead");
    ctx.human.tells(&format!(
        "Run exactly this command in your shell, then reply with one line:\n\
         cf task resume T-{task} \"{words}\"\n{}",
        accepts(&format!("T-{task}"))
    ))?;
    let held = ctx.human.waits_for_gated(
        "the words that resume the task to wait at the gate",
        RESUMES,
        |message| {
            message["taskNumber"] == task
                && message["recipient"] == given.session.as_str()
                && message["id"] != given.brief["id"]
                && text(&message["body"]).contains(RESUMED)
        },
    )?;
    ctx.check(
        "the words that resume the task are not held as the gate holds them",
        verdict::held(&held, &ctx.trace()),
    )?;
    let state = ctx.state_of(task)?;
    if state == "working" {
        return Err(Error::Daemon(format!(
            "T-{task} went back to work before the human passed its words on"
        )));
    }
    ctx.human.approves(&held)?;
    let id = verdict::id(&held);
    wait(
        ctx.rig,
        ctx.log,
        &format!("the words m-{id} to be delivered to the window"),
        DELIVERS,
        || {
            ctx.progress(task, &given.session)?;
            Ok(ctx.message(task, id)?["state"] == "delivered")
        },
    )?;
    wait(
        ctx.rig,
        ctx.log,
        &format!("T-{task} to go back to working"),
        STOPS,
        || Ok(["working", "done"].contains(&ctx.state_of(task)?.as_str())),
    )?;

    let result = ctx.result_held(&given, &[RESUMED], ANSWERS)?;
    codex_markup(ctx, seats.advisor, "advisor", &result);
    ctx.check(
        "the advisor's window did not record the resume as given",
        verdict::recorded(&ctx.record(task)?, &words, Some(RESUMED)),
    )?;
    ctx.deliver(task, &result)?;
    ctx.accepted(task)?;
    ctx.closes(&given)
}

/// Step 3: a task for the reviewer whose brief the human declines: the task is
/// cancelled, the chief is told, and no window opens.
pub fn reviewer(ctx: &Ctx) -> Result {
    let seats = &ctx.planned.seats;
    let agent = agents::id(seats.reviewer);
    let marker = marker(ctx, "REVIEW");
    let given = ctx.give(
        &format!(
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --review --tier {TIER} \"Review this sentence for spelling: The quick \
             brown fox. Reply with exactly: {marker}\""
        ),
        &marker,
        &agent,
    )?;
    let task = given.task;
    let declined = ctx.human.declines(&given.brief)?;
    let id = verdict::id(&declined);
    ctx.check(
        "the declined brief is not what a declined brief is",
        verdict::declined(&ctx.message(task, id)?, &ctx.trace()),
    )?;
    let state = ctx.state_of(task)?;
    if state != "cancelled" {
        return Err(Error::Daemon(format!(
            "T-{task} is {state} after its brief was declined, not cancelled"
        )));
    }
    // The chief is told, in its own window, once.
    let told = format!("@human declined T-{task}");
    let note = {
        let find = || -> Result<Option<Value>> {
            Ok(ctx
                .project
                .inbox("chief")?
                .into_iter()
                .find(|message| text(&message["body"]).starts_with(&told)))
        };
        wait(
            ctx.rig,
            ctx.log,
            &format!("the chief to be told the human declined T-{task}"),
            DELIVERS,
            || Ok(find()?.is_some_and(|note| note["state"] == "delivered")),
        )?;
        find()?.ok_or_else(|| Error::Daemon("the note to the chief is gone".to_owned()))?
    };
    ctx.check(
        "the chief was not told of the declined task as a message is delivered",
        verdict::typed_once(&note, &ctx.trace(), "chief"),
    )?;
    // And no window opens, then or later.
    std::thread::sleep(QUIET);
    if let Some(frame) = ctx.opened_since(&agent, &given.before).first() {
        return Err(Error::Daemon(format!(
            "a window ({}) was opened for the reviewer after its brief was declined",
            text(&frame["id"])
        )));
    }
    ctx.log.say(format!(
        "T-{task} is cancelled, the chief was told once, and no window opened for @{agent}"
    ));
    Ok(())
}

/// Step 4: the worker asks the chief a question through its harness's own
/// tool; the question and the answer are each held at the gate; the answer
/// reaches the worker at the door that asked; the worker finishes.
pub fn question(ctx: &Ctx) -> Result {
    let seats = &ctx.planned.seats;
    let Some(tool) = agents::question_tool(seats.worker) else {
        return Err(Error::Daemon(format!(
            "{} has no tool to ask a question with",
            seats.worker.name()
        )));
    };
    let fragment = "which colour to use";
    let given = ctx.give(
        &format!(
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --tier {TIER} \"Use your {tool} to ask {fragment}, with the options red \
             and blue. Once it is answered, reply with exactly one line: COLOUR=<the answer>\"\n\
             When its question reaches you, choose a colour yourself and answer it with cf \
             answer. {}",
            accepts("the number of the task that command printed")
        ),
        fragment,
        &agents::id(seats.worker),
    )?;
    let task = given.task;
    ctx.open_window(&given)?;

    // The question waits at the gate, and the asker's task waits for its answer.
    let question = ctx.human.waits_for_gated(
        "the worker's question to wait at the gate",
        ASKS,
        |message| {
            message["kind"] == "question"
                && message["taskNumber"] == task
                && message["sender"] == given.session.as_str()
        },
    )?;
    let asked = verdict::id(&question);
    ctx.log.say(format!(
        "the question m-{asked} waits at the gate: {}",
        cut(&text(&question["body"]), 160)
    ));
    std::thread::sleep(QUIET);
    ctx.check(
        "the question is not held as the gate holds it",
        verdict::held(&ctx.message(task, asked)?, &ctx.trace()),
    )?;
    if question["questions"].is_null() {
        return Err(Error::Daemon(format!(
            "the question m-{asked} carries no options of the tool that asked it: {question}"
        )));
    }
    ctx.human.approves(&question)?;
    wait(
        ctx.rig,
        ctx.log,
        &format!("the question m-{asked} to be typed into the chief's window"),
        DELIVERS,
        || Ok(ctx.message(task, asked)?["state"] == "delivered"),
    )?;
    ctx.check(
        "the question was not typed into the chief as the gate promises",
        verdict::typed_once(&ctx.message(task, asked)?, &ctx.trace(), "chief"),
    )?;

    // The chief's answer waits at the gate; the worker's door has not got it.
    let answer = ctx.human.waits_for_gated(
        "the chief's answer to wait at the gate",
        ANSWERS,
        |message| message["kind"] == "answer" && message["replyTo"] == asked,
    )?;
    let answered = verdict::id(&answer);
    ctx.log.say(format!(
        "the answer m-{answered} waits at the gate: {}",
        cut(&text(&answer["body"]), 120)
    ));
    std::thread::sleep(QUIET);
    ctx.check(
        "the answer is not held as the gate holds it",
        verdict::held(&ctx.message(task, answered)?, &ctx.trace()),
    )?;
    let state = ctx.state_of(task)?;
    if state != "waiting" {
        return Err(Error::Daemon(format!(
            "T-{task} is {state} while the answer to its question waits at the gate, not waiting"
        )));
    }
    ctx.human.approves(&answer)?;
    wait(
        ctx.rig,
        ctx.log,
        &format!("the worker's door to read the answer m-{answered}"),
        READS,
        || {
            ctx.progress(task, &given.session)?;
            Ok(ctx.message(task, answered)?["state"] == "read")
        },
    )?;
    let read = ctx.message(task, answered)?;
    ctx.check(
        "the answer was not handed to the door that asked",
        verdict::at_the_door(&read, &ctx.trace()),
    )?;

    // The worker finishes with the colour that was answered.
    let colour = read["choices"][0][0]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| text(&read["body"]));
    let says = format!("COLOUR={colour}");
    let result = ctx.result_held(&given, &[], ANSWERS)?;
    ctx.check(
        "the result does not hold the colour that was answered",
        if text(&result["body"])
            .to_lowercase()
            .contains(&says.to_lowercase())
        {
            Vec::new()
        } else {
            vec![format!(
                "the result does not say {says}: {}",
                cut(&text(&result["body"]), 200)
            )]
        },
    )?;
    codex_markup(ctx, seats.worker, "worker", &result);
    ctx.deliver(task, &result)?;
    ctx.accepted(task)?;
    ctx.closes(&given)
}

/// Step 5: a task for the image designer, which draws the image the brief names
/// where it says; the brief and the result wait at the gate as any do.
pub fn designer(ctx: &Ctx) -> Result {
    let brief = "Draw a small flat icon of a jar of honey on a plain background. Save it as \
                 images/honey.png.";
    let given = ctx.give(
        &format!(
            "Run exactly this command in your shell, then reply with one line:\n\
             cf task add --design \"{brief}\"\n{}",
            accepts("the number of the task that command printed")
        ),
        "jar of honey",
        DESIGNER,
    )?;
    ctx.open_window(&given)?;
    let result = ctx.result_held(&given, &[], DRAWS)?;
    codex_markup(ctx, Harness::Codex, "image designer", &result);
    let file = IMAGE
        .split('/')
        .fold(ctx.folder.to_path_buf(), |path, part| path.join(part));
    let image = files::read(&file).map_err(|_| {
        Error::Daemon(format!(
            "the designer saved no file at {IMAGE}: its result says {}",
            cut(&text(&result["body"]), 200)
        ))
    })?;
    let picture = png::inspect(&image)
        .map_err(|why| Error::Daemon(format!("{IMAGE} is no whole PNG: {why}")))?;
    if image.len() < 1024 || picture.width < 16 || picture.height < 16 {
        return Err(Error::Daemon(format!(
            "{IMAGE} is {} bytes and {} by {} pixels: not a picture",
            image.len(),
            picture.width,
            picture.height
        )));
    }
    ctx.log.say(format!(
        "the designer drew {IMAGE}: a {} by {} PNG of {} bytes",
        picture.width,
        picture.height,
        image.len()
    ));
    ctx.deliver(given.task, &result)?;
    ctx.accepted(given.task)?;
    ctx.closes(&given)
}

/// Runs `step` of the project.
pub fn run(ctx: &Ctx, step: Step) -> Result {
    match step {
        Step::Worker => worker(ctx),
        Step::Advisor => advisor(ctx),
        Step::Reviewer => reviewer(ctx),
        Step::Question => question(ctx),
        Step::Designer => designer(ctx),
    }
}
