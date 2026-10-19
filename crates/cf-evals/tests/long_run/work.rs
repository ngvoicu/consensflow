//! 1 (A1): a working task whose window rested with no job held; 2 (A2): a task done
//! while its Claude Code window's background job still ran.

use cf_evals::long_run::look::{OpenWindow, Process};
use cf_evals::long_run::record::Record;
use serde_json::json;

use crate::scene::{found, ms, t, Scene, CHIEF};

/// The worker (3), on `harness`, with T-4 working since 10:00 and a conversation
/// whose copy ends with a turn that ended at 10:05, whose tool said `tool`.
fn rested_after(harness: &str, tool: &str) -> (Scene, i64) {
    rested_among(&[("long-worker", harness)], tool)
}

/// [`rested_after`], the worker the first of `staff`.
fn rested_among(staff: &[(&str, &str)], tool: &str) -> (Scene, i64) {
    let harness = staff[0].1;
    let scene = Scene::new(staff);
    let conversation = scene.conversation(3, harness, "s-worker");
    scene.task(4, 3, "working", &t(0, 0));
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-1 · T-4 · task from @chief]\nBuild it",
        true,
        &t(0, 1),
    );
    scene.item(
        conversation,
        1,
        "assistant",
        "Building now.",
        false,
        &t(1, 0),
    );
    scene.item(conversation, 2, "tool", tool, true, &t(4, 0));
    scene.item(
        conversation,
        3,
        "assistant",
        "Built: 36 pages.",
        true,
        &t(5, 0),
    );
    (scene, conversation)
}

/// [`rested_after`] a build that is done.
fn rested(harness: &str) -> (Scene, i64) {
    rested_after(harness, "npm run build: done")
}

#[test]
fn a_working_task_whose_window_has_rested_past_the_idle_time_with_no_job_breaks_the_first() {
    let (scene, _) = rested("codex");
    let breaks = found(&scene.look(ms(26, 0)), 1);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(5, 0)));
    assert!(
        breaks[0].said.contains("T-4 is working"),
        "{}",
        breaks[0].said
    );
    assert!(breaks[0]
        .evidence
        .record
        .iter()
        .any(|line| line.contains("Built: 36 pages.")));
}

#[test]
fn a_window_at_rest_for_less_than_the_idle_time_or_in_a_turn_holds_the_first() {
    let (scene, conversation) = rested("codex");
    assert!(
        found(&scene.look(ms(20, 0)), 1).is_empty(),
        "15 minutes at rest"
    );
    // A turn at work: the chief told it something, and it has not answered.
    scene.item(
        conversation,
        4,
        "user",
        "[ConsensFlow m-2 · T-4 · note from @chief]\nAnd the tests",
        true,
        &t(6, 0),
    );
    assert!(
        found(&scene.look(ms(90, 0)), 1).is_empty(),
        "a long turn is no fault"
    );
}

#[test]
fn a_job_its_record_holds_or_its_turn_left_running_holds_the_first() {
    let (scene, _) = rested_after("claude-code", "Command running in background with ID: b7x2");
    assert!(
        found(&scene.look(ms(60, 0)), 1).is_empty(),
        "the job holds the task"
    );
    // Its end, said by Claude Code itself, lets the task go: then it is held for nothing
    // (its own record read at the look, saying nothing the copy does not).
    let (scene, conversation) =
        rested_after("claude-code", "Command running in background with ID: b7x2");
    scene.item(
        conversation,
        4,
        "user",
        "<task-notification>\n<task-id>b7x2</task-id>\n</task-notification>",
        true,
        &t(6, 0),
    );
    scene.item(
        conversation,
        5,
        "assistant",
        "The build is done.",
        true,
        &t(6, 30),
    );
    assert_eq!(
        found(
            &scene.look_with(ms(60, 0), conversation, Record::claude("")),
            1
        )
        .len(),
        1
    );
    // A harness whose record says nothing of jobs: what the pane host lists.
    let (scene, _) = rested("opencode");
    let mut look = scene.look(ms(60, 0));
    look.windows.open.push(OpenWindow {
        handle: "long-worker".to_owned(),
        pane: "p1-long-worker-a".to_owned(),
        generation: 1,
        pid: Some(4242),
        running: true,
        hosted: true,
        background: vec![Process {
            pid: 4300,
            command: "node scripts/longrun-dev-server.mjs".to_owned(),
        }],
        background_total: 1,
        tree_unread: None,
        tail: Vec::new(),
    });
    assert!(found(&look, 1).is_empty());
}

