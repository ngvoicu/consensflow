//! A worker's result is not lost because an earlier turn of its ended with the write-up and was
//! not taken (T-1219; the class of T-1204, from another door). The board's T-1200, 2026-11-04:
//! a command the worker's Bash tool had moved to the background ended while the turn was at
//! work, and Claude Code queued its notification; the turn ended with the 8.8 KB write-up with
//! that message still queued, which keeps a turn in flight, so the board took no result of it and
//! held nothing; the notification began the next turn 0.1 s later and its three lines ("My result
//! … is unchanged") became the result. The write-up was in the window's record and nowhere on
//! the board.
//!
//! Now the result begins with the words of each turn that ended before the last and was not
//! taken, labelled with what began the next, and ends with "What it said last:" and the last
//! words: for a turn held for a job that runs, for one in flight on a queued notice, for one
//! the words of a note told before the window was up came after. What a turn wrote of where it
//! stands (500 characters or fewer) is not in it. The rule is on the window's record
//! (`turn_end::earlier_turns`), so it is the same whatever harness wrote it.

use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::background::{results, working};
use crate::queued_tells::told_on_its_way;
use crate::result_while_asking::on_the_way;

/// What the notice of a finished background command says, as Claude Code writes it.
const ENDED: &str =
    "<task-notification>\n<task-id>bufeuti75</task-id>\n<status>completed</status>\n</task-notification>";

/// The write-up of T-1200, shortened, but over the 500 characters of a line about where it stands.
const WRITE_UP: &str = "# T-1200: integration a2-x is merged, checked and pushed\n\nThe three branches are merged onto a3-1 and the checks are green: the dispatcher suite passes in full, the rig cases pass on the Mac and on zeewin2, and the plant tables of both branches are caught. The pins moved by 4 and 6 bytes and stay under their ceilings. The build folder was removed (about 400 GB) and the tree is clean.\n\nOpen points: the Windows leg of the Muse cases waits for T-1084's isolation, and the notice of the removal should name the folder it freed. Head 1a2b3c4d5e6f on origin/wt/integration-a2-x.";

/// What the turn the notice began ended with.
const UNCHANGED: &str =
    "My result for T-1200 above is unchanged. The removal ended; nothing else is left running.";

/// The label of a turn that a notice of the harness's followed.
const BEFORE_A_NOTICE: &str =
    "Written before a notice of its harness's (or words typed into its window) began its next turn:";

/// The result of a write-up and then a last turn, with the label of what began the last.
fn with_write_up(label: &str, write_up: &str, last: &str) -> String {
    format!("{label}\n\n{write_up}\n\nWhat it said last:\n\n{last}")
}

/// The command T-1200's worker ran: the harness moved it to the background when its time ran out,
/// and the worker waited for it with a command of its own.
fn ran_a_command_in_the_background(context: &Context) {
    let mut moved = context
        .adapter
        .item(Role::Assistant, "Waiting for the removal to end");
    moved.complete = false;
    let started = context.adapter.item(
        Role::Tool,
        "Command running in background with ID: bufeuti75. Output is being written to: /tmp/claude/tasks/bufeuti75.output",
    );
    let waited = context.adapter.item(Role::Tool, "No matches found");
    context.adapter.with("zeus", |agent| {
        agent.items.extend([moved, started, waited]);
    });
}

/// The harness's notice that a job ended begins a turn of the window's own: `marked` as Claude
/// Code's reader marks it (the harness's own item), or not (a window that wakes its agent with
/// an item that says nothing of what wrote it).
fn notice(context: &Context, said: &str, marked: bool) {
    context.adapter.wake("zeus", said);
    context.adapter.with("zeus", |agent| {
        if let Some(item) = agent.items.last_mut() {
            item.synthetic = marked;
        }
    });
}

/// T-1200's order: the write-up ends the turn while the harness still holds its notice (the
/// turn reads in flight, nothing pending), the notice begins the next turn, and that turn's
/// short words end it. The write-up is in the result, before them.
#[test]
fn a_write_up_that_ended_in_flight_on_a_queued_notice_is_in_the_result_before_the_notices_turn() {
    let context = Context::new();
    let tiers = working(&context);
    ran_a_command_in_the_background(&context);
    context.adapter.answer("zeus", WRITE_UP);
    // A queued message keeps the turn in flight: the record has no end for it yet.
    context.adapter.with("zeus", |agent| agent.settled = false);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "no result, nothing held"
    );
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());

    notice(&context, ENDED, true);
    context.adapter.answer("zeus", UNCHANGED);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [with_write_up(BEFORE_A_NOTICE, WRITE_UP, UNCHANGED)],
        "the write-up, then the three lines"
    );
}

/// The turn is held for a job that runs (a pass saw it settled with the job pending): its
/// requester is told once, with the words; the job ends, the notice begins a turn, and the
/// result is the write-up and then that turn's words.
#[test]
fn a_write_up_held_for_a_job_is_in_the_result_before_the_turn_the_jobs_end_woke() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    let told = tiers.notes("chief");
    assert_eq!(told.len(), 1, "the hold is told once: {told:?}");
    assert!(told[0].contains(WRITE_UP), "with the words: {told:?}");

    notice(&context, ENDED, true);
    context.adapter.answer("zeus", UNCHANGED);
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [with_write_up(BEFORE_A_NOTICE, WRITE_UP, UNCHANGED)]
    );
}

