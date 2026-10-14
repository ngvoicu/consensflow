//! The steps of a project that are about what the chief is told, and the small ask:
//! what its launches say of specs (off, then on), and that a small ask goes straight
//! on the board whether specs are on or not. (The steps that have the chief plan a
//! spec and carry it to the board are [`crate::planning`]'s.) Each is judged by what
//! the run reads where the product wrote it: the windows' launches, the board, the
//! daemon's events and the project's folder; never by what the agent says. A step
//! says what it checked as it goes, so that one that cannot finish has said so far.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::report::Check;
use cf_e2e::live::trace::Trace;
use cf_e2e::live::{cut, until, Harness};
use cf_e2e::{Error, Result};

use crate::ctx::Ctx;
use crate::evidence::DEFAULT_FOLDER;
use crate::judge::BoardTask;
use crate::launched;
use crate::plan::Step;
use crate::planning;

/// What the human asks of the chief for a small thing: one file, one task for a
/// worker, no plan.
const SMALL_OFF: &str = "Please have a worker create a file named hello.txt in this folder, containing just the word hello.";
const SMALL_ON: &str = "Please have a worker create a file named note.txt in this folder, containing just the word note.";

/// How long the chief has to put a small ask on the board: it reads, thinks and
/// runs `cf`.
const TASK: Duration = Duration::from_secs(300);

/// How long the stand-in worker has to be given its task and answer it, and the
/// chief to settle after it: a window opens and closes.
const WORKER: Duration = Duration::from_secs(300);

/// Runs a step of the project, saying what it checked in `checks`: an error is why
/// it could not go on.
pub fn run(ctx: &Ctx, step: Step, checks: &mut Vec<Check>) -> Result {
    match step {
        Step::Off => off(ctx, checks),
        Step::Source => source(ctx, checks),
        Step::Small => small(ctx, checks),
        Step::Spec => planning::spec(ctx, checks),
        Step::Board => planning::board(ctx, checks),
    }
}

/// A task as a line of a report: its number, its state and what it is about.
pub fn shown(task: &BoardTask) -> String {
    format!(
        "T-{} {} ({})",
        task.number,
        task.state,
        cut(&task.title, 60)
    )
}

/// The tasks as the lines of a report.
fn shown_all(tasks: &[BoardTask]) -> String {
    tasks.iter().map(shown).collect::<Vec<_>>().join("; ")
}

/// Waits for the chief to put the ask on the board as a task of its own, one that
/// was not there at `before`: the tasks it made.
fn goes_on_the_board(ctx: &Ctx, before: &BTreeSet<i64>) -> Result<Vec<BoardTask>> {
    let mut made = Vec::new();
    ctx.wait(
        "the chief to put the ask on the board as a task",
        TASK,
        || {
            made = ctx.new_chief_tasks(before)?;
            Ok(!made.is_empty())
        },
    )?;
    Ok(made)
}

/// Waits for the stand-in worker to have answered `made`, the tasks the chief put
/// on the board, and the chief to have settled after it.
fn is_worked(ctx: &Ctx, made: &[BoardTask]) -> Result {
    ctx.wait("the worker to answer the task", WORKER, || {
        let now = ctx.tasks()?;
        Ok(made.iter().all(|task| {
            now.iter()
                .find(|found| found.number == task.number)
                .is_some_and(|found| {
                    ["done", "accepted", "failed", "cancelled"].contains(&found.state.as_str())
                })
        }))
    })?;
    ctx.settles()
}

/// That no spec was written anywhere below the project: no spec file and no
/// registry. With `no_folder` (specs are off, and the chief has been told nothing of
/// them), no specs folder appeared either, the documented one or the one the
/// project names; with specs on, a folder that is there and holds nothing is no spec.
fn no_spec_anywhere(ctx: &Ctx, no_folder: bool) -> std::result::Result<String, String> {
    let mut named = vec![DEFAULT_FOLDER];
    if ctx.settings.folder != DEFAULT_FOLDER {
        named.push(ctx.settings.folder.as_str());
    }
    for folder in &named {
        let seen = ctx.files(folder).map_err(|failed| failed.to_string())?;
        seen.nothing_of_specs()?;
        if no_folder && seen.folder_exists {
            return Err(format!("the chief made the folder {folder}"));
        }
    }
    Ok(if no_folder {
        format!("no spec, no registry and no folder {}", named.join(" or "))
    } else {
        "no spec and no registry".to_owned()
    })
}

/// Specs off: the chief is told nothing of them; a small ask is a task on the
/// board, and nothing of specs appears in the folder.
fn off(ctx: &Ctx, checks: &mut Vec<Check>) -> Result {
    let first = ctx
        .chief_windows()
        .into_iter()
        .next()
        .ok_or_else(|| Error::Daemon("the daemon opened no window for the chief".to_owned()))?;
    let launch = ctx
        .role_text(&first)
        .and_then(|role| launched::told_nothing_of_specs(&role))
        .and_then(|()| launched::carries_no_skill(&first));
    checks.push(ctx.held("the chief's launch says nothing of specs", launch));

    let before = ctx.numbers()?;
    ctx.tells(SMALL_OFF)?;
    let made = goes_on_the_board(ctx, &before)?;
    checks.push(ctx.passed("a small ask goes on the board as a task", shown_all(&made)));
    is_worked(ctx, &made)?;
    checks.push(ctx.judged("no spec is written", no_spec_anywhere(ctx, true)));
    Ok(())
}

