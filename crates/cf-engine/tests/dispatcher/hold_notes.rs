//! The note a held task's requester is given (T-1181, the field boards of 2026-10-26 and
//! 2026-11-02): **one note when a task first goes on in the background, then the result.**
//! The jobs of a held task come and go, its monitors say their time ran out, a look at
//! its window finds commands that still run: none of that is told again. Kluris' T-104 and
//! T-105 sent about ten such notes while they waited for Windows runs, T-298 sent twelve in
//! 1 h 46, and each woke the chief for a turn with nothing to do.
//!
//! The note is plain: what runs, by kind and count, and with what the harness's reader
//! knows of a command (when it began, that a time limit moved it into the background: the
//! field board's T-299 had one run 100 minutes with its worker none the wiser), and that
//! the result comes when they end; then the requester's other choice, to take what the
//! worker said last now.
//!
//! The notes that remain are those that need the requester: a hold that turns into one
//! nothing ends but its Finish (told once more), the failure or the pause of a held task
//! whose window goes, the answer of a held task the requester tells, and a hold told again
//! in a window opened again or after a restart.

use cf_engine::testing::{Context, START_MS};
use cf_harness::records::{Command, End, Monitor, Pending, Source};

use crate::fixtures::Tiers;
use crate::turn_end::{monitor, pending};

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// A project of zeus and diana, and T-1 given to zeus by name, at work in its window.
fn zeus_at_work(context: &Context) -> Tiers<'_> {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    context.give(tiers.project.id, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

/// The notes the chief was told of T-1 that say it is held.
fn hold_notes(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1 is still working in the background"))
        .collect()
}

/// The results the chief was given, of T-1.
fn results(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .task(1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_held_task_whose_jobs_and_monitors_come_and_go_tells_its_chief_once_and_then_the_result() {
    // T-298's shape (the field board of 2026-11-02, 12 notes in 1 h 46) and kluris' T-104
    // and T-105 (about ten): a release or a suite starts a job, ends its turn, and each end
    // wakes it for another turn that starts more.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.background.commands_run(Some(2));
    context
        .adapter
        .answer_leaving("zeus", "The Windows run starts", pending(1, 0));
    context.pass().unwrap();
    assert_eq!(
        tiers.notes("chief").len(),
        1,
        "told when it first goes on: {:?}",
        tiers.notes("chief")
    );

    // Jobs end and begin: the counts rise and fall.
    for (round, jobs) in [2, 3, 1, 2, 1].into_iter().enumerate() {
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        context
            .adapter
            .answer_leaving("zeus", &format!("Round {round}"), pending(jobs, 0));
        context.pass().unwrap();
    }
    // Monitors are armed, say their time ran out, and a look finds commands that run on.
    for monitors in [
        vec![monitor("windows", Source::Command, None)],
        vec![
            monitor("windows", Source::Command, None),
            monitor("linux", Source::Command, None),
        ],
        vec![monitor("windows", Source::Command, Some(End::Expired))],
        vec![monitor("windows", Source::Command, Some(End::Stopped))],
    ] {
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        let noticed = monitors.iter().filter(|each| each.notice.is_some()).count();
        context.adapter.answer_leaving_monitors(
            "zeus",
            "Watching the runs",
            pending(monitors.len(), noticed),
            monitors,
        );
        passes(&context, 3);
        context.advance(2_000);
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        tiers.notes("chief").len(),
        1,
        "later changes of the hold are told no more: {:?}",
        tiers.notes("chief")
    );
    assert_eq!(hold_notes(&tiers).len(), 1);

    // The last job ends: the result comes, and it is the second thing the chief hears.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Both runs are green: committed as fe76a3c");
    passes(&context, 2);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&tiers),
        ["Both runs are green: committed as fe76a3c"]
    );
    assert_eq!(tiers.notes("chief").len(), 1, "no note beside the result");
}

#[test]
fn the_note_names_the_commands_what_is_known_of_them_and_when_the_result_comes() {
    // The field board's T-299: a foreground command with a 10-minute limit that Claude Code
    // moved to the background, which ran 100 minutes. The reader says when it began and
    // that a limit moved it, and the one note names it.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.adapter.answer_leaving_commands(
        "zeus",
        "The suite is running in the background",
        pending(1, 0),
        vec![Command {
            id: "bfxgkew9o".into(),
            since: Some(START_MS - 90 * 60_000),
            limit_ms: Some(600_000),
        }],
    );
    context.pass().unwrap();
    assert_eq!(
        tiers.notes("chief"),
        ["T-1 is still working in the background (1 command, running since 10:30 UTC, moved to the background when it passed its 10-minute limit); its result comes when it ends. `cf task finish T-1` takes what it said last now.\n\nWhat it said:\n\nThe suite is running in the background"]
    );
}

