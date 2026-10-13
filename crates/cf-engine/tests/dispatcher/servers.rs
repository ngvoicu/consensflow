//! A window's harness keeps its own processes, its MCP servers, and what they
//! start, and none of it is work a turn left running, whenever each begins: found on
//! 3.0.1, where the servers a Claude worker's harness starts as it takes its first
//! message, and the children of the plugin servers a Codex designer's window opens
//! with, held the tasks for ever. Since the owner's decision of 2026-10-13 what a
//! turn left running is ended, so what is the harness's must never be: it would break
//! the worker's servers. How the harness keeps them is its adapter's word
//! (`Adapter::servers`), which the engine tells the pane host with each mark; what
//! the host does with it is the host's, and these hold what the engine does: it says
//! what the adapter said, for the window the turn is in, asks the host to end what
//! the host lists of the turn's, the agent's own command beside the servers, and
//! says what the host would not end.

use cf_engine::testing::Context;
use cf_proto::panes::Servers;
use serde_json::json;

use crate::background::{results, working, LOCK, LOG};
use crate::chiefs::with_codex;
use crate::fixtures::Tiers;

/// The servers of a user's own that Claude Code starts as it takes its first
/// message, which is after the daemon marked where its turn began.
const PLAYWRIGHT: &str = "npm exec @playwright/mcp@latest";
const DEVTOOLS: &str = "npm exec chrome-devtools-mcp@1.9.0";

/// T-1 given to zeus, whose harness started its servers after the mark (they stay
/// in the group of the window's program), and whose agent answers and ends its turn:
/// the pass that looks at it is made. `ready` is what runs before the answer.
fn answered_with_servers_running<'a>(context: &'a Context, ready: impl FnOnce()) -> Tiers<'a> {
    let tiers = working(context);
    context.background.serve("zeus", &[PLAYWRIGHT, DEVTOOLS]);
    ready();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    tiers
}

#[test]
fn servers_the_harness_started_after_the_mark_are_no_work_of_the_turn_where_it_keeps_them_in_its_group(
) {
    // Found on 3.0.1: a Claude worker's task was never done when the user's own
    // MCP servers began after the first message showed in its record.
    let context = Context::new();
    context.adapter.servers.set(Servers::InGroup);
    let tiers = answered_with_servers_running(&context, || {});

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(
        context.background.servers_said(),
        [Servers::InGroup],
        "the mark said how the harness keeps its servers"
    );
    // They are not the turn's: nothing is asked to end and nothing is said.
    assert_eq!(context.background.endings_asked("zeus"), 0);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn a_job_in_a_group_of_its_own_is_ended_beside_servers_that_are_not_asked_for_or_named() {
    let context = Context::new();
    context.adapter.servers.set(Servers::InGroup);
    let tiers = answered_with_servers_running(&context, || {
        context.background.run("zeus", &[LOCK]);
    });

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Ended: python3 lock.py record --run 7."]
    );
}

/// What a REPL a Codex window was opened with starts after the mark, as a tool
/// is called: the workers of the call.
const REPL_WORKER: &str = "node /tmp/node_repl/worker.js";

/// T-1 given to zeus, a Codex worker, whose window is at work on it: its turn is
/// marked, as it will be when the first message shows in the record.
fn with_a_codex_worker(context: &Context) -> Tiers<'_> {
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "codex", "role": "worker", "tier": "standard" }],
        }))
        .unwrap()
        .unwrap();
    let tiers = Tiers { context, project };
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

#[test]
fn a_harness_not_known_to_keep_its_servers_any_way_has_none_of_what_it_lists_by_time_alone_ended() {
    // An adapter that says nothing of how its harness keeps its servers (OpenCode's and
    // Devin's were not probed): the host lists them by when they began, cannot tell
    // them from the turn's work, and ends none of them. The result goes at once and
    // says they still run.
    let context = with_codex();
    let tiers = with_a_codex_worker(&context);
    context.background.serve("zeus", &[PLAYWRIGHT]);
    context.background.serve_from_launch("zeus", &[REPL_WORKER]);
    context.codex.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(context.background.servers_said(), [Servers::Unknown]);
    assert_eq!(context.background.endings_asked("zeus"), 1, "it was asked");
    assert_eq!(
        context.background.ended("zeus"),
        Vec::<String>::new(),
        "and would end none of them"
    );
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1's turn left background work running. Still running, not ended: {PLAYWRIGHT}; {REPL_WORKER}."
        )]
    );
}

#[test]
fn what_a_server_a_codex_window_opened_with_starts_later_is_no_work_of_the_designers_turn() {
    // Found on 3.0.1: Codex designers' tasks (T-32, T-34) were never done: the
    // children of a REPL the window was opened with began after the first message
    // showed in its record, and the tasks that waited on them (`--after`) never went.
    let context = with_codex();
    context.codex.servers.set(Servers::AtLaunch);
    let tiers = with_a_codex_worker(&context);
    context
        .background
        .serve_from_launch("zeus", &[REPL_WORKER, REPL_WORKER]);
    context.codex.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
    assert_eq!(
        context.background.servers_said(),
        [Servers::AtLaunch],
        "the mark said the harness opens its servers with its window"
    );
    assert_eq!(context.background.endings_asked("zeus"), 0);
    assert_eq!(tiers.notes("chief"), Vec::<String>::new());
}

#[test]
fn a_command_of_a_codex_agent_is_ended_beside_what_the_servers_it_opened_with_started() {
    let context = with_codex();
    context.codex.servers.set(Servers::AtLaunch);
    let tiers = with_a_codex_worker(&context);
    context
        .background
        .serve_from_launch("zeus", &[REPL_WORKER, REPL_WORKER]);
    context.background.run("zeus", &[LOCK]);
    // What the harness starts in its window's own group after the mark is the
    // agent's here: only what a server opened with the window started is its own.
    context.background.serve("zeus", &[LOG]);
    context.codex.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.background.ended("zeus"), [LOG, LOCK]);
    assert_eq!(
        tiers.notes("chief"),
        ["T-1's turn left background work running. Ended: tail -f run.log; python3 lock.py record --run 7."]
    );
}

#[test]
fn the_job_of_a_worker_whose_servers_the_host_cannot_tell_is_ended_beside_them_and_they_are_left() {
    // Where the harness is not known to keep its servers, what an agent's shell ran
    // is ended and what the host lists beside it by time alone is left, and said.
    let context = with_codex();
    let tiers = with_a_codex_worker(&context);
    context.background.serve("zeus", &[PLAYWRIGHT]);
    context.background.run("zeus", &[LOCK]);
    context.codex.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.background.ended("zeus"), [LOCK]);
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1's turn left background work running. Ended: {LOCK}. Still running, not ended: {PLAYWRIGHT}."
        )]
    );
}