#[test]
fn a_claude_window_is_read_from_its_own_record_whatever_the_copy_says() {
    let (scene, conversation) = rested("claude-code");
    // The copy has a prompt not yet answered; the record says the turn ended at 10:06.
    scene.item(conversation, 4, "user", "late copy", true, &t(5, 30));
    let record = Record::claude(
        &[
            json!({ "timestamp": t(5, 30), "type": "user", "message": { "role": "user", "content": "late copy" } }),
            json!({ "timestamp": t(5, 50), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Noted." }] } }),
            json!({ "timestamp": t(6, 0), "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 30000, "messageCount": 2 }),
        ]
        .map(|line| line.to_string())
        .join("\n"),
    );
    let breaks = found(&scene.look_with(ms(30, 0), conversation, record), 1);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(6, 0)));
}

#[test]
fn background_agents_a_claude_turn_left_running_hold_the_first_until_it_says_none() {
    // The turn ended at 10:06 with two background agents running, which wake the
    // worker as each ends: the product keeps T-4 working, and so may it be for long.
    let (scene, conversation) = rested("claude-code");
    let ended_with = |minute: u32, pending: Option<u64>| {
        let mut duration = json!({ "timestamp": t(minute, 0), "type": "system", "subtype": "turn_duration",
            "isSidechain": false, "durationMs": 9000, "messageCount": 2 });
        if let Some(count) = pending {
            duration["pendingBackgroundAgentCount"] = json!(count);
        }
        [
            json!({ "timestamp": t(minute - 1, 0), "type": "user", "message": { "role": "user", "content": "Go on" } }),
            json!({ "timestamp": t(minute - 1, 30), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Waiting on the agents." }] } }),
            duration,
        ]
    };
    let text = |lines: &[serde_json::Value]| {
        lines
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let record = Record::claude(&text(&ended_with(6, Some(2))));
    assert!(found(&scene.look_with(ms(60, 0), conversation, record), 1).is_empty());
    // The turn their ends woke said none run any more: at rest from then.
    let mut lines = ended_with(6, Some(2)).to_vec();
    lines.extend(ended_with(20, None));
    let breaks = found(
        &scene.look_with(ms(60, 0), conversation, Record::claude(&text(&lines))),
        1,
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(20, 0)));
    // A later turn that ended at its stop hook's summary alone, which says nothing of
    // agents, carries over none of the turn before's.
    let mut lines = ended_with(6, Some(2)).to_vec();
    lines.extend([
        json!({ "timestamp": t(19, 0), "type": "user", "message": { "role": "user", "content": "Go on" } }),
        json!({ "timestamp": t(19, 30), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "All in." }] } }),
        json!({ "timestamp": t(20, 0), "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": false }),
    ]);
    let breaks = found(
        &scene.look_with(ms(60, 0), conversation, Record::claude(&text(&lines))),
        1,
    );
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(20, 0)));
}

