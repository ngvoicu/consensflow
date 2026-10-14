//! The steps that have the chief plan. The human asks for a spec, and the chief
//! interviews them as Specmint does (in plain text, a message of questions, which
//! the human answers in one) and writes the spec in the specs folder; then the human
//! approves it, and the chief puts its tasks on the board in phase order, with what
//! each needs, has the stand-in workers answer them, accepts the answers, and marks
//! the spec's progress. What the run judges is in the folder and on the board.

use std::time::{Duration, Instant};

use cf_e2e::live::report::Check;
use cf_e2e::{files, Error, Result};

use crate::ctx::Ctx;
use crate::evidence::Files;
use crate::judge::{self, Spec};
use crate::steps::shown;

/// What the human asks for: a spec of two phases, each as small as the variant's
/// format allows (the tdd ones' tasks come in pairs, a test and its implementation).
const SPEC_ASK: &str = "I want this planned as a spec, in two phases: phase 1 creates a file named hello.txt containing the word hello, and phase 2 creates a file named goodbye.txt containing the word goodbye. Keep each phase as small as the skill's format allows. If you need to ask me anything, ask it in plain text, all in one message.";

/// What the human answers to whatever else the chief asks, however it asks it.
const ANSWER: &str = "Use your best judgment on whatever you still need: keep it exactly as small as I said, two phases and no extra requirements. Write the spec now.";

/// What the human says once the spec is written: it is approved, to be carried out.
const GO_AHEAD: &str = "The spec is approved. Carry it out.";

/// What the human says to a chief that has results waiting and has not accepted them.
const NUDGE: &str = "Accept the tasks the workers have finished.";

/// How long the chief has for a turn of the interview or of the writing, and how
/// many times the human answers it before the run says it wrote no spec.
const TURN: Duration = Duration::from_secs(420);
const ROUNDS: usize = 4;

/// How long the chief has to put the spec's tasks on the board once the human has
/// approved the spec.
const BOARD: Duration = Duration::from_secs(600);

/// How long the work has to be done and accepted, each phase after the one before
/// it: a window for each task, and a turn of the chief for each result. The chief
/// is asked to accept once results have waited for it this long, if it is idle.
const WORK: Duration = Duration::from_secs(1500);
const ASKED_AFTER: Duration = Duration::from_secs(150);

/// How long the spec and its registry have to show what the chief accepted, once it
/// has: they are written in a turn of the chief's after the acceptance.
const MARKED: Duration = Duration::from_secs(240);

/// Whether a wait ran out, which is a verdict of a check and no failure of the run;
/// any other error is the run's.
fn timed_out(waited: Result) -> Result<bool> {
    match waited {
        Ok(()) => Ok(false),
        Err(Error::Timeout(_)) => Ok(true),
        Err(other) => Err(other),
    }
}

/// The spec in the file at `path`, below the project's folder, as it is now.
fn read_spec(ctx: &Ctx, path: &str) -> std::result::Result<Spec, String> {
    let name = path.rsplit('/').next().unwrap_or_default();
    let text = files::read_string(&ctx.folder.join(path)).map_err(|failed| failed.to_string())?;
    judge::read(name, &text)
}

/// The registry's row of the spec, as it is now.
fn registry_row(ctx: &Ctx, id: &str) -> std::result::Result<judge::Registered, String> {
    let seen = ctx
        .files(&ctx.settings.folder)
        .map_err(|failed| failed.to_string())?;
    let registry = seen
        .registry
        .ok_or_else(|| format!("{} holds no registry.md", ctx.settings.folder))?;
    judge::registered(&registry, id).ok_or_else(|| format!("the registry has no row for {id}"))
}

/// Specs on: the human has the chief plan a tiny two-phase ask as a spec; the
/// chief's interview is answered in one message each time it stops to ask, and the
/// spec is in the specs folder the project names, in the shape of its variant, with a
/// row in the registry.
pub fn spec(ctx: &Ctx, checks: &mut Vec<Check>) -> Result {
    ctx.with_specs_on()?;
    let folder = ctx.settings.folder.as_str();
    ctx.tells(SPEC_ASK)?;
    let mut answered = 0;
    let mut seen = Files::default();
    loop {
        ctx.wait(
            "the chief to write the spec, or to stop and ask what it needs",
            TURN,
            || {
                seen = ctx.files(folder)?;
                Ok(!seen.specs.is_empty() || ctx.human.chief_is()? == "idle")
            },
        )?;
        if !seen.specs.is_empty() || answered == ROUNDS {
            break;
        }
        answered += 1;
        ctx.log.say(format!(
            "the chief stopped with no spec: the human answers it (answer {answered} of {ROUNDS})"
        ));
        ctx.tells(ANSWER)?;
    }
    // The registry is written beside the spec, in the same turn or the one after it.
    ctx.settles()?;
    let seen = ctx.files(folder)?;
    let found = match seen.the_spec(folder) {
        Ok(found) => found,
        Err(why) => {
            checks.push(ctx.judged("the chief writes a spec in the specs folder", Err(why)));
            return Ok(());
        }
    };
    let spec = match &found.spec {
        Ok(spec) => spec,
        Err(why) => {
            checks.push(ctx.judged(
                "the chief writes a spec in the specs folder",
                Err(format!("{} is no spec: {why}", found.path)),
            ));
            return Ok(());
        }
    };
    ctx.found_spec(&found.path);
    let (done, total) = spec.progress();
    checks.push(ctx.passed(
        "the chief writes a spec in the specs folder",
        format!(
            "{}: {} in {} phases, {done} of {total} tasks done",
            found.path,
            spec.id,
            spec.phases.len()
        ),
    ));
    let thin = spec
        .phases
        .iter()
        .find(|phase| phase.tasks.is_empty())
        .map(|phase| format!("phase {} has no task", phase.number));
    checks.push(ctx.held(
        "the spec has the two phases that were asked for, with their tasks",
        match (spec.phases.len() >= 2, thin) {
            (false, _) => Err(format!("the spec has {} phases", spec.phases.len())),
            (true, Some(why)) => Err(why),
            (true, None) => Ok(()),
        },
    ));
    checks.push(ctx.held(
        "the spec is in the shape of the variant",
        judge::in_the_shape_of(ctx.settings.variant.name, &found.file, spec),
    ));
    checks.push(
        ctx.judged(
            "the registry has a row for the spec",
            registry_row(ctx, &spec.id)
                .map(|row| format!("{}: {} of {} done", row.status, row.done, row.total)),
        ),
    );
    Ok(())
}

