//! The background agents and workflows a turn left running. Claude Code says how
//! many on the `turn_duration` that ends the turn (`pendingBackgroundAgentCount`,
//! `pendingWorkflowCount`, each left out when none), and its status says `busy`
//! for as long as any runs, though its prompt takes input: the end of one wakes
//! a turn of its own, which writes its own `turn_duration`.
//!
//! **These depart from Node's reader on purpose** (the owner's decision of
//! 2026-10-11). Node read a `turn_duration` that said any pending as no end of
//! the turn: the turn stayed in flight until the last agent finished. Here that
//! record ends the turn, as a `stop_hook_summary` does (a ConsensFlow window
//! writes one before every `turn_duration`), and the reading says the agents are
//! pending (`agents_pending`), for the engine to say in its note to the task's
//! requester (it takes the result all the same). What Node refused for being no
//! count at all (a string, a fraction, a number below 0) it still refuses.

use super::*;

/// What an agent's end enqueues, and the turn it wakes begins with.
const NOTIFICATION: &str =
    "<task-notification>\n<task-id>t1</task-id>\n<status>completed</status>\n</task-notification>";

/// The turn's end of the duration `uuid` of `parent`, which says `count` agents run.
fn pending(uuid: &str, parent: &str, count: u64) -> Value {
    having(
        duration(uuid, parent),
        json!({ "pendingBackgroundAgentCount": count }),
    )
}

/// The assistant's answer of message `id` in a turn after the first.
fn answered(uuid: &str, parent: &str, id: &str) -> Value {
    assistant(
        uuid,
        parent,
        json!(id),
        json!([text(json!("Noted"))]),
        json!("end_turn"),
    )
}

/// The user's record of the turn an agent's end woke: Claude Code's own message,
/// not typed.
fn woken(uuid: &str, parent: &str) -> Value {
    having(
        user(uuid, json!(parent), json!(NOTIFICATION)),
        json!({ "promptSource": "system" }),
    )
}

/// What a look at the stage reads.
fn read(stage: &mut Stage) -> Record {
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

/// What one look at `records` reads.
fn once_read(records: &[Value]) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    read(&mut stage)
}

/// What a look says of the turn and of the agents: `(settlement, agents_pending)`.
fn says(stage: &mut Stage) -> (Settlement, bool) {
    let record = read(stage);
    (record.settlement, record.pending.agents > 0)
}

#[test]
fn a_turn_duration_that_says_agents_or_workflows_are_pending_ends_the_turn_and_the_reading_says_so()
{
    // Node's answer for each of the first six was that it did not end the turn.
    let cases = [
        (
            "one agent",
            json!({ "pendingBackgroundAgentCount": 1 }),
            true,
        ),
        (
            "four agents",
            json!({ "pendingBackgroundAgentCount": 4 }),
            true,
        ),
        (
            "a count written 2.0",
            json!({ "pendingBackgroundAgentCount": 2.0 }),
            true,
        ),
        ("one workflow", json!({ "pendingWorkflowCount": 1 }), true),
        (
            "none of one and some of the other",
            json!({ "pendingBackgroundAgentCount": 0, "pendingWorkflowCount": 2 }),
            true,
        ),
        (
            "agents and workflows",
            json!({ "pendingBackgroundAgentCount": 1, "pendingWorkflowCount": 1 }),
            true,
        ),
        // Node's answer for these was the same: the turn ended, and nothing runs.
        (
            "counts of none",
            json!({ "pendingBackgroundAgentCount": 0, "pendingWorkflowCount": 0 }),
            false,
        ),
        ("no counts", json!({}), false),
    ];
    for (name, counts, agents) in cases {
        let record = once_read(&[
            hello(),
            answer("a1", "u1"),
            having(duration("d1", "a1"), counts),
        ]);
        assert_eq!(record.settlement, Settlement::Settled, "{name}");
        assert!(!record.in_flight, "{name}");
        assert_eq!(record.pending.agents > 0, agents, "{name}");
        assert!(
            record.items.last().is_some_and(|item| item.complete),
            "{name}: its answer is whole"
        );
    }
}

#[test]
fn a_count_that_is_no_count_does_not_end_the_turn_as_in_node_and_nothing_is_said_of_agents() {
    for value in [
        "-1", "1.5", "1e-7", "1e300", r#""2""#, "null", "true", "[]", "{}",
    ] {
        for field in ["pendingBackgroundAgentCount", "pendingWorkflowCount"] {
            let said = json!({ field: serde_json::from_str::<Value>(value).unwrap() });
            let record = once_read(&[
                hello(),
                answer("a1", "u1"),
                having(duration("d1", "a1"), said),
            ]);
            assert_eq!(record.settlement, Settlement::InFlight, "{field} {value}");
            assert!(record.pending.agents == 0, "{field} {value}");
        }
    }
}