#[test]
fn a_claude_window_whose_stop_hook_still_runs_is_in_its_turn() {
    // The answer is written at 10:06 and a stop hook runs for half an hour: the turn is
    // not over until its boundary, and a long hook is no rest (A1 found it at rest).
    let (scene, conversation) = rested("claude-code");
    let answered = [
        json!({ "timestamp": t(5, 30), "type": "user", "message": { "role": "user", "content": "And the docs" } }),
        json!({ "timestamp": t(6, 0), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Docs done." }] } }),
    ];
    let lines = |more: &[serde_json::Value]| {
        answered
            .iter()
            .chain(more)
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let record = Record::claude(&lines(&[]));
    assert!(found(&scene.look_with(ms(40, 0), conversation, record), 1).is_empty());
    // A hook that prevented the end: the agent goes on.
    let record = Record::claude(&lines(&[
        json!({ "timestamp": t(7, 0), "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": true }),
    ]));
    assert!(found(&scene.look_with(ms(40, 0), conversation, record), 1).is_empty());
    // The hook's summary that let it end: at rest since then.
    let record = Record::claude(&lines(&[
        json!({ "timestamp": t(7, 0), "type": "system", "subtype": "stop_hook_summary", "preventedContinuation": false }),
    ]));
    let breaks = found(&scene.look_with(ms(40, 0), conversation, record), 1);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(7, 0)));
}

/// T-14 of a Claude Code worker (3): a job started at 10:01, the result m-131 at
/// 10:02.
fn job_then_result(harness: &str) -> (Scene, i64, i64) {
    let scene = Scene::new(&[("long-worker", harness)]);
    let conversation = scene.conversation(3, harness, "s-worker");
    let task = scene.task(14, 3, "done", &t(2, 0));
    scene.item(
        conversation,
        0,
        "user",
        "[ConsensFlow m-120 · T-14 · task from @chief]\nRun the mutations",
        true,
        &t(0, 30),
    );
    scene.item(
        conversation,
        1,
        "tool",
        "Command running in background with ID: b33m. Output is being written to: /tmp/b33m.output",
        true,
        &t(1, 0),
    );
    scene.item(
        conversation,
        2,
        "assistant",
        "The mutation run is going in the background. I'll write up the result once it finishes.",
        true,
        &t(1, 30),
    );
    scene.message(
        131,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "The mutation run is going in the background. I'll write up the result once it finishes.",
        &t(2, 0),
    );
    (scene, conversation, task)
}

#[test]
fn a_result_taken_while_its_claude_windows_job_still_ran_breaks_the_second() {
    let (scene, _, _) = job_then_result("claude-code");
    let breaks = found(&scene.look(ms(10, 0)), 2);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, "2:m-131");
    assert!(breaks[0].said.contains("b33m"), "{}", breaks[0].said);
    // A notification quoted in a message ConsensFlow delivered is no end of the job.
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", "[ConsensFlow m-125 · note from @chief]\n<task-notification><task-id>b33m</task-id></task-notification>", true, &t(1, 40));
    assert_eq!(found(&scene.look(ms(10, 0)), 2).len(), 1);
}

/// What a look could not judge of the second, by key.
fn unjudged_second(look: &cf_evals::long_run::look::Look) -> Vec<String> {
    cf_evals::long_run::invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 2)
        .map(|open| open.key)
        .collect()
}

#[test]
fn a_result_whose_native_record_was_not_read_through_its_turn_is_unresolved() {
    // Sol's: the copy's tool text names no job, and the Claude worker's own record,
    // where a job it started would be, was never read. No job seen is no job held.
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", B33M_ENDED, true, &t(1, 50));
    let look = scene.look(ms(10, 0));
    assert!(found(&look, 2).is_empty());
    assert_eq!(unjudged_second(&look), ["2:m-131"]);
    // Its record read through the turn: judged, and it holds.
    let entry = |at: String, text: &str| {
        json!({ "timestamp": at, "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": text }] } })
            .to_string()
    };
    let record = Record::claude(&entry(
        t(1, 30),
        "The mutation run is going in the background.",
    ));
    let look = scene.look_with(ms(10, 0), conversation, record);
    assert!(
        unjudged_second(&look).is_empty(),
        "{:?}",
        unjudged_second(&look)
    );
    // Read only as far as before the turn's prompt: it says nothing of the turn.
    let record = Record::claude(&entry(t(0, 10), "Ready."));
    let look = scene.look_with(ms(10, 0), conversation, record);
    assert_eq!(unjudged_second(&look), ["2:m-131"]);
}

/// Why the worker's own record could not be read at the look.
const UNREADABLE: &str = "its record could not be read: Input/output error (os error 5)";

#[test]
fn a_result_whose_record_was_not_read_through_its_turn_is_unresolved_whatever_jobs_the_copy_shows()
{
    // Both reviewers': the copy names the job b33m (A), whose end was told 5 s after the
    // result, so the second lets it go; the worker's own record, read at 10:01:40 before
    // its readings began to fail, names b44n (B) too, which ran on.
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", B33M_ENDED, true, &t(2, 5));
    let record = Record::claude(
        &json!({ "timestamp": t(1, 20), "type": "user", "toolUseResult": { "backgroundTaskId": "b44n" },
                 "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "y",
                 "content": "Command running in background with ID: b44n" }] } })
        .to_string(),
    );
    let look = scene.look_read(
        ms(10, 0),
        conversation,
        record.clone(),
        (ms(1, 40), UNREADABLE),
    );
    assert!(found(&look, 2).is_empty(), "{:?}", found(&look, 2));
    assert_eq!(unjudged_second(&look), ["2:m-131"]);
    // Read through the settle time after the result before its readings failed: what
    // was read stays read, and b44n ran on when the result was taken.
    let look = scene.look_read(ms(10, 0), conversation, record, (ms(3, 30), UNREADABLE));
    let breaks = found(&look, 2);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!(breaks[0].said.contains("b44n"), "{}", breaks[0].said);
    assert!(
        unjudged_second(&look).is_empty(),
        "{:?}",
        unjudged_second(&look)
    );
}

