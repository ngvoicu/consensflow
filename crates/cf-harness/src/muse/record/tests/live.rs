//! The reader on the log of a real model's session (T-705's contributor run,
//! `tests/fixtures/muse/live/contributor-full-run.jsonl`, whose README says what each
//! stretch holds): what holds a turn open, and the work it leaves running, read at the
//! records the log itself numbers.

use serde_json::Value;

use super::*;
use crate::shared::record::reading::Pending;

const LIVE: &str = include_str!("../../../../tests/fixtures/muse/live/contributor-full-run.jsonl");

/// The log's lines, each with the sequence its record says.
fn lines() -> Vec<(u64, &'static str)> {
    LIVE.lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            (record["sequence"].as_u64().unwrap_or(0), line)
        })
        .collect()
}

/// What a fresh reader reads of the log up to the record of `sequence`, that one too.
fn through(sequence: u64) -> Record {
    let text: String = lines()
        .into_iter()
        .filter(|(at, _)| *at <= sequence)
        .map(|(_, line)| format!("{line}\n"))
        .collect();
    known(&fresh_reading(&session_of(LIVE), &text)).clone()
}

#[test]
fn the_live_log_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    let session = session_of(LIVE);
    let stage = Stage::new(&session);
    let mut reading_on = stage.reader();
    let mut text = String::new();
    for (at, (sequence, line)) in lines().into_iter().enumerate() {
        text.push_str(line);
        text.push('\n');
        stage.append(line);
        let on = look(&mut reading_on);
        // Fresh at every twentieth record and at each that ends something.
        let ends = line.contains("\"kind\":\"terminal\"") || line.contains("session_end");
        if at % 20 == 0 || ends {
            assert_eq!(
                on,
                fresh_reading(&session, &text),
                "after sequence {sequence}"
            );
        }
    }
}

#[test]
fn no_turn_of_the_live_log_ends_with_work_pending_and_each_reads_settled() {
    // At each `terminal` the agent had waited on everything it started: nothing
    // runs, and the turn reads settled, never working for good.
    for terminal in [97, 252, 441, 602, 670, 723, 845] {
        let record = through(terminal);
        assert_eq!(record.settlement, Settlement::Settled, "{terminal}");
        assert_eq!(record.pending, Pending::default(), "{terminal}");
    }
}

#[test]
fn a_command_run_in_the_background_is_pending_until_its_end_is_taken_into_a_run() {
    // Started (533), and not yet said to run in the background.
    assert_eq!(through(536).pending.jobs, 0);
    // Its output says it runs in the background, and the call's result comes at once.
    for sequence in [537, 541, 552] {
        assert_eq!(through(sequence).pending.jobs, 1, "{sequence}");
    }
    // Its end is queued for the agent (553) and its task ends (555): one piece of work,
    // pending until its end is drained into the run (582).
    for sequence in [553, 555, 581] {
        assert_eq!(through(sequence).pending.jobs, 1, "{sequence}");
    }
    assert_eq!(through(582).pending.jobs, 0);
    let ended = through(602);
    assert_eq!(ended.settlement, Settlement::Settled);
    assert_eq!(ended.pending.jobs, 0);
}

#[test]
fn a_monitor_is_pending_until_its_stream_ends_and_its_events_are_taken_in() {
    assert_eq!(
        through(306).pending.jobs,
        0,
        "the monitor's task, before it says so"
    );
    for sequence in [307, 337, 350, 351, 352, 380] {
        assert_eq!(through(sequence).pending.jobs, 1, "{sequence}");
    }
    assert_eq!(through(382).pending.jobs, 0);
}

#[test]
fn a_child_agent_its_parent_waits_on_holds_the_turn_and_is_no_agent_pending_of_it() {
    // The agent waited on its child (`subagent_wait`): the turn is under way while the
    // child runs, and its agents are said of a turn that is over alone (`Pending`), so a
    // child that outlives its parent's turn is `background`'s.
    for sequence in [319, 329, 354, 355, 373] {
        let record = through(sequence);
        assert_eq!(record.settlement, Settlement::InFlight, "{sequence}");
        assert_eq!(record.pending.agents, 0, "{sequence}");
    }
    assert_eq!(through(441).pending, Pending::default());
}