/// Specs on, which takes effect at the chief's next launch: the daemon says so in
/// its events, and the window is launched with the section of specs that names the
/// folder and the skill of the variant, which is the harness's own copy where it has
/// the variant installed and ConsensFlow's where it has not. A Codex chief's own
/// record then holds the section too, which its launch cannot say: its thread keeps
/// the instructions it began with.
fn source(ctx: &Ctx, checks: &mut Vec<Check>) -> Result {
    let frame = ctx.with_specs_on()?;
    let logged = Trace::read(ctx.rig)
        .events()
        .iter()
        .find(|event| event["kind"] == "project.specs")
        .map(|event| event["data"]["to"].clone());
    checks.push(ctx.held(
        "the daemon logged what the project's specs were set to",
        match logged {
            Some(to) if to == ctx.settings.view() => Ok(()),
            Some(to) => Err(format!("it logged {to}, not {}", ctx.settings.view())),
            None => Err("it logged no project.specs event".to_owned()),
        },
    ));
    let text = match ctx.role_text(&frame) {
        Ok(text) => text,
        Err(why) => {
            checks.push(ctx.held("the chief's launch has the section of specs", Err(why)));
            return Ok(());
        }
    };
    checks.push(
        ctx.held(
            "the chief's launch has the section of specs",
            launched::has_the_section(&text)
                .then_some(())
                .ok_or_else(|| "its text has no section of specs".to_owned()),
        ),
    );
    checks.push(ctx.judged(
        "the section names the specs folder",
        match launched::folder_named_in(&text) {
            Some(named) if named == ctx.settings.folder => Ok(named),
            other => Err(format!("it names {other:?}, not {}", ctx.settings.folder)),
        },
    ));
    checks.push(ctx.judged(
        "the skill is the harness's own copy where the harness has the variant, and ConsensFlow's where it has not",
        which_skill(ctx, &text),
    ));
    // A Codex thread keeps the instructions it began with, so what its launch says
    // is not what the chief was told: its own record, after the Resume, must hold
    // the section.
    if ctx.planned.chief == Harness::Codex {
        checks.push(
            ctx.judged(
                "the chief's own record holds the section of specs after the Resume",
                ctx.chief_record(&frame)
                    .and_then(|record| launched::told_in_the_record(&record)),
            ),
        );
    }
    Ok(())
}

/// Which copy of the skill the text names, judged ([`launched::which_copy`]); where
/// it is ConsensFlow's, that the files it names are beside it.
fn which_skill(ctx: &Ctx, text: &str) -> std::result::Result<String, String> {
    let named = launched::skill_named_in(text);
    let read = named.as_deref().map(std::fs::read_to_string);
    if let Some(Err(failed)) = &read {
        return Err(format!(
            "the skill's file {} cannot be read: {failed}",
            named.as_deref().unwrap_or_default()
        ));
    }
    let called = read
        .and_then(std::result::Result::ok)
        .and_then(|file| launched::skill_called(&file));
    let home = ctx.rig.home().to_string_lossy().into_owned();
    let which = launched::which_copy(
        ctx.settings.variant.name,
        named.as_deref(),
        called.as_deref(),
        ctx.settings.installed_at.as_deref(),
        &home,
    )?;
    if let (None, Some(file)) = (&ctx.settings.installed_at, named.as_deref()) {
        let beside = Path::new(file)
            .parent()
            .map(|folder| folder.join("references"));
        if !beside.is_some_and(|folder| folder.is_dir()) {
            return Err(format!("{which}, with no references folder beside it"));
        }
    }
    Ok(which)
}

/// Specs on: a small ask is still a task on the board and no spec, and the window of
/// the worker that has it is launched with nothing of specs.
fn small(ctx: &Ctx, checks: &mut Vec<Check>) -> Result {
    ctx.with_specs_on()?;
    let opened = ctx.worker_windows().len();
    let before = ctx.numbers()?;
    ctx.tells(SMALL_ON)?;
    let made = goes_on_the_board(ctx, &before)?;
    checks.push(ctx.passed(
        "a small ask still goes straight on the board as a task",
        shown_all(&made),
    ));
    // Read now, while the worker's window is open: its files go when it closes.
    let given = member_launch(ctx, opened);
    is_worked(ctx, &made)?;
    checks.push(ctx.judged("a member's window is given nothing of specs", given));
    checks.push(ctx.judged("no spec is written", no_spec_anywhere(ctx, false)));
    Ok(())
}

/// What the windows the daemon opened for the worker since `opened` were launched
/// with, read as soon as there is one: nothing of Specmint on its command line or in
/// its environment, and none in its role text where that can still be read. (A
/// launch's files go with its window, which the stand-in worker closes within
/// seconds; the run looks every tenth of a second, and says when it was too late for
/// the text.)
fn member_launch(ctx: &Ctx, opened: usize) -> std::result::Result<String, String> {
    let windows = until(WORKER, Duration::from_millis(100), || {
        let windows: Vec<_> = ctx.worker_windows().into_iter().skip(opened).collect();
        (!windows.is_empty()).then_some(windows)
    })
    .ok_or("the daemon opened no window for the worker's task")?;
    let mut texts = 0;
    for frame in &windows {
        launched::carries_no_skill(frame)?;
        if let Ok(role) = ctx.role_text(frame) {
            launched::told_nothing_of_specs(&role)?;
            texts += 1;
        }
    }
    let of = if windows.len() == 1 {
        "its window".to_owned()
    } else {
        format!("its {} windows", windows.len())
    };
    Ok(if texts == windows.len() {
        format!(
            "{of}: nothing of specs on the command line, in the environment or in the role text"
        )
    } else {
        format!(
            "{of}: nothing of specs on the command line or in the environment; the role text of {} was gone with its window",
            windows.len() - texts
        )
    })
}