/// What a look could not judge of the first, by key.
fn unjudged_first(look: &cf_evals::long_run::look::Look) -> Vec<String> {
    cf_evals::long_run::invariants::unjudged(look, &Default::default())
        .into_iter()
        .filter(|open| open.invariant == 1)
        .map(|open| open.key)
        .collect()
}

#[test]
fn a_claude_windows_rest_its_own_record_was_not_read_through_is_unresolved() {
    // The copy says the window rested from 10:05, 21 minutes; its own record, which
    // would say whether a turn or a job went on since, was never read.
    let (scene, conversation) = rested("claude-code");
    let look = scene.look(ms(26, 0));
    assert!(found(&look, 1).is_empty(), "{:?}", found(&look, 1));
    assert_eq!(unjudged_first(&look), [format!("1:T-4:{}", ms(5, 0))]);
    // Read as far as 10:20 before its readings failed: no word of the look's moment.
    let look = scene.look_read(
        ms(26, 0),
        conversation,
        Record::claude(""),
        (ms(20, 0), UNREADABLE),
    );
    assert!(found(&look, 1).is_empty(), "{:?}", found(&look, 1));
    assert_eq!(unjudged_first(&look), [format!("1:T-4:{}", ms(5, 0))]);
    // Read at the look, and saying nothing the copy does not: judged.
    let look = scene.look_with(ms(26, 0), conversation, Record::claude(""));
    assert_eq!(found(&look, 1).len(), 1);
    assert!(unjudged_first(&look).is_empty());
}

