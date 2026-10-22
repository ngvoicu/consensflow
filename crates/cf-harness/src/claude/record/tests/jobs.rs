//! The jobs a turn left running in the background. A Bash call that asks to run
//! in the background returns at once, and the record of its tool result says the
//! job's id (`toolUseResult.backgroundTaskId`); the turn can end with the job
//! running, and when the job ends Claude Code queues a `<task-notification>` that
//! names the id and begins a turn by itself, which wakes the agent. Claude Code's
//! `turn_duration` does not count these jobs, as it counts the background agents
//! and workflows (`agents`): the reader does, from the record that starts a job
//! to the notification that ends it, and the reading says how many are pending.
//!
//! The records are the shapes Claude Code 2.1.299 wrote for a turn that started a
//! mutation run in the background (the owner's report of T-14 on 3.0.4, 2026-10-15).

use super::*;
use crate::claude::record::jobs::Written;
use crate::shared::record::reading::{Pending, Role};

/// The id Claude Code gave the job.
const JOB: &str = "bayehs8y4";

/// What Claude Code queues when the job `id` ends with `status`, and the turn it
/// wakes begins with.
fn notice(id: &str, status: &str) -> String {
    format!(
        "<task-notification>\n<task-id>{id}</task-id>\n<status>{status}</status>\n<summary>Background command \"mutation run\" {status}</summary>\n</task-notification>"
    )
}

/// The assistant's message `m1`: a word, and the Bash call `t1` that runs in the
/// background, which stops for its tool.
fn calling() -> Value {
    assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([
            text(json!("I'll run it in the background.")),
            tool_use(json!("t1"))
        ]),
        json!("tool_use"),
    )
}

/// The record of the result of the call `call`, which says the job `id` runs in the
/// background, as Claude Code writes it.
fn started(uuid: &str, parent: &str, call: &str, id: &str) -> Value {
    let said = format!(
        "Command running in background with ID: {id}. Output is being written to: /tmp/claude/tasks/{id}.output. You will be notified when it completes."
    );
    having(
        user(
            uuid,
            json!(parent),
            json!([tool_result(json!(call), json!(said))]),
        ),
        json!({ "toolUseResult": { "stdout": "", "stderr": "", "interrupted": false, "isImage": false, "backgroundTaskId": id } }),
    )
}

/// The turn's words after the call, which ended it.
fn going_on() -> Value {
    assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([text(json!(
            "The mutation run is still going in the background. I'll write up the result once it finishes."
        ))]),
        json!("end_turn"),
    )
}

/// A turn that started the job `id` and ended with it running: Claude Code's summary of
/// the stop hooks, and a `turn_duration` that counts no agents.
fn turn_leaving(id: &str) -> Vec<Value> {
    vec![
        hello(),
        calling(),
        started("u2", "a1", "t1", id),
        going_on(),
        summary("s1", "a2"),
        duration("d1", "s1"),
    ]
}

/// The user's record of the turn the notification woke: Claude Code's own words, from
/// the system.
fn woken(uuid: &str, parent: &str, said: &str) -> Value {
    having(
        user(uuid, json!(parent), json!(said)),
        json!({ "promptSource": "system", "turnOrigin": "task_notification" }),
    )
}

/// The report the woken turn ends with: message `m3`.
fn reporting(uuid: &str, parent: &str) -> Value {
    assistant(
        uuid,
        parent,
        json!("m3"),
        json!([text(json!("All 33 mutants are killed."))]),
        json!("end_turn"),
    )
}

/// What a look says of the turn and of what it left: `(settlement, pending)`, the
/// agents and the jobs together.
fn says(stage: &mut Stage) -> (Settlement, usize) {
    let record = match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
    };
    (record.settlement, record.pending.total())
}

/// What one look at `records` says of the turn and of what it left.
fn once_says(records: &[Value]) -> (Settlement, usize) {
    let mut stage = Stage::new();
    stage.write(records);
    says(&mut stage)
}

#[test]
fn a_turn_that_ends_with_a_job_running_in_the_background_is_settled_and_the_reading_says_it_is_pending(
) {
    let mut stage = Stage::new();
    stage.write(&turn_leaving(JOB));
    assert_eq!(says(&mut stage), (Settlement::Settled, 1));

    // Its words are its answer all the same, and the call and its result are in the items.
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };
    assert_eq!(
        record.pending,
        Pending {
            agents: 0,
            jobs: 1,
            retired: 0
        }
    );
    let roles: Vec<(Role, bool)> = record
        .items
        .iter()
        .map(|item| (item.role, item.complete))
        .collect();
    assert_eq!(
        roles,
        [
            (Role::User, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, true)
        ]
    );
    assert!(record.items[3].text.contains("still going"));
}

