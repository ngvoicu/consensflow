//! A window's harness keeps its own processes, its MCP servers and its helpers, and
//! what they start, and none of it is work a turn left running, whenever each begins:
//! found on 3.0.1, where the servers a Claude worker's harness starts as it takes its
//! first message, and the children of the plugin servers a Codex designer's window
//! opens with, held the tasks for ever. A close ends what the host can tell is the
//! window's, and what is the harness's is listed as no turn's work. What is the
//! harness's is the pane host's to tell (`cf-panes`'
//! `background`, which proves what is the turn's and leaves what it cannot alone), by
//! how the harness keeps its servers (its adapter's word, `Adapter::servers`) and by
//! the programs it is named for; these hold what the engine does: it says what the
//! adapter said, and which harness the window is, for the window the turn is in.

use cf_engine::testing::Context;
use cf_proto::panes::Servers;
use serde_json::json;

use crate::background::results;
use crate::chiefs::with_codex;
use crate::fixtures::Tiers;

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
fn a_codex_window_is_marked_as_codex_and_as_opening_its_servers_with_it() {
    let context = with_codex();
    context.codex.servers.set(Servers::AtLaunch);
    let _tiers = with_a_codex_worker(&context);

    assert_eq!(context.background.servers_said(), [Servers::AtLaunch]);
    assert_eq!(context.background.harness_said(), ["codex"]);
}

#[test]
fn a_harness_that_says_nothing_of_how_it_keeps_its_servers_is_marked_so_and_the_host_goes_by_what_it_can_prove(
) {
    // OpenCode's and Devin's were not probed: their adapters say nothing. The mark
    // says so, and the host lists at a close what it proves the turn's; the result
    // ends nothing.
    let context = with_codex();
    let tiers = with_a_codex_worker(&context);
    context.codex.answer("zeus", "Started the recording run");
    context.pass().unwrap();

    assert_eq!(context.background.servers_said(), [Servers::Unknown]);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results(&context, &tiers), ["Started the recording run"]);
}