#[test]
fn the_summary_a_stop_hook_writes_settles_the_turn_and_the_duration_after_it_says_the_agents() {
    // The order of every turn in a ConsensFlow window (its Stop hook makes
    // Claude Code write the summary), as Claude Code 2.1.296 wrote it: the
    // summary, and a `turn_duration` that says three agents run.
    let mut stage = Stage::new();
    stage.write(&[hello(), answer("a1", "u1"), summary("s1", "a1")]);
    assert_eq!(
        says(&mut stage),
        (Settlement::Settled, false),
        "the summary says nothing of agents"
    );
    stage.append(&[pending("d1", "s1", 3)]);
    assert_eq!(says(&mut stage), (Settlement::Settled, true));

    // The other order is read the same: the duration said it, and the summary
    // after it takes nothing back.
    let mut stage = Stage::new();
    stage.write(&[hello(), answer("a1", "u1"), pending("d1", "a1", 3)]);
    assert_eq!(says(&mut stage), (Settlement::Settled, true));
    stage.append(&[summary("s1", "d1")]);
    assert_eq!(says(&mut stage), (Settlement::Settled, true));

    // A duration after it that says none says what is left of the same turn.
    stage.append(&[duration("d2", "s1")]);
    assert_eq!(says(&mut stage), (Settlement::Settled, false));
}

#[test]
fn agents_that_finish_one_at_a_time_leave_a_turn_that_says_so_until_the_last_turn_says_none() {
    // A chief started four research agents and ended its turn. Each one's end
    // woke it: Claude Code queues a notification and takes it at once, and the
    // turn it begins writes its own `turn_duration`, which says how many are left.
    let mut stage = Stage::new();
    stage.write(&[
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        pending("d1", "s1", 4),
    ]);
    assert_eq!(says(&mut stage), (Settlement::Settled, true));

    let mut last = "d1".to_owned();
    for (turn, left) in [(2_u64, 3_u64), (3, 2), (4, 1), (5, 0)] {
        stage.append(&[queue("enqueue", json!(NOTIFICATION))]);
        assert_eq!(
            says(&mut stage),
            (Settlement::InFlight, false),
            "the notification is queued: a turn is about to begin"
        );
        let (took, reply) = (format!("w{turn}"), format!("r{turn}"));
        let (summed, ended) = (format!("s{turn}"), format!("d{turn}"));
        stage.append(&[queue("dequeue", Value::Null), woken(&took, &last)]);
        assert_eq!(
            says(&mut stage),
            (Settlement::InFlight, false),
            "the woken turn is at work: nothing is said of the agents of the turn before"
        );
        let end = if left == 0 {
            duration(&ended, &summed)
        } else {
            pending(&ended, &summed, left)
        };
        stage.append(&[
            answered(&reply, &took, &format!("m{turn}")),
            summary(&summed, &reply),
            end,
        ]);
        assert_eq!(
            says(&mut stage),
            (Settlement::Settled, left > 0),
            "{left} agents run after turn {turn}: a later turn_duration with none clears it"
        );
        last = ended;
    }
}

#[test]
fn a_turn_that_begins_says_nothing_of_the_agents_of_the_one_before_even_when_it_writes_no_duration()
{
    let mut stage = Stage::new();
    stage.write(&[
        hello(),
        answer("a1", "u1"),
        summary("s1", "a1"),
        pending("d1", "s1", 2),
    ]);
    assert_eq!(says(&mut stage), (Settlement::Settled, true));

    // The human types into the window between turns, which Claude Code takes.
    stage.append(&[user("u2", json!("d1"), json!("Go on"))]);
    assert_eq!(says(&mut stage), (Settlement::InFlight, false));

    // Its turn ends at a stop hook's summary alone, as some of Claude Code's
    // turns do (no `turn_duration`): the two agents of the turn before are not
    // carried into it.
    stage.append(&[answered("a2", "u2", "m2"), summary("s2", "a2")]);
    assert_eq!(says(&mut stage), (Settlement::Settled, false));
}

#[test]
fn agents_are_said_of_a_turn_that_is_over_and_of_none_that_is_open() {
    // A tool call the turn left open, or a message queued behind it, keeps the
    // turn in flight whatever its duration says.
    let calling = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1")), text(json!("Working"))]),
        json!("end_turn"),
    );
    let open = once_read(&[hello(), calling, pending("d1", "a1", 2)]);
    assert_eq!(open.settlement, Settlement::InFlight);
    assert!(open.pending.agents == 0, "a call is open");

    let queued = once_read(&[
        hello(),
        answer("a1", "u1"),
        pending("d1", "a1", 2),
        queue("enqueue", json!("More")),
    ]);
    assert_eq!(queued.settlement, Settlement::InFlight);
    assert!(queued.pending.agents == 0, "a message is queued");
}
