//! A window's harness keeps its own processes, its MCP servers, and what they
//! start, and none of it is work a turn left running, whenever each begins: found on
//! 3.0.1, where the servers a Claude worker's harness starts as it takes its first
//! message, and the children of the plugin servers a Codex designer's window opens
//! with, held the tasks for ever. How the harness keeps them is its adapter's word
//! (`Adapter::servers`), which the engine tells the pane host with each mark; what
//! the host does with it is the host's, and these hold what the engine does: it says
//! what the adapter said, for the window the turn is in, and what the host then
//! lists is the work the task is held for, the agent's own command beside it.

use cf_engine::testing::Context;
use cf_proto::panes::Servers;
use serde_json::json;

use crate::background::{doing, results, LOCK, LOG, SETTLE_MS};
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
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    context.background.serve("zeus", &[PLAYWRIGHT, DEVTOOLS]);
    ready();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    tiers
}

#[test]
fn servers_the_harness_started_after_the_mark_hold_nothing_where_it_keeps_them_in_its_group() {
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
}

#[test]
fn a_job_in_a_group_of_its_own_holds_the_result_beside_servers_that_are_not_counted_or_named() {
    let context = Context::new();
    context.adapter.servers.set(Servers::InGroup);
    let tiers = answered_with_servers_running(&context, || {
        context.background.run("zeus", &[LOCK]);
    });

    assert_eq!(tiers.task(1).task.state, "working", "held for the job");
    assert_eq!(
        doing(&context, &tiers).reason,
        Some(format!("background work running (1 process): {LOCK}"))
    );
    assert_eq!(results(&context, &tiers), Vec::<String>::new());

    // The job ends: what is left is the harness's, and the turn's end is the result.
    context.background.end("zeus");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the agent may be woken"
    );
    context.advance(SETTLE_MS);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Parser done"]);
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
fn a_harness_not_known_to_keep_its_servers_any_way_is_held_for_them_as_before() {
    // An adapter that says nothing of how its harness keeps its servers (OpenCode's and
    // Devin's were not probed): the host tells them from the turn's work by when they began.
    let context = with_codex();
    let tiers = with_a_codex_worker(&context);
    context.background.serve("zeus", &[PLAYWRIGHT]);
    context.background.serve_from_launch("zeus", &[REPL_WORKER]);
    context.codex.answer("zeus", "Parser done");
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "working", "held for the server");
    assert_eq!(
        doing(&context, &tiers).reason,
        Some(format!(
            "background work running (2 processes): {PLAYWRIGHT}; {REPL_WORKER}"
        ))
    );
    assert_eq!(context.background.servers_said(), [Servers::Unknown]);
}

#[test]
fn what_a_server_a_codex_window_opened_with_starts_later_holds_nothing_for_the_designer() {
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
}

#[test]
fn a_command_of_a_codex_agent_holds_the_result_beside_what_the_servers_it_opened_with_started() {
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

    assert_eq!(tiers.task(1).task.state, "working", "held for the command");
    assert_eq!(
        doing(&context, &tiers).reason,
        Some(format!(
            "background work running (2 processes): {LOG}; {LOCK}"
        ))
    );
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}