#[test]
fn a_question_and_an_approval_are_asking_until_answered() {
    assert!(!through(642).asking);
    assert!(through(643).asking, "request_user_input");
    assert!(!through(644).asking);
    assert!(!through(801).asking);
    for sequence in [802, 803, 808] {
        let waiting = through(sequence);
        assert!(waiting.asking, "approval at {sequence}");
        assert_eq!(waiting.settlement, Settlement::InFlight, "{sequence}");
    }
    assert!(!through(809).asking, "the decision applied");
    assert_eq!(through(845).settlement, Settlement::Settled);
}

#[test]
fn a_compactions_summary_is_the_harnesss_own_item_marked_a_compaction() {
    let record = through(747);
    let summaries: Vec<&Item> = record.items.iter().filter(|item| item.compaction).collect();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].role, Role::Custom);
    assert!(!summaries[0].text.trim().is_empty());
}

#[test]
fn a_foreground_command_cancelled_with_its_turn_leaves_nothing_pending() {
    let record = through(723);
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(
        !record.failed && !record.took_back,
        "output came before the Escape"
    );
    assert_eq!(record.pending, Pending::default());
}

#[test]
fn a_tools_results_are_its_items_the_turn_open_until_each_is_answered() {
    let record = through(252);
    let tools = record
        .items
        .iter()
        .filter(|item| item.role == Role::Tool)
        .count();
    assert_eq!(tools, 2, "the bash that succeeded, the one that exited 7");
    assert_eq!(
        through(148).settlement,
        Settlement::InFlight,
        "a call unanswered"
    );
}

/// The log of the live case `live_muse_background` (T-789, `muse-spark-1.3` at
/// `minimal`, contributor mode off): a command that outlived its turn, and the run that
/// woke for its end.
const OUTLIVED: &str =
    include_str!("../../../../tests/fixtures/muse/live/background-outlives-turn.jsonl");

/// What a fresh reader reads of that log up to the record of `sequence`, that one too.
fn outlived_through(sequence: u64) -> Record {
    let text: String = OUTLIVED
        .lines()
        .filter(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            record["sequence"].as_u64().unwrap_or(0) <= sequence
        })
        .map(|line| format!("{line}\n"))
        .collect();
    known(&fresh_reading(&session_of(OUTLIVED), &text)).clone()
}

#[test]
fn a_command_that_outlived_its_turn_held_it_until_the_run_it_woke_took_its_end() {
    // The turn's answer (80) and its end (86): settled, the command running.
    let ended = outlived_through(86);
    assert_eq!(ended.settlement, Settlement::Settled);
    assert_eq!(ended.pending.jobs, 1);
    // Its end queued (90), its task over (92): its agent not told yet.
    for sequence in [90, 92, 94] {
        let record = outlived_through(sequence);
        assert_eq!(record.settlement, Settlement::Settled, "{sequence}");
        assert_eq!(record.pending.jobs, 1, "{sequence}");
    }
    // Muse starts the run it wakes (95) before it drains the end into it (107).
    let woken = outlived_through(95);
    assert_eq!(woken.settlement, Settlement::InFlight);
    assert_eq!(woken.pending.jobs, 1);
    assert_eq!(outlived_through(107).pending.jobs, 0);
    // The woken run answers and ends: nothing left.
    let done = outlived_through(126);
    assert_eq!(done.settlement, Settlement::Settled);
    assert_eq!(done.pending, Pending::default());
    assert_eq!(
        done.items
            .iter()
            .filter(|item| item.role == Role::Assistant)
            .count(),
        2,
        "STARTED, then the woken run's answer"
    );
}

#[test]
fn the_outlived_log_reads_the_same_read_on_a_record_at_a_time_as_read_fresh() {
    let session = session_of(OUTLIVED);
    let stage = Stage::new(&session);
    let mut reading_on = stage.reader();
    let mut text = String::new();
    for line in OUTLIVED.lines() {
        text.push_str(line);
        text.push('\n');
        stage.append(line);
        assert_eq!(look(&mut reading_on), fresh_reading(&session, &text));
    }
}