/// Whatever the harness's record, and whether its reader marks the notice as its own or not.
#[test]
fn every_harnesss_held_write_up_is_in_the_result_before_the_turn_its_notice_began() {
    for harness in ["claude", "codex", "pi", "devin", "muse"] {
        for marked in [true, false] {
            let context = Context::new();
            let tiers = working(&context);
            let written = on_the_way(&context, harness);
            context
                .adapter
                .with("zeus", |agent| agent.items.extend(written));
            context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
            context.pass().unwrap();
            context.pass().unwrap();
            assert_eq!(tiers.task(1).task.state, "working", "{harness}");

            notice(&context, ENDED, marked);
            let written = on_the_way(&context, harness);
            context
                .adapter
                .with("zeus", |agent| agent.items.extend(written));
            context.adapter.answer("zeus", UNCHANGED);
            context.pass().unwrap();

            assert_eq!(tiers.task(1).task.state, "done", "{harness}");
            assert_eq!(
                results(&context, &tiers),
                [with_write_up(BEFORE_A_NOTICE, WRITE_UP, UNCHANGED)],
                "{harness}, marked: {marked}"
            );
        }
    }
}

/// Where a held turn stands is not a report: a line of at most 500 characters before a notice
/// is not in the result, whatever the turn did.
#[test]
fn where_a_held_turn_stands_is_not_in_the_result_up_to_500_characters() {
    for (length, kept) in [(500, false), (501, true)] {
        let context = Context::new();
        let tiers = working(&context);
        ran_a_command_in_the_background(&context);
        let line = "a".repeat(length);
        context.adapter.answer_leaving_a_job("zeus", &line);
        context.pass().unwrap();
        context.pass().unwrap();
        notice(&context, ENDED, true);
        context.adapter.answer("zeus", UNCHANGED);
        context.pass().unwrap();

        let expected = if kept {
            with_write_up(BEFORE_A_NOTICE, &line, UNCHANGED)
        } else {
            UNCHANGED.to_owned()
        };
        assert_eq!(results(&context, &tiers), [expected], "{length}");
    }
}

/// The turns a task's jobs woke, one after the other: each long turn is kept, in order.
#[test]
fn the_write_ups_of_two_held_turns_come_in_order() {
    let context = Context::new();
    let tiers = working(&context);
    let first = format!("{WRITE_UP}\n\nThe first run is over; the second begins.");
    context.adapter.answer_leaving_a_job("zeus", &first);
    context.pass().unwrap();
    notice(&context, ENDED, true);
    context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held again");
    notice(&context, ENDED, true);
    context.adapter.answer("zeus", UNCHANGED);
    context.pass().unwrap();

    assert_eq!(
        results(&context, &tiers),
        [format!(
            "{BEFORE_A_NOTICE}\n\n{first}\n\n{BEFORE_A_NOTICE}\n\n{WRITE_UP}\n\nWhat it said last:\n\n{UNCHANGED}"
        )]
    );
}

/// A delivery that went in right after: the chief's words told before the window was up go in
/// after the brief, and the answer to the brief was no result. Its words are in the result.
#[test]
fn a_write_up_the_words_told_before_the_window_was_up_came_after_is_in_the_result() {
    let context = Context::new();
    let (tiers, _session, told) = told_on_its_way(&context);
    context.adapter.answer("zeus", WRITE_UP);
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "no result while the words wait"
    );
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Parser done, on the v2 grammar");
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(
        results(&context, &tiers),
        [with_write_up(
            &format!("Written before the note m-{told} went in:"),
            WRITE_UP,
            "Parser done, on the v2 grammar"
        )]
    );
}

/// ...and what it wrote of where it stands is not.
#[test]
fn a_short_answer_the_told_words_came_after_is_not_in_the_result() {
    let context = Context::new();
    let (tiers, _session, _told) = told_on_its_way(&context);
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Parser done, on the v2 grammar");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        results(&context, &tiers),
        ["Parser done, on the v2 grammar"]
    );
}

/// Finish at the held turn: the write-up is what the worker said last, and the result is it, as
/// it always was.
#[test]
fn the_chief_finishing_a_held_task_takes_the_write_up_alone() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");

    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with(WRITE_UP), "{results:?}");
    assert!(!results[0].contains("Written before"), "{results:?}");
}

/// Finish while the notice's turn is at work takes the last complete answer, which is still the
/// write-up: not given twice.
#[test]
fn the_chief_finishing_while_the_notices_turn_is_at_work_gets_the_write_up_once() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
    context.pass().unwrap();
    notice(&context, ENDED, true);
    context.pass().unwrap();

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with(WRITE_UP), "{results:?}");
    assert!(!results[0].contains("Written before"), "{results:?}");
    assert_eq!(results[0].matches("Open points").count(), 1, "{results:?}");
}

/// Finish at the notice's turn, which a job holds again: the result is the write-up and then
/// that turn's words, as the turn's own end would have recorded them.
#[test]
fn the_chief_finishing_the_held_turn_after_a_notice_gives_the_write_up_too() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer_leaving_a_job("zeus", WRITE_UP);
    context.pass().unwrap();
    notice(&context, ENDED, true);
    context.adapter.answer_leaving_a_job("zeus", UNCHANGED);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "held again");

    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].starts_with(&with_write_up(BEFORE_A_NOTICE, WRITE_UP, UNCHANGED)),
        "{results:?}"
    );
}