/// The human approves the spec: the chief puts its tasks on the board in phase order,
/// each task of a phase needing the tasks of the phase before it; the stand-in workers
/// answer them, the chief accepts the answers, and the spec and its registry show the
/// work that was accepted.
pub fn board(ctx: &Ctx, checks: &mut Vec<Check>) -> Result {
    ctx.with_specs_on()?;
    let path = ctx.spec_path().ok_or_else(|| {
        Error::Daemon("the run has no spec to carry out: the step before wrote none".to_owned())
    })?;
    let spec = read_spec(ctx, &path).map_err(Error::Daemon)?;
    ctx.tells(GO_AHEAD)?;

    // Every phase has a task on the board, which the chief made from the spec.
    let mut tasks = Vec::new();
    let reached = ctx.wait(
        "the chief to put a task of every phase of the spec on the board",
        BOARD,
        || {
            tasks = ctx.tasks()?;
            let made = judge::from_the_spec(&spec, &tasks);
            Ok(spec
                .phases
                .iter()
                .all(|phase| made.iter().any(|(number, _)| *number == phase.number)))
        },
    );
    let stopped = timed_out(reached)?;
    if !stopped {
        // Its turn is over, and the tasks it made in it are all there; a chief that has
        // results to take in goes on a while, which is no failure of the board.
        if let Err(why) = ctx.settles() {
            ctx.log.say(format!("the chief was still at work: {why}"));
        }
        tasks = ctx.tasks()?;
    }
    checks.push(ctx.judged(
        "the tasks on the board follow the spec's phases",
        judge::follow_the_phases(&spec, &tasks),
    ));
    if stopped {
        return Ok(());
    }

    // The workers answer, and the chief accepts what they answer.
    let started = Instant::now();
    let mut asked = false;
    let mut states = String::new();
    let waited = ctx.wait("every task of the spec to be accepted", WORK, || {
        tasks = ctx.tasks()?;
        let made = judge::from_the_spec(&spec, &tasks);
        states = made
            .iter()
            .map(|(_, task)| shown(task))
            .collect::<Vec<_>>()
            .join("; ");
        if let Some((_, task)) = made.iter().find(|(_, task)| task.state == "failed") {
            return Err(Error::Daemon(format!(
                "{} failed: the worker was given it and did not answer it",
                shown(task)
            )));
        }
        let over = made.iter().all(|(_, task)| task.state == "accepted");
        if !over
            && !asked
            && started.elapsed() >= ASKED_AFTER
            && made.iter().any(|(_, task)| task.state == "done")
            && ctx.human.chief_is()? == "idle"
        {
            asked = true;
            ctx.log.say(format!(
                "the chief did not accept the results waiting for it in {} s: the human asks it to",
                ASKED_AFTER.as_secs()
            ));
            ctx.tells(NUDGE)?;
        }
        Ok(over)
    });
    let accepted = !timed_out(waited)?;
    let asked_for = if asked {
        "; the human had to ask it to"
    } else {
        ""
    };
    checks.push(if accepted {
        ctx.passed(
            "the chief accepts the tasks the workers finish",
            format!("{states}{asked_for}"),
        )
    } else {
        ctx.judged(
            "the chief accepts the tasks the workers finish",
            Err(format!(
                "not all accepted in {} s: {states}{asked_for}",
                WORK.as_secs()
            )),
        )
    });

    // What was accepted is shown in the spec and its registry.
    let mut shows = Err("the spec has not been read".to_owned());
    let marked = ctx.wait(
        "the spec and its registry to show what the chief accepted",
        MARKED,
        || {
            tasks = ctx.tasks()?;
            shows = read_spec(ctx, &path).and_then(|now| {
                let row = registry_row(ctx, &now.id)?;
                judge::progress_shows(&now, Some(&row), &tasks)
            });
            Ok(shows.is_ok())
        },
    );
    if timed_out(marked)? {
        ctx.log.say(format!(
            "the spec and its registry showed nothing of it in {} s",
            MARKED.as_secs()
        ));
    }
    checks.push(ctx.judged(
        "the spec and its registry show the progress the chief accepted",
        shows,
    ));
    Ok(())
}