#[test]
fn the_note_says_the_kinds_apart_a_day_apart_and_a_bare_count_where_the_reader_cannot_say() {
    /// What a turn left running as its reading says it, and what the note says of it.
    struct Case {
        commands: Vec<Command>,
        pending: Pending,
        monitors: Vec<Monitor>,
        said: &'static str,
    }
    // Each in a project of its own.
    let cases = [
        Case {
            commands: vec![Command {
                id: "b1".into(),
                since: Some(START_MS - 3_600_000),
                limit_ms: None,
            }],
            pending: pending(2, 0),
            monitors: vec![monitor("ticks", Source::Command, None)],
            said: "(1 command, running since 11:00 UTC; 1 monitor)",
        },
        Case {
            commands: vec![
                Command {
                    id: "b1".into(),
                    since: Some(START_MS - 86_400_000),
                    limit_ms: Some(590_000),
                },
                Command {
                    id: "b2".into(),
                    since: Some(START_MS - 60_000),
                    limit_ms: None,
                },
            ],
            pending: pending(2, 0),
            monitors: vec![],
            said: "(2 commands, the oldest running since 2026-09-18 12:00 UTC, 1 of them moved to the background when it passed its 590-second limit)",
        },
        Case {
            commands: vec![],
            pending: Pending {
                agents: 2,
                ..pending(3, 0)
            },
            monitors: vec![],
            said: "(3 commands, 2 agents)",
        },
        Case {
            commands: vec![],
            pending: pending(1, 1),
            monitors: vec![monitor("ticks", Source::Command, Some(End::Expired))],
            said: "(1 monitor that said it ended)",
        },
    ];
    for Case {
        commands,
        pending,
        monitors,
        said,
    } in cases
    {
        let context = Context::new();
        let tiers = zeus_at_work(&context);
        context.background.commands_run(Some(2));
        context
            .adapter
            .answer_leaving_commands("zeus", "Going on", pending, commands);
        context
            .adapter
            .with("zeus", |agent| agent.monitors = monitors);
        passes(&context, 4);
        // A monitor that said it ended is looked at twice before its requester is told.
        context.advance(1_000);
        passes(&context, 2);
        let told = hold_notes(&tiers);
        assert_eq!(told.len(), 1, "{said}: {:?}", tiers.notes("chief"));
        assert!(
            told[0].starts_with(&format!("T-1 is still working in the background {said}; ")),
            "{said}: {told:?}"
        );
    }
}

#[test]
fn a_hold_that_turns_into_one_nothing_ends_but_finish_is_told_once_more_and_no_third_time() {
    // The one change that still needs the requester: every job left is a monitor that said
    // it ended and has no command of the window to look for (a socket's), so no pass of
    // the engine ends the hold, and the first note said the result comes when it ends.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.background.commands_run(Some(1));
    context.adapter.answer_leaving_monitors(
        "zeus",
        "Watching the feed",
        pending(1, 0),
        vec![monitor("feed", Source::Socket, None)],
    );
    context.pass().unwrap();
    assert_eq!(
        hold_notes(&tiers),
        ["T-1 is still working in the background (1 monitor); its result comes when it ends. `cf task finish T-1` takes what it said last now.\n\nWhat it said:\n\nWatching the feed"]
    );
    // Its socket closes: the monitor said it ended, and nothing in the window is its.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer_leaving_monitors(
        "zeus",
        "The feed closed; nothing more to watch",
        pending(1, 1),
        vec![monitor("feed", Source::Socket, Some(End::Closed))],
    );
    passes(&context, 4);
    let told = hold_notes(&tiers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(
        told[1],
        "T-1 is still working in the background (1 monitor that said it ended); ConsensFlow cannot tell when it ends, so its result will not come by itself. `cf task finish T-1` takes what it said last.\n\nWhat it said:\n\nThe feed closed; nothing more to watch"
    );
    // Any later change is told no more.
    for round in 0..3 {
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        context.adapter.answer_leaving_monitors(
            "zeus",
            &format!("Still nothing {round}"),
            pending(1, 1),
            vec![monitor("feed", Source::Socket, Some(End::Closed))],
        );
        passes(&context, 3);
    }
    assert_eq!(hold_notes(&tiers).len(), 2);
    // Finish takes what it said last.
    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
}

#[test]
fn a_hold_that_cannot_end_by_itself_at_its_first_note_is_told_so_in_that_one_note() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.background.commands_run(None);
    context.adapter.answer_leaving_monitors(
        "zeus",
        "Watching",
        pending(1, 1),
        vec![monitor("ticks", Source::Command, Some(End::Expired))],
    );
    passes(&context, 3);
    assert_eq!(
        hold_notes(&tiers),
        ["T-1 is still working in the background (1 monitor that said it ended); ConsensFlow cannot tell when it ends, so its result will not come by itself. `cf task finish T-1` takes what it said last.\n\nWhat it said:\n\nWatching"]
    );
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(tiers.notes("chief").len(), 1);
}

#[test]
fn a_held_task_whose_window_goes_is_not_left_silent_by_the_one_note_it_was_given() {
    // The notes that remain: the hold ends with no result, the window gone. The chief hears
    // it, whatever the hold note said, and the task is not working any more.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context
        .adapter
        .answer_leaving("zeus", "Watching the run", pending(1, 0));
    context.pass().unwrap();
    assert_eq!(hold_notes(&tiers).len(), 1);
    context.exit("zeus");
    passes(&context, 3);
    assert_eq!(tiers.task(1).task.state, "paused");
    let told = tiers.notes("chief");
    assert!(
        told.len() >= 2 && told[1..].iter().any(|note| note.contains("T-1")),
        "the chief hears the pause: {told:?}"
    );
}