#[test]
fn the_notification_of_the_jobs_end_wakes_a_turn_whose_report_leaves_none_pending() {
    // Claude Code queues the notification and begins the turn by itself: the record
    // says no dequeue, the turn's first record is the system's own, with the same words.
    let said = notice(JOB, "completed");
    let mut stage = Stage::new();
    stage.write(&turn_leaving(JOB));
    assert_eq!(says(&mut stage), (Settlement::Settled, 1));

    // Queued, its notification ends the job only once a turn takes it: in flight, the
    // job is still counted.
    stage.append(&[queue("enqueue", json!(said))]);
    assert_eq!(
        says(&mut stage),
        (Settlement::InFlight, 1),
        "a turn is about to begin: nothing is said of what the last left"
    );

    stage.append(&[woken("w1", "d1", &said)]);
    assert_eq!(
        says(&mut stage),
        (Settlement::InFlight, 0),
        "the message is taken, and the woken turn is at work"
    );

    stage.append(&[
        reporting("a3", "w1"),
        summary("s2", "a3"),
        duration("d2", "s2"),
    ]);
    assert_eq!(says(&mut stage), (Settlement::Settled, 0));
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };
    let last = record.items.last().unwrap();
    assert_eq!(&*last.text, "All 33 mutants are killed.");
    assert!(last.complete);
}

#[test]
fn a_notification_ends_its_job_whichever_record_says_it_and_whichever_end_it_says() {
    for status in ["completed", "failed", "killed"] {
        let said = notice(JOB, status);
        // Queued and taken, as an agent's end is.
        let mut queued = turn_leaving(JOB);
        queued.extend([
            queue("enqueue", json!(said)),
            queue("dequeue", Value::Null),
            woken("w1", "d1", &said),
            reporting("a3", "w1"),
            summary("s2", "a3"),
        ]);
        assert_eq!(
            once_says(&queued),
            (Settlement::Settled, 0),
            "{status}, queued and taken"
        );

        // Only the user's record of it: a Claude Code that queues nothing.
        let mut unqueued = turn_leaving(JOB);
        unqueued.extend([
            woken("w1", "d1", &said),
            reporting("a3", "w1"),
            summary("s2", "a3"),
        ]);
        assert_eq!(
            once_says(&unqueued),
            (Settlement::Settled, 0),
            "{status}, the user's record alone"
        );

        // Only the queue's: the turn it wakes has not begun, and the job is counted until
        // a turn takes its notification.
        let mut waiting = turn_leaving(JOB);
        waiting.push(queue("enqueue", json!(said)));
        assert_eq!(
            once_says(&waiting),
            (Settlement::InFlight, 1),
            "{status}, only queued"
        );
    }
}

#[test]
fn a_notification_queued_and_taken_back_ends_its_job_only_once_it_is_brought_into_a_turn() {
    // Taken back from the queue (Claude Code's `remove`, or a `popAll`) and never
    // delivered, it ended nothing: the queue's word is no notification's.
    for taking in ["remove", "popAll"] {
        let said = notice(JOB, "completed");
        let mut records = turn_leaving(JOB);
        records.extend([
            queue("enqueue", json!(said)),
            queue(taking, json!(said)),
            user("u3", json!("d1"), json!("Never mind")),
            reporting("a3", "u3"),
            summary("s2", "a3"),
        ]);
        assert_eq!(once_says(&records), (Settlement::Settled, 1), "{taking}");
    }
}