#[test]
fn a_claude_windows_rest_is_read_from_its_conversation_open_now_whatever_the_copy_holds() {
    // Astra's: conversation 7 ended at 10:02 mid-turn; 8 is the one open now, its own
    // record at rest since 10:05 with no job; the copy is stuck on 7. Both records are
    // read through the look at 10:26.
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let old = scene.conversation(3, "claude-code", "s-old");
    scene.task(4, 3, "working", &t(0, 0));
    scene.item(
        old,
        0,
        "user",
        "[ConsensFlow m-1 · T-4 · task from @chief]\nBuild it",
        true,
        &t(0, 1),
    );
    scene.item(old, 1, "assistant", "Building now.", false, &t(1, 0));
    scene.end_conversation(old, &t(2, 0));
    let new = scene.conversation(3, "claude-code", "s-new");
    let entries = |lines: &[serde_json::Value]| {
        Record::claude(
            &lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    let mut look = scene.look(ms(26, 0));
    look.records.insert(
        old,
        entries(&[
            json!({ "timestamp": t(0, 1), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-1 · T-4 · task from @chief]\nBuild it" } }),
        ]),
    );
    look.records.insert(
        new,
        entries(&[
            json!({ "timestamp": t(4, 50), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Built: 36 pages." }] } }),
            json!({ "timestamp": t(5, 0), "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 10000, "messageCount": 2 }),
        ]),
    );
    let breaks = found(&look, 1);
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].key, format!("1:T-4:{}", ms(5, 0)));
    // No conversation open at the look: which holds the task's turns cannot be told.
    scene.end_conversation(new, &t(20, 0));
    scene.item(new, 0, "assistant", "Built: 36 pages.", true, &t(5, 0));
    let look = scene.look(ms(26, 0));
    assert!(found(&look, 1).is_empty(), "{:?}", found(&look, 1));
    assert_eq!(unjudged_first(&look), [format!("1:T-4:{}", ms(5, 0))]);
}

#[test]
fn a_rest_a_later_reading_reads_through_is_judged_as_of_its_moment() {
    // The look at 10:26 could not read the worker's record past 10:20; the copy said the
    // window rested from 10:05. A later look reads it through: as of 10:26, the record
    // says the window was at rest since 10:05 (a break), or in a turn begun at 10:25.
    use cf_evals::long_run::invariants::{unjudged, Keeper, Settings};
    use cf_evals::long_run::mask::Names;
    let settings = Settings::default();
    for (record, broke) in [
        (
            vec![
                json!({ "timestamp": t(5, 0), "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 10000, "messageCount": 2 }),
            ],
            true,
        ),
        (
            vec![
                json!({ "timestamp": t(25, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2 · T-4 · note from @chief]\nAnd the tests" } }),
            ],
            false,
        ),
    ] {
        let (scene, conversation) = rested("claude-code");
        let record = Record::claude(
            &record
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut keeper = Keeper::default();
        let first = scene.look_read(
            ms(26, 0),
            conversation,
            record.clone(),
            (ms(20, 0), UNREADABLE),
        );
        let open = unjudged(&first, &settings);
        keeper.keep_unjudged(first.at, open, &Names::default());
        assert_eq!(keeper.unjudged.len(), 1, "{:?}", keeper.unjudged);
        let later = scene.look_with(ms(27, 0), conversation, record);
        let filled = keeper.fill(&later, &settings, &unjudged(&later, &settings));
        assert_eq!(filled.len(), 1, "{filled:?}");
        assert_eq!(filled[0].1.is_some(), broke, "{filled:?}");
        assert!(keeper.unjudged.is_empty());
        assert_eq!(keeper.filled.len(), 1);
    }
}

/// A Claude Code record of `lines`.
fn claude_record(lines: &[serde_json::Value]) -> Record {
    Record::claude(
        &lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// A Claude Code record whose window came to rest at 10:05: an answer, and its turn's end.
fn at_rest_since_five() -> Record {
    claude_record(&[
        json!({ "timestamp": t(4, 50), "type": "assistant", "message": { "role": "assistant", "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Built: 36 pages." }] } }),
        json!({ "timestamp": t(5, 0), "type": "system", "subtype": "turn_duration", "isSidechain": false, "durationMs": 10000, "messageCount": 2 }),
    ])
}

#[test]
fn a_rest_is_judged_again_on_the_worker_it_was_taken_for_whatever_became_of_the_task() {
    // Both reviewers': at 10:26 T-4's window had rested since 10:05 with no job, its own
    // record unreadable past 10:20: unresolved. Before a reading holds, the task is let go
    // of (no worker) or given to another worker, who has no record of then.
    use cf_evals::long_run::invariants::{unjudged, Keeper, Settings};
    use cf_evals::long_run::mask::Names;
    let settings = Settings::default();
    for change in [
        "UPDATE task SET assignee_id = NULL, state = 'open', updated_at = ?1 WHERE number = 4",
        "UPDATE task SET assignee_id = 4, updated_at = ?1 WHERE number = 4",
    ] {
        let (scene, conversation) = rested_among(
            &[
                ("long-worker", "claude-code"),
                ("long-other", "claude-code"),
            ],
            "npm run build: done",
        );
        let mut keeper = Keeper::default();
        let first = scene.look_read(
            ms(26, 0),
            conversation,
            Record::claude(""),
            (ms(20, 0), UNREADABLE),
        );
        keeper.keep_unjudged(first.at, unjudged(&first, &settings), &Names::default());
        assert_eq!(keeper.unjudged.len(), 1, "{:?}", keeper.unjudged);
        scene.db.execute(change, [t(26, 30)]).unwrap();
        // Still unreadable: the gap stays open, never taken for a rest that held.
        let later = scene.look_read(
            ms(27, 0),
            conversation,
            Record::claude(""),
            (ms(20, 0), UNREADABLE),
        );
        let filled = keeper.fill(&later, &settings, &unjudged(&later, &settings));
        assert!(filled.is_empty(), "{change}: {filled:?}");
        assert_eq!(keeper.unjudged.len(), 1, "{change}");
        // Read through 10:26 at last: the first worker's rest broke the first.
        let read = scene.look_with(ms(28, 0), conversation, at_rest_since_five());
        let filled = keeper.fill(&read, &settings, &unjudged(&read, &settings));
        assert_eq!(filled.len(), 1, "{change}: {filled:?}");
        assert!(filled[0].0.broke, "{change}: {filled:?}");
        assert_eq!(
            filled[0].1.as_ref().map(|found| found.key.clone()),
            Some(format!("1:T-4:{}", ms(5, 0))),
            "{change}"
        );
    }
}

#[test]
fn a_rest_is_judged_again_only_on_its_windows_own_record_whatever_the_copy_says_since() {
    // At 10:26 the copy said the window rested from 10:05, its own record unreadable past
    // 10:20: unresolved. The daemon then copies a prompt it took at 10:25; the record
    // stays unreadable. The copy alone never fills the gap.
    use cf_evals::long_run::invariants::{unjudged, Keeper, Settings};
    use cf_evals::long_run::mask::Names;
    let settings = Settings::default();
    let (scene, conversation) = rested("claude-code");
    let mut keeper = Keeper::default();
    let first = scene.look_read(
        ms(26, 0),
        conversation,
        Record::claude(""),
        (ms(20, 0), UNREADABLE),
    );
    keeper.keep_unjudged(first.at, unjudged(&first, &settings), &Names::default());
    assert_eq!(keeper.unjudged.len(), 1, "{:?}", keeper.unjudged);
    scene.item(
        conversation,
        4,
        "user",
        "[ConsensFlow m-2 · T-4 · note from @chief]\nAnd the tests",
        true,
        &t(25, 0),
    );
    let later = scene.look_read(
        ms(27, 0),
        conversation,
        Record::claude(""),
        (ms(20, 0), UNREADABLE),
    );
    let filled = keeper.fill(&later, &settings, &unjudged(&later, &settings));
    assert!(filled.is_empty(), "{filled:?}");
    assert_eq!(keeper.unjudged.len(), 1);
}

/// The worker (3) on Claude Code with T-4 working since 10:00: conversation 7 (the
/// first) ended at 10:02 after an answer, its copy holding it; 8 (the second) opened
/// then, its copy holding `copied` items, each an assistant's at its minute, ending a
/// turn. The two conversations' rows.
fn moved_on(copied: &[u32]) -> (Scene, i64, i64) {
    moved_on_at(2, copied)
}

/// [`moved_on`], 8 opened at `opened` minutes past 10:00.
fn moved_on_at(opened: u32, copied: &[u32]) -> (Scene, i64, i64) {
    let scene = Scene::new(&[("long-worker", "claude-code")]);
    let old = scene.conversation(3, "claude-code", "s-old");
    scene.task(4, 3, "working", &t(0, 0));
    scene.item(
        old,
        0,
        "user",
        "[ConsensFlow m-1 · T-4 · task from @chief]\nBuild it",
        true,
        &t(0, 1),
    );
    scene.item(old, 1, "assistant", "Building now.", true, &t(1, 0));
    scene.end_conversation(old, &t(2, 0));
    let new = scene.conversation(3, "claude-code", "s-new");
    scene
        .db
        .execute(
            "UPDATE conversation SET started_at = ?1 WHERE id = ?2",
            rusqlite::params![t(opened, 0), new],
        )
        .unwrap();
    for (seq, minute) in (0..).zip(copied) {
        scene.item(
            new,
            seq,
            "assistant",
            "Built: 36 pages.",
            true,
            &t(*minute, 0),
        );
    }
    (scene, old, new)
}

#[test]
fn a_rest_is_judged_again_on_the_conversation_it_was_taken_for_whichever_is_open_later() {
    // Astra's: at 10:26 conversation 8 was open, its copy at rest since 10:05, its own
    // record unreadable: unresolved. Then 7 is opened again (its end gone) and 8 ends; 7's
    // record has a turn at work then. Only 8's says what 10:26 was.
    use cf_evals::long_run::invariants::{unjudged, Keeper, Settings};
    use cf_evals::long_run::mask::Names;
    let settings = Settings::default();
    let (scene, old, new) = moved_on(&[5]);
    let mut keeper = Keeper::default();
    let first = scene.look_read(ms(26, 0), new, Record::claude(""), (ms(20, 0), UNREADABLE));
    keeper.keep_unjudged(first.at, unjudged(&first, &settings), &Names::default());
    assert_eq!(
        keeper
            .unjudged
            .iter()
            .map(|open| open.key.clone())
            .collect::<Vec<_>>(),
        [format!("1:T-4:{}", ms(5, 0))]
    );
    scene.end_conversation(new, &t(26, 30));
    scene
        .db
        .execute(
            "UPDATE conversation SET ended_at = NULL WHERE id = ?1",
            [old],
        )
        .unwrap();
    let mut later = scene.look_with(ms(27, 0), new, at_rest_since_five());
    later.records.insert(
        old,
        claude_record(&[
            json!({ "timestamp": t(25, 0), "type": "user", "message": { "role": "user", "content": "[ConsensFlow m-2 · T-4 · note from @chief]\nAnd the tests" } }),
        ]),
    );
    let filled = keeper.fill(&later, &settings, &unjudged(&later, &settings));
    assert_eq!(filled.len(), 1, "{filled:?}");
    assert!(filled[0].0.broke, "{filled:?}");
}

#[test]
fn a_claude_windows_rest_neither_its_record_nor_the_copy_of_its_open_conversation_tells_is_unresolved(
) {
    // Both reviewers': conversation 7 ended at 10:02; 8 is open, its window at rest since
    // 10:05 with no job, its copy empty (the daemon still copies 7) and its own record
    // unreadable. At 10:26 nothing tells what the window did: unresolved, never silent.
    use cf_evals::long_run::invariants::{unjudged, Keeper, Settings};
    use cf_evals::long_run::mask::Names;
    let settings = Settings::default();
    let (scene, _, new) = moved_on(&[]);
    let first = scene.look_read(ms(26, 0), new, Record::claude(""), (ms(20, 0), UNREADABLE));
    assert!(found(&first, 1).is_empty(), "{:?}", found(&first, 1));
    assert_eq!(unjudged_first(&first), [format!("1:T-4:{}", ms(0, 0))]);
    // Never read, or read and saying nothing yet: the same.
    assert_eq!(
        unjudged_first(&scene.look(ms(26, 0))),
        [format!("1:T-4:{}", ms(0, 0))]
    );
    let silent = scene.look_with(ms(26, 0), new, Record::claude(""));
    assert_eq!(unjudged_first(&silent), [format!("1:T-4:{}", ms(0, 0))]);
    // A later reading through 10:26 that still says nothing leaves it open; one that
    // says judges it: at rest since 10:05, the first broke.
    let mut keeper = Keeper::default();
    keeper.keep_unjudged(first.at, unjudged(&first, &settings), &Names::default());
    let filled = keeper.fill(&silent, &settings, &unjudged(&silent, &settings));
    assert!(filled.is_empty(), "{filled:?}");
    let read = scene.look_with(ms(27, 0), new, at_rest_since_five());
    let filled = keeper.fill(&read, &settings, &unjudged(&read, &settings));
    assert_eq!(filled.len(), 1, "{filled:?}");
    assert!(filled[0].0.broke, "{filled:?}");
    // Given again at 10:10, in 8 open since 10:02: before the idle time had passed since,
    // no rest could be too long.
    scene
        .db
        .execute(
            "UPDATE task SET updated_at = ?1 WHERE number = 4",
            [t(10, 0)],
        )
        .unwrap();
    let unread = |at| scene.look_read(at, new, Record::claude(""), (ms(20, 0), UNREADABLE));
    assert!(unjudged_first(&unread(ms(26, 0))).is_empty());
    assert_eq!(
        unjudged_first(&unread(ms(30, 0))),
        [format!("1:T-4:{}", ms(10, 0))]
    );
}

#[test]
fn a_window_rests_in_its_open_conversation_no_longer_than_that_has_been_open() {
    // A window relaunched mid-task: 8 opened at 10:10, the task given at 10:00, its copy
    // empty and its record unreadable at 10:26. Its window cannot have rested in 8 longer
    // than 16 minutes: held. Once 8 has been open the idle time, at 10:30, it is untold
    // again, as one opened at 10:02 was at 10:26.
    let (scene, _, new) = moved_on_at(10, &[]);
    let unread = |at| scene.look_read(at, new, Record::claude(""), (ms(20, 0), UNREADABLE));
    assert!(unjudged_first(&unread(ms(26, 0))).is_empty());
    assert!(unjudged_first(&scene.look(ms(26, 0))).is_empty());
    assert_eq!(
        unjudged_first(&unread(ms(30, 0))),
        [format!("1:T-4:{}", ms(0, 0))]
    );
}

/// The notification of the end of the job b33m, as Claude Code writes it.
const B33M_ENDED: &str =
    "<task-notification>\n<task-id>b33m</task-id>\n<status>completed</status>\n</task-notification>";

/// What ConsensFlow writes to whoever gave T-14 when someone finishes it while its
/// jobs run, before the result (Tyr's `left_note`, `crates/cf-engine/src/background.rs`
/// on `wt/turn-end`).
const T14_FINISHED: &str = "T-14 is finished at @chief's word: what it said last is its result.";

#[test]
fn a_job_that_ended_before_the_result_a_finished_task_or_another_harness_holds_the_second() {
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", B33M_ENDED, true, &t(1, 50));
    assert!(found(&scene.look(ms(10, 0)), 2).is_empty());
    // Devin's command moved to the background wakes no turn: its answer is the result.
    let (scene, _, _) = job_then_result("devin");
    assert!(found(&scene.look(ms(10, 0)), 2).is_empty());
    // A result too fresh to judge: the record is copied a moment after it is written.
    let (scene, _, _) = job_then_result("claude-code");
    assert!(found(&scene.look(ms(2, 30)), 2).is_empty());
}

#[test]
fn a_task_finished_at_the_chiefs_word_before_its_result_holds_the_second() {
    // A ledger with exactly ConsensFlow's note, to the chief, before the result.
    let (scene, _, task) = job_then_result("claude-code");
    scene.message(
        130,
        (CHIEF, None),
        "note",
        Some(task),
        T14_FINISHED,
        &t(1, 59),
    );
    assert!(
        found(&scene.look(ms(10, 0)), 2).is_empty(),
        "the chief finished it"
    );
    // The note of an attempt before is none of this result's: T-14 was finished once,
    // reopened, and its next result taken while the job ran.
    let (scene, _, task) = job_then_result("claude-code");
    scene.message(
        100,
        (CHIEF, None),
        "note",
        Some(task),
        T14_FINISHED,
        &t(0, 10),
    );
    scene.message(
        101,
        (CHIEF, Some(3)),
        "result",
        Some(task),
        "First go.",
        &t(0, 20),
    );
    assert_eq!(found(&scene.look(ms(10, 0)), 2).len(), 1);
    // Nor is a note after the result, or one of another task.
    let (scene, _, task) = job_then_result("claude-code");
    scene.message(
        140,
        (CHIEF, None),
        "note",
        Some(task),
        T14_FINISHED,
        &t(2, 1),
    );
    scene.message(
        129,
        (CHIEF, None),
        "note",
        Some(task),
        "T-15 is finished at @chief's word: what it said last is its result.",
        &t(1, 58),
    );
    assert_eq!(found(&scene.look(ms(10, 0)), 2).len(), 1);
}

#[test]
fn a_jobs_end_written_a_moment_after_the_result_is_seen_within_the_settle_time() {
    // The job ended as the result was taken; its notification came 5 s after.
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", B33M_ENDED, true, &t(2, 5));
    assert!(
        found(&scene.look(ms(10, 0)), 2).is_empty(),
        "the job had ended"
    );
    // A notification past the settle time (60 s) is the end of a job that ran on.
    let (scene, conversation, _) = job_then_result("claude-code");
    scene.item(conversation, 3, "user", B33M_ENDED, true, &t(3, 30));
    assert_eq!(found(&scene.look(ms(10, 0)), 2).len(), 1);
    // The record read whole says the same, a moment after the result.
    let (scene, conversation, _) = job_then_result("claude-code");
    let record = Record::claude(
        &[
            json!({ "timestamp": t(1, 0), "type": "user", "toolUseResult": { "backgroundTaskId": "b33m" },
                    "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "x",
                    "content": "Command running in background with ID: b33m" }] } }),
            json!({ "timestamp": t(2, 4), "type": "queue-operation", "operation": "enqueue", "content": B33M_ENDED }),
        ]
        .map(|line| line.to_string())
        .join("\n"),
    );
    assert!(found(&scene.look_with(ms(10, 0), conversation, record), 2).is_empty());
}