/// The records of real transcripts of Claude Code 2.1.277 and 2.1.292 about its jobs
/// (`tests/fixtures/claude/job-notifications.jsonl`), in their order: the start of
/// `bmh4uwmeq`, its notification queued, taken back and brought into a turn under way;
/// the start of `bey3qyuly`, its notification queued at rest, taken, and the user's
/// record of the turn it begins; and the notification, queued, taken and recorded, of
/// two jobs a resumed session's last process left.
fn real() -> Vec<Value> {
    include_str!("../../../../tests/fixtures/claude/job-notifications.jsonl")
        .replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// `record`, the child of `parent`.
fn under(record: &Value, parent: &str) -> Value {
    having(record.clone(), json!({ "parentUuid": parent }))
}

/// The uuid of `record`.
fn uuid_of(record: &Value) -> String {
    record["uuid"].as_str().unwrap().to_owned()
}

#[test]
fn a_real_notification_brought_into_a_turn_under_way_ends_its_job_and_its_queued_copy_ends_none() {
    // 2.1.277: the job's end was queued while a turn was at work, taken back from the
    // queue (`absorbed_mid_turn`) and brought into that turn as an attachment.
    let real = real();
    let mut records = vec![
        hello(),
        calling(),
        under(&real[0], "a1"),
        going_on(),
        summary("s1", "a2"),
    ];
    assert_eq!(
        once_says(&records),
        (Settlement::Settled, 1),
        "bmh4uwmeq runs"
    );

    // The next turn is at work when the notification is queued and taken back.
    records.extend([
        user("u3", json!("s1"), json!("Go on")),
        assistant(
            "a3",
            "u3",
            json!("m9"),
            json!([tool_use(json!("t3"))]),
            json!("tool_use"),
        ),
        real[1].clone(),
        real[2].clone(),
    ]);
    let closing = || {
        user(
            "r3",
            json!("a3"),
            json!([tool_result(json!("t3"), json!("ok"))]),
        )
    };
    let mut queued_only = records.clone();
    queued_only.extend([closing(), reporting("a4", "r3"), summary("s3", "a4")]);
    assert_eq!(
        once_says(&queued_only),
        (Settlement::Settled, 1),
        "queued and taken back, never delivered: the job is as it was"
    );

    records.extend([
        under(&real[3], "a3"),
        closing(),
        reporting("a4", "r3"),
        summary("s3", "a4"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
}

#[test]
fn a_real_notification_queued_at_rest_ends_its_job_when_its_turn_takes_it() {
    // 2.1.277: queued at rest, taken (`dequeue`), and the user's record of the turn it
    // begins, from the system for a notification.
    let real = real();
    let mut records = vec![
        hello(),
        calling(),
        under(&real[4], "a1"),
        going_on(),
        summary("s1", "a2"),
    ];
    assert_eq!(once_says(&records), (Settlement::Settled, 1));

    // Queued, not yet delivered: a turn is to begin, no result is taken, and the job is
    // counted still.
    records.push(real[5].clone());
    assert_eq!(once_says(&records), (Settlement::InFlight, 1));

    let woken = under(&real[7], "s1");
    records.extend([
        real[6].clone(),
        woken.clone(),
        reporting("a3", &uuid_of(&woken)),
        summary("s2", "a3"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
}

#[test]
fn a_real_notification_that_names_two_jobs_ends_both() {
    // 2.1.292, after a resume: one notification, `origin.kind` and no `turnOrigin`, for
    // the two jobs the session's last process left with no record of their end.
    let real = real();
    let mut records = vec![
        hello(),
        calling(),
        started("u2", "a1", "t1", "btz49cpd7"),
        started("u3", "u2", "t1", "bq3e7alyc"),
        going_on(),
        summary("s1", "a2"),
    ];
    assert_eq!(once_says(&records), (Settlement::Settled, 2));
    // Written to the transcript alone (`queueTranscriptOnly`): Claude Code answers
    // nothing to it and waits at its prompt, which the human's next message reached in
    // the real log. It begins no turn.
    records.extend([real[8].clone(), real[9].clone(), under(&real[10], "s1")]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
    // And it is Claude Code's own, no message of the user's: the words the turn ended with
    // are still the last the agent wrote after the last message it was given (both reviews
    // of round 4: read as the user's, it left the result with no answer).
    let mut stage = Stage::new();
    stage.write(&records);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };
    let roles: Vec<(Role, bool)> = record
        .items
        .iter()
        .rev()
        .take(2)
        .map(|item| (item.role, item.complete))
        .collect();
    assert_eq!(roles, [(Role::Custom, true), (Role::Assistant, true)]);
    assert!(record.items.last().unwrap().text.contains("didn't finish"));
    records.push(user("u4", json!(uuid_of(&real[10])), json!("progress?")));
    assert_eq!(once_says(&records), (Settlement::InFlight, 0));
}

#[test]
fn a_job_is_pending_until_its_own_notification_and_jobs_count_one_each() {
    let mut records = vec![
        hello(),
        calling(),
        started("u2", "a1", "t1", "first"),
        started("u3", "u2", "t1", "second"),
        going_on(),
        summary("s1", "a2"),
    ];
    assert_eq!(once_says(&records), (Settlement::Settled, 2));

    // The end of another job, or of one never started, ends neither.
    let mut stage = Stage::new();
    stage.write(&records);
    stage.append(&[woken("w1", "s1", &notice("third", "completed"))]);
    let others = [
        reporting("a3", "w1"),
        summary("s2", "a3"),
        duration("d2", "s2"),
    ];
    stage.append(&others);
    assert_eq!(says(&mut stage), (Settlement::Settled, 2));

    // The end of one ends one.
    records.extend([
        woken("w1", "s1", &notice("first", "completed")),
        reporting("a3", "w1"),
        summary("s2", "a3"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 1));
}

#[test]
fn jobs_are_not_counted_by_the_turn_duration_and_say_themselves_where_it_says_none_or_there_is_none(
) {
    // Claude Code's summary of the stop hooks alone ends the turn, as some of its turns end.
    let mut records = turn_leaving(JOB);
    records.truncate(5);
    assert_eq!(once_says(&records), (Settlement::Settled, 1));

    // A duration that counts agents counts them with the jobs.
    let mut records = turn_leaving(JOB);
    records.pop();
    records.push(having(
        duration("d1", "s1"),
        json!({ "pendingBackgroundAgentCount": 2, "pendingWorkflowCount": 1 }),
    ));
    assert_eq!(once_says(&records), (Settlement::Settled, 4));
    let mut stage = Stage::new();
    stage.write(&records);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };
    assert_eq!(
        record.pending,
        Pending {
            agents: 3,
            jobs: 1,
            retired: 0
        }
    );

    // A duration that counts none says nothing of the job, which runs.
    let mut records = turn_leaving(JOB);
    records.pop();
    records.push(having(
        duration("d1", "s1"),
        json!({ "pendingBackgroundAgentCount": 0 }),
    ));
    assert_eq!(once_says(&records), (Settlement::Settled, 1));
}

#[test]
fn a_job_is_said_of_a_turn_that_is_over_and_of_one_still_under_way() {
    // The turn has not ended: a call is open, or the user typed into the window. The job is
    // counted all the same, for Finish to say it runs (Astraeus on 0eec91d2: a reading in
    // flight said none).
    let open = assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([tool_use(json!("t2"))]),
        json!("tool_use"),
    );
    let mut records = vec![hello(), calling(), started("u2", "a1", "t1", JOB), open];
    assert_eq!(once_says(&records), (Settlement::InFlight, 1));

    records = turn_leaving(JOB);
    records.push(user("u3", json!("d1"), json!("Go on")));
    assert_eq!(once_says(&records), (Settlement::InFlight, 1));
}

#[test]
fn a_job_of_a_subagent_or_of_another_session_is_not_the_conversations_and_a_result_that_names_none_starts_none(
) {
    let started_in = |change: fn(Value) -> Value| {
        let mut records = turn_leaving(JOB);
        records[2] = change(records[2].clone());
        once_says(&records)
    };
    assert_eq!(
        started_in(|record| having(record, json!({ "isSidechain": true }))),
        (Settlement::Settled, 0),
        "a sidechain's"
    );
    assert_eq!(
        started_in(|record| having(record, json!({ "sessionId": "other" }))),
        (Settlement::Settled, 0),
        "another session's"
    );
    // What a result says that is no job's id: text where an object is, no id, an id that
    // is no text or is empty.
    for said in [
        json!("Error: the command failed"),
        json!({ "stdout": "done" }),
        json!({ "backgroundTaskId": null }),
        json!({ "backgroundTaskId": 7 }),
        json!({ "backgroundTaskId": "" }),
        json!({ "backgroundTaskId": ["a"] }),
    ] {
        let mut records = turn_leaving(JOB);
        records[2] = having(records[2].clone(), json!({ "toolUseResult": said }));
        assert_eq!(
            once_says(&records),
            (Settlement::Settled, 0),
            "{}",
            records[2]["toolUseResult"]
        );
    }
    let mut records = turn_leaving(JOB);
    records[2] = lacking(records[2].clone(), "toolUseResult");
    assert_eq!(once_says(&records), (Settlement::Settled, 0), "no result");
}

#[test]
fn a_message_that_quotes_a_whole_notification_ends_no_job_and_only_claude_codes_own_does() {
    // Astra's review of round 1: the job `b1` runs, an ordinary message quotes its
    // notification whole, and the worker answers it while the job still runs.
    let quoted = notice(JOB, "completed");
    for (whence, said) in [
        ("typed", quoted.clone()),
        ("typed", format!("Look at this: {quoted}")),
        ("queued", quoted.clone()),
        ("sdk", quoted.clone()),
    ] {
        let mut records = turn_leaving(JOB);
        records.extend([
            having(
                user("u3", json!("d1"), json!(said)),
                json!({ "promptSource": whence, "turnOrigin": "human", "origin": { "kind": "human" } }),
            ),
            reporting("a3", "u3"),
            summary("s2", "a3"),
        ]);
        assert_eq!(
            once_says(&records),
            (Settlement::Settled, 1),
            "a message {whence}: {said}"
        );
    }
    // Typed while the turn was open, it is queued first: no notification for all that,
    // however it begins (both reviews of round 3: one that begins as a notification did).
    for said in [
        quoted.clone(),
        format!("Look at this: {quoted}"),
        format!(" {quoted}"),
    ] {
        let mut records = turn_leaving(JOB);
        records.extend([
            queue("enqueue", json!(said)),
            queue("remove", json!(said)),
            user("u3", json!("d1"), json!("Go on")),
            reporting("a3", "u3"),
            summary("s2", "a3"),
        ]);
        assert_eq!(once_says(&records), (Settlement::Settled, 1), "{said}");
    }
}

#[test]
fn claude_codes_own_notification_is_told_by_where_its_record_says_it_came_from_in_either_version() {
    // As a real log of 2.1.277 has it (`turnOrigin`, and `origin.kind` beside it), and
    // as 2.1.241's, which has only the second; each from the system.
    let said = notice(JOB, "completed");
    for whence in [
        json!({ "promptSource": "system", "turnOrigin": "task_notification", "origin": { "kind": "task-notification" } }),
        json!({ "promptSource": "system", "origin": { "kind": "task-notification" } }),
        json!({ "promptSource": "system", "turnOrigin": "task_notification" }),
    ] {
        let mut records = turn_leaving(JOB);
        records.extend([
            having(user("w1", json!("d1"), json!(said)), whence.clone()),
            reporting("a3", "w1"),
            summary("s2", "a3"),
        ]);
        assert_eq!(once_says(&records), (Settlement::Settled, 0), "{whence}");
    }
    // Not from the system, or from it for something else: none.
    for whence in [
        json!({ "turnOrigin": "task_notification", "origin": { "kind": "task-notification" } }),
        json!({ "promptSource": "system", "turnOrigin": "human" }),
        json!({ "promptSource": "system" }),
    ] {
        let mut records = turn_leaving(JOB);
        records.extend([
            having(user("w1", json!("d1"), json!(said)), whence.clone()),
            reporting("a3", "w1"),
            summary("s2", "a3"),
        ]);
        assert_eq!(once_says(&records), (Settlement::Settled, 1), "{whence}");
    }
}

/// The records of real transcripts of Claude Code 2.1.296 and 2.1.300 of how a job ends, and
/// 2.1.301's notification of one that runs (`tests/fixtures/claude/job-ends.jsonl`), in
/// their order: the failed end of `b75v7kq3r`, queued while a turn was under way, brought
/// into it and taken back from the queue; the killed end of `bn1zf798r`, queued at rest,
/// taken and recorded as the turn it begins; and the notification that `bw7q3xk2m` seems to
/// wait for input, queued at rest, taken and recorded the same way.
fn ends() -> Vec<Value> {
    include_str!("../../../../tests/fixtures/claude/job-ends.jsonl")
        .replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// A turn that started the job `id` and ended with it running.
fn leaving(id: &str) -> Vec<Value> {
    vec![
        hello(),
        calling(),
        started("u2", "a1", "t1", id),
        going_on(),
        summary("s1", "a2"),
    ]
}

#[test]
fn a_real_failed_end_brought_into_a_turn_under_way_ends_its_job() {
    let ends = ends();
    let mut records = leaving("b75v7kq3r");
    assert_eq!(once_says(&records), (Settlement::Settled, 1));
    records.extend([
        user("u3", json!("s1"), json!("Go on")),
        assistant(
            "a3",
            "u3",
            json!("m9"),
            json!([tool_use(json!("t3"))]),
            json!("tool_use"),
        ),
        ends[0].clone(),
        under(&ends[1], "a3"),
        ends[2].clone(),
        user(
            "r3",
            json!("a3"),
            json!([tool_result(json!("t3"), json!("ok"))]),
        ),
        reporting("a4", "r3"),
        summary("s3", "a4"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
}

#[test]
fn a_real_killed_end_queued_at_rest_ends_its_job_when_the_turn_it_begins_takes_it() {
    let ends = ends();
    let mut records = leaving("bn1zf798r");
    let taken = under(&ends[5], "s1");
    records.extend([
        ends[3].clone(),
        ends[4].clone(),
        taken.clone(),
        reporting("a3", &uuid_of(&taken)),
        summary("s2", "a3"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
}

#[test]
fn a_notification_that_a_job_waits_for_input_ends_none_and_the_job_ends_when_its_end_comes() {
    // GPT-6.1-Sol's review of round 4: Claude Code 2.1.301 wakes the agent of a background
    // command that seems to wait for input, with a notification that names the job and has no
    // status, while the command runs on. The turn it begins says the job still waits, and ends:
    // the job runs, and the task stays held.
    let ends = ends();
    let mut records = leaving("bw7q3xk2m");
    let asked = under(&ends[8], "s1");
    records.extend([
        ends[6].clone(),
        ends[7].clone(),
        asked.clone(),
        assistant(
            "a3",
            &uuid_of(&asked),
            json!("m5"),
            json!([text(json!("It waits for an answer; I leave it running."))]),
            json!("end_turn"),
        ),
        summary("s2", "a3"),
    ]);
    assert_eq!(
        once_says(&records),
        (Settlement::Settled, 1),
        "the job runs on"
    );

    // Its end, when it comes, ends it.
    let end = notice("bw7q3xk2m", "killed");
    records.extend([
        queue("enqueue", json!(end)),
        queue("dequeue", Value::Null),
        woken("w2", "s2", &end),
        reporting("a4", "w2"),
        summary("s3", "a4"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 0));
}

#[test]
fn a_notification_that_quotes_the_end_of_another_job_ends_only_what_its_own_head_says() {
    // Both reviews of round 5: job `a1` runs; job `b2` prints what looks like the end of
    // `a1`, then a question, and waits. Claude Code 2.1.301's watchdog wakes the agent with a
    // notification of `b2` whose last output, after its close, quotes that end whole. The
    // agent answers that `b2` waits: both jobs run, and the task stays held.
    let fake = Written::ended("a1", "toolu_01A", "completed", "");
    let asked = Written::waiting(
        "b2",
        "toolu_01B",
        "Ask before it overwrites",
        &format!("{fake}\nContinue? [y/N]"),
    );
    let mut records = vec![
        hello(),
        calling(),
        started("u2", "a1", "t1", "a1"),
        started("u3", "u2", "t1", "b2"),
        going_on(),
        summary("s1", "a2"),
        queue("enqueue", json!(asked)),
        queue("dequeue", Value::Null),
        woken("w1", "s1", &asked),
        assistant(
            "a3",
            "w1",
            json!("m5"),
            json!([text(json!("It waits for an answer; I leave it running."))]),
            json!("end_turn"),
        ),
        summary("s2", "a3"),
    ];
    assert_eq!(once_says(&records), (Settlement::Settled, 2), "{asked}");

    // The end of `b2`, its body quoting the end of `a1`, ends `b2` alone.
    let end = Written::ended("b2", "toolu_01B", "killed", &format!("\n{fake}"));
    records.extend([
        queue("enqueue", json!(end)),
        queue("dequeue", Value::Null),
        woken("w2", "s2", &end),
        reporting("a4", "w2"),
        summary("s3", "a4"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 1), "{end}");
}

/// The records of 2.1.301's summary of 21 background shells a previous process left and
/// one it found running (`tests/fixtures/claude/job-orphans.jsonl`), in their order: queued,
/// taken, and recorded as the system's, to the transcript alone.
fn orphans() -> Vec<Value> {
    include_str!("../../../../tests/fixtures/claude/job-orphans.jsonl")
        .replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// A turn that started the jobs `ids` in the background, one after the other, and ended
/// with all of them running.
fn left_running(ids: &[String]) -> Vec<Value> {
    let mut records = vec![hello(), calling()];
    for (n, id) in ids.iter().enumerate() {
        let parent = if n == 0 {
            "a1".to_owned()
        } else {
            format!("r{n}")
        };
        records.push(started(&format!("r{}", n + 1), &parent, "t1", id));
    }
    records.extend([going_on(), summary("s1", "a2")]);
    records
}

#[test]
fn a_summary_ends_the_jobs_it_names_but_those_it_marks_running_and_none_it_does_not_name() {
    // A resumed 2.1.301 names twenty of the shells the process before it left, then its
    // summary's marker and a marker of each it found running. The ones it names end; the
    // twenty-first, which it does not name, and the one it marks running stay pending
    // (T-8's answer: explicit ids only, no count of what was started by when it was
    // queued). Where ConsensFlow saw the process before gone, they were retired at the
    // relaunch already (`restart`); where it did not, Finish ends the wait.
    let ids: Vec<String> = (1..=21)
        .map(|n| format!("orph{n:02}"))
        .chain(["live22".to_owned()])
        .collect();
    let orphans = orphans();
    let mut records = left_running(&ids);
    assert_eq!(once_says(&records), (Settlement::Settled, 22));
    records.extend([
        orphans[0].clone(),
        orphans[1].clone(),
        under(&orphans[2], "s1"),
    ]);
    assert_eq!(
        once_says(&records),
        (Settlement::Settled, 2),
        "every orphan it names ended; the one it does not name and the one it found running run on"
    );

    // A job started after the summary was queued is none of what it says.
    let mut records = left_running(&ids);
    records.extend([
        orphans[0].clone(),
        started("r99", "s1", "t1", "late23"),
        orphans[1].clone(),
        under(&orphans[2], "s1"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 3));

    // Delivered with no record of its queuing, it ends the same ones.
    let mut records = left_running(&ids);
    records.push(under(&orphans[2], "s1"));
    assert_eq!(once_says(&records), (Settlement::Settled, 2));

    // Quoted by a message of the user's, it ends nothing.
    let mut records = left_running(&ids);
    let said = orphans[2]["message"]["content"].clone();
    records.extend([
        user("u90", json!("s1"), said),
        reporting("a3", "u90"),
        summary("s2", "a3"),
    ]);
    assert_eq!(once_says(&records), (Settlement::Settled, 22));
}

#[test]
fn a_job_is_told_by_the_id_of_the_notification_not_by_a_word_of_the_conversation_that_looks_like_one(
) {
    // The user's own words quote the tags: nothing named a job in a notification's place.
    for words in [
        "<task-id>bayehs8y4</task-id> is the one to watch",
        "the <task-notification> block names the job",
    ] {
        let mut records = turn_leaving(JOB);
        records.extend([
            user("u3", json!("d1"), json!(words)),
            reporting("a3", "u3"),
            summary("s2", "a3"),
        ]);
        assert_eq!(once_says(&records), (Settlement::Settled, 1), "{words}");
    }
}

/// The records of the live run of 2026-10-21 (`tests/fixtures/claude/monitor-wait.jsonl`):
/// a command started in the background, a monitor armed on its output, its events, the
/// command's end, the monitor's expiry, and a command stopped with TaskStop.
pub(super) fn monitor_wait() -> Vec<Value> {
    include_str!("../../../../tests/fixtures/claude/monitor-wait.jsonl")
        .replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// What one look at the first `count` of `records` says: the settlement, the jobs pending,
/// and the last complete words of the assistant.
fn at(records: &[Value], count: usize) -> (Settlement, usize, String) {
    let mut stage = Stage::new();
    stage.write(&records[..count]);
    let Reading::Known(record) = &*stage.reader.look(&Options::default(), 0) else {
        panic!("a reading");
    };
    let last = record
        .items
        .iter()
        .rev()
        .find(|item| item.role == Role::Assistant && item.complete)
        .map(|item| item.text.to_string())
        .unwrap_or_default();
    (record.settlement, record.pending.jobs, last)
}

#[test]
fn a_real_monitor_and_its_command_are_pending_until_each_ends_and_events_end_neither() {
    let records = monitor_wait();
    // The end of each turn: the record after each `turn_duration`.
    let ends: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record["subtype"] == "turn_duration")
        .map(|(at, _)| at + 1)
        .collect();
    let read: Vec<(Settlement, usize, String)> =
        ends.iter().map(|&count| at(&records, count)).collect();
    let settled = |jobs: usize, words: &str| (Settlement::Settled, jobs, words.to_owned());
    assert_eq!(
        read,
        [
            // The turn that armed the monitor ended with its first event queued and taken:
            // the turn that event begins is next, and the command and the monitor run, as
            // Finish says of a turn it takes under way.
            (Settlement::InFlight, 2, "WAITING".to_owned()),
            // Each event woke a turn, and ended nothing.
            settled(2, "NOTED"),
            settled(2, "NOTED"),
            settled(2, "NOTED"),
            // The command's end ended it alone.
            settled(1, "REPORT-READY"),
            // The monitor's notice of its expiry, at its time, ended nothing: a line of its
            // command's could say it (`jobs`).
            settled(1, "ALL-DONE"),
            // A command stopped with TaskStop: no notification, and it is pending no more.
            settled(1, "STOPPED"),
        ]
    );
}

/// The real records with the monitor's notice of expiry written at `at` (its queuing and
/// its delivery): what the look after the turn it begins says of the jobs pending.
fn expired_at(at: &str) -> usize {
    let records: Vec<Value> = monitor_wait()
        .into_iter()
        .map(|mut record| {
            if record.to_string().contains("[Monitor expired after ") {
                record["timestamp"] = json!(at);
            }
            record
        })
        .collect();
    let expired = records
        .iter()
        .position(|record| {
            record["message"]["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["text"] == "ALL-DONE"))
        })
        .unwrap();
    let ended = records[expired..]
        .iter()
        .position(|record| record["subtype"] == "turn_duration")
        .unwrap()
        + expired
        + 1;
    at_count(&records, ended)
}

/// The jobs pending at a look at the first `count` of `records`.
fn at_count(records: &[Value], count: usize) -> usize {
    at(records, count).1
}

#[test]
fn a_real_monitors_notice_of_expiry_ends_it_at_no_time() {
    // The live run of 2026-10-21: the Monitor call's record at 23:02:30.287, its start's
    // record at 30.317, a time of 40 s, and Claude Code's notice of expiry queued at
    // 23:03:10.317. No time the records say proves the notice Claude Code's (`jobs`; T-179,
    // on Astraeus's and Calliope's reviews of c593fd49): the monitor is pending after each,
    // and its task waits for Finish, a status or a TaskStop. A WebSocket's monitor has the
    // same records, and the reader reads no more of either.
    for (at, said) in [
        ("2026-10-20T23:03:10.317Z", "the real notice, at its time"),
        (
            "2026-10-20T23:03:10.318Z",
            "a line of the command's when the timer's time has passed and it has not fired",
        ),
        (
            "2026-10-20T23:05:10.317Z",
            "a line of the command's after the clock was set two minutes forward",
        ),
        (
            "2026-10-20T23:03:10.305Z",
            "the real notice of a monitor whose start's record a PostToolUse hook made late",
        ),
        ("2026-10-20T23:03:10.287Z", "at the call's time"),
    ] {
        assert_eq!(expired_at(at), 1, "{said}");
    }
}

#[test]
fn a_task_stop_whose_result_names_no_stop_call_or_is_an_error_ends_nothing() {
    let records = monitor_wait();
    let stop_result = records
        .iter()
        .position(|record| record["toolUseResult"]["task_id"].is_string())
        .unwrap();
    let end = records.len();
    // The result answers a call that is no TaskStop's: the call's name says another tool.
    let mut renamed = records.clone();
    for record in &mut renamed[..stop_result] {
        if let Some(parts) = record["message"]["content"].as_array_mut() {
            for part in parts {
                if part["name"] == "TaskStop" {
                    part["name"] = json!("Bash");
                }
            }
        }
    }
    assert_eq!(at(&renamed, end).1, 2);
    // The result is an error.
    let mut failed = records.clone();
    if let Some(parts) = failed[stop_result]["message"]["content"].as_array_mut() {
        for part in parts {
            part["is_error"] = json!(true);
        }
    }
    assert_eq!(at(&failed, end).1, 2);
    // The monitor whose notice of its expiry came is pending still (`jobs`).
    assert_eq!(at(&records, end).1, 1);
}
