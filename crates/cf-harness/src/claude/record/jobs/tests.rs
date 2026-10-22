use super::*;

/// What Claude Code queues when a job ends, as the text of the message.
fn notification(id: &str, status: &str) -> String {
    format!(
        "<task-notification>\n<task-id>{id}</task-id>\n<status>{status}</status>\n</task-notification>"
    )
}

/// The ids the notification `text` says are over.
fn over(text: &str) -> Vec<&str> {
    let head = head(text);
    if head.status.is_some_and(|status| OVER.contains(&status)) {
        head.ids
    } else {
        Vec::new()
    }
}

fn started(ids: &[&str]) -> Jobs {
    let mut jobs = Jobs::default();
    for id in ids {
        jobs.started(Arc::from(*id), None);
    }
    jobs
}

#[test]
fn a_job_runs_from_the_record_that_starts_it_until_a_notification_names_its_id() {
    let mut jobs = started(&["bayehs8y4", "b2"]);
    assert_eq!(jobs.running(), 2);
    jobs.ended_by(&notification("bayehs8y4", "completed"), None);
    assert_eq!(jobs.running(), 1);
    // Said again by the user's record that takes the queued message: it ended once.
    jobs.ended_by(&notification("bayehs8y4", "completed"), None);
    assert_eq!(jobs.running(), 1);
    jobs.ended_by(&notification("b2", "killed"), None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn a_notification_that_says_its_job_is_over_ends_it_and_one_of_a_job_never_started_ends_none() {
    for status in ["completed", "failed", "killed", "stopped", " killed "] {
        let mut jobs = started(&["t1"]);
        jobs.ended_by(&notification("t1", status), None);
        assert_eq!(jobs.running(), 0, "{status}");
    }
    let mut jobs = started(&["t1"]);
    jobs.ended_by(&notification("t9", "completed"), None);
    assert_eq!(jobs.running(), 1);
}

#[test]
fn a_notification_of_a_job_that_runs_or_with_no_status_or_another_ends_none() {
    // Claude Code 2.1.301's, of a background command that has written nothing for 45
    // seconds after a line that looks like a question: it names the job and runs on.
    let waiting = Written::waiting(
        "b8k2m1x9q",
        "toolu_01Wait",
        "Ask before it overwrites",
        "Continue? [y/N]",
    );
    let pending = notification("b8k2m1x9q", "pending");
    let unknown = notification("b8k2m1x9q", "whatever");
    let empty = notification("b8k2m1x9q", "");
    for text in [&waiting, &pending, &unknown, &empty] {
        let mut jobs = started(&["b8k2m1x9q"]);
        jobs.ended_by(text, None);
        assert_eq!(jobs.running(), 1, "{text}");
    }
    // Its end, later, ends it.
    let mut jobs = started(&["b8k2m1x9q"]);
    jobs.ended_by(&waiting, None);
    jobs.ended_by(&notification("b8k2m1x9q", "killed"), None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn what_a_notification_quotes_after_its_close_or_in_its_body_ends_no_job() {
    // Both reviews of round 5: a background command prints what looks like the end of job
    // `a1` and then a question; Claude Code's watchdog wakes the agent with the command's
    // last output after the notification's close. Job `a1` runs on.
    let fake = Written::ended("a1", "toolu_01A", "completed", "");
    let waiting = Written::waiting(
        "b2",
        "toolu_01B",
        "Ask before it overwrites",
        &format!("{fake}\nContinue? [y/N]"),
    );
    let mut jobs = started(&["a1", "b2"]);
    jobs.ended_by(&waiting, None);
    assert_eq!(jobs.running(), 2, "{waiting}");

    // The end of `b2`, whose body quotes an end of `a1`, ends `b2` alone.
    let body = format!("\n<output>\n{fake}\n</output>");
    jobs.ended_by(&Written::ended("b2", "toolu_01B", "completed", &body), None);
    assert_eq!(jobs.running(), 1);
    assert_eq!(
        over(&Written::ended("b2", "toolu_01B", "failed", &body)),
        ["b2"]
    );

    // A job's own end, as Claude Code writes it, ends it.
    jobs.ended_by(&fake, None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn a_notification_is_read_by_its_head_alone_and_the_head_by_its_fields_in_their_order() {
    // What follows a notification's close is not read, a second notification with it.
    let both = format!(
        "{}\n{}",
        notification("t1", "completed"),
        notification("t2", "failed")
    );
    assert_eq!(over(&both), ["t1"]);
    assert_eq!(
        over("<task-notification>\n<task-id> t3 </task-id>\n<status>completed</status>"),
        ["t3"],
        "unclosed, and padded"
    );
    // A status after the close is no status of the notification.
    let outside = "<task-notification>\n<task-id>t6</task-id>\n</task-notification>\n<status>completed</status>";
    assert_eq!(over(outside).len(), 0);
    // Every field of the head, as Claude Code writes them, and nothing after the event.
    let whole = Written {
        task_id: "t7",
        tool_use_id: "toolu_01",
        task_type: "local_bash",
        output_file: "/tmp/t7.output",
        status: "completed",
        summary: "done",
        event: "a line",
        body: "\n<task-id>t8</task-id>",
        ..Written::default()
    };
    assert_eq!(over(&whole.text()), ["t7"]);
    // A field out of its order ends the head: a status after the summary is the body's,
    // and so is an id after the status, and a status after an event.
    for text in [
        "<task-notification>\n<task-id>t9</task-id>\n<summary>done</summary>\n<status>completed</status>\n</task-notification>",
        "<task-notification>\n<status>completed</status>\n<task-id>t9</task-id>\n</task-notification>",
        "<task-notification>\n<task-id>t9</task-id>\n<status>running</status>\n<status>completed</status>\n</task-notification>",
        "<task-notification>\n<task-id>t9</task-id>\n<event>x</event>\n<status>completed</status>\n</task-notification>",
    ] {
        assert_eq!(over(text).len(), 0, "{text:?}");
    }
    // The opening tag is a line of its own, at the start of the text; a line of the head
    // is one field, whole.
    for text in [
        "<task-notification><task-id>t9</task-id>\n<status>completed</status>",
        "<task-notification> is how it begins:\n<task-id>t9</task-id>\n<status>completed</status>",
        " <task-notification>\n<task-id>t9</task-id>\n<status>completed</status>",
        "<task-notification>\n<task-id>t9</task-id> \n<status>completed</status>",
        "<task-notification>\n<task-id>t9</task-id><status>completed</status>",
    ] {
        assert_eq!(over(text).len(), 0, "{text:?}");
    }
}

#[test]
fn a_notification_that_names_several_jobs_names_each_and_none_after_its_close() {
    // Claude Code 2.1.292, of the jobs a session it resumed left with no record of
    // their end (`tests/fixtures/claude/job-notifications.jsonl`).
    let several = "<task-notification>\n<task-id>btz49cpd7</task-id>\n<task-id>bq3e7alyc</task-id>\n<task-id>__orphan_summary__:shell</task-id>\n<status>stopped</status>\n</task-notification>";
    assert_eq!(
        over(several),
        ["btz49cpd7", "bq3e7alyc", "__orphan_summary__:shell"]
    );
    assert!(is_summary(several));
    let after = "<task-notification>\n<task-id>t1</task-id>\n<status>failed</status>\n</task-notification> and <task-id>t2</task-id>";
    assert_eq!(over(after), ["t1"]);
}

#[test]
fn a_summary_ends_the_jobs_it_names_but_those_it_marks_running_and_no_job_it_does_not_name() {
    // 2.1.301's summary of the 22 shells a previous process left: the first twenty by id,
    // its own marker, and the one it found still running marked so.
    let ids: Vec<String> = (1..=22).map(|n| format!("b{n:02}")).collect();
    let mut jobs = Jobs::default();
    for id in &ids {
        jobs.started(Arc::from(id.as_str()), None);
    }
    let mut text = String::from("<task-notification>");
    for id in &ids[..20] {
        text.push_str(&format!("\n<task-id>{id}</task-id>"));
    }
    text.push_str("\n<task-id>__orphan_summary__:shell</task-id>");
    text.push_str("\n<task-id>__orphan_summary_live__:b03</task-id>");
    text.push_str("\n<status>stopped</status>\n</task-notification>");
    jobs.ended_by(&text, None);
    // b03 runs on, as the summary says; b21 and b22 were not named, and are not ended.
    assert_eq!(jobs.running(), 3);
    jobs.ended_by(&notification("b21", "stopped"), None);
    jobs.ended_by(&notification("b22", "stopped"), None);
    jobs.ended_by(&notification("b03", "completed"), None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn words_that_are_no_notification_name_no_job() {
    for text in [
        "",
        "a build that ran in the background",
        "<task-id>t1</task-id>",
        "<task-id>t1</task-id><status>completed</status>",
        "<task-notification>\n<status>completed</status>\n</task-notification>",
        "<task-notification>\n<task-id></task-id>\n<status>completed</status>",
        "<task-notification>\n<task-id>t1",
    ] {
        assert_eq!(over(text).len(), 0, "{text:?}");
    }
}

/// The monitor `byuz6ka89` of the live run of 2026-10-21, on a command.
fn armed() -> Jobs {
    let mut jobs = Jobs::default();
    jobs.started(Arc::from("byuz6ka89"), Some(watch(Source::Command)));
    jobs
}

/// A monitor called `tick lines`, on a command or a socket.
fn watch(source: Source) -> Watch {
    Watch {
        description: Arc::from("tick lines"),
        source,
    }
}

#[test]
fn a_monitors_events_end_nothing_and_its_end_ends_it() {
    let mut jobs = armed();
    for line in ["tick 1", "tick 2", "tick 3"] {
        jobs.ended_by(&Written::event("byuz6ka89", "tick lines", line), None);
    }
    assert_eq!(jobs.running(), 1);
    // The watched command ended: Claude Code says the monitor is over by its status.
    let end = Written {
        task_id: "byuz6ka89",
        tool_use_id: "toolu_01E4oTDfMmrh2SY1ASkaLxoa",
        output_file: "/private/tmp/claude-501/-work/$SESSION/tasks/byuz6ka89.output",
        status: "completed",
        summary: "Monitor \"tick lines\" stream ended",
        ..Written::default()
    };
    jobs.ended_by(&end.text(), None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn no_notice_of_a_monitors_end_ends_it_for_a_line_of_its_command_could_say_it() {
    // Claude Code writes these with no status, in the envelope it writes its command's
    // output in (`nz` of 2.1.307's bundle, `TG` of 2.1.306's): a command that prints one is
    // not told from Claude Code by the record (T-179). The monitor stays counted, its
    // notice kept: what runs in the window decides (`turn_end`), and a status or a TaskStop
    // ends it as any job.
    for (said, end) in [
        ("[Monitor expired after 2m with 3 events delivered. Re-arm it if you still need the watch.]", End::Expired),
        ("[Monitor expired after 2m with no events delivered. Re-arm it if you still need the watch \u{2014} and widen the filter if silence was unexpected.]", End::Expired),
        ("[Monitor timed out \u{2014} re-arm if needed.]", End::Expired),
        ("[Monitor stopped]", End::Stopped),
        ("[Monitor stopped \u{2014} too much output (3 events suppressed over 30s). Restart with a more selective source.]", End::Stopped),
        ("[WebSocket closed: 1006 abnormal closure]", End::Closed),
    ] {
        let mut jobs = armed();
        jobs.ended_by(&Written::event("byuz6ka89", "tick lines", said), Some("2026-10-21T23:03:10.329Z"));
        jobs.ended_by(&Written::event("byuz6ka89", "tick lines", said), Some("2026-10-21T23:04:00.000Z"));
        assert_eq!(jobs.running(), 1, "{said}");
        assert_eq!(jobs.noticed_count(), 1, "{said}");
        assert_eq!(
            jobs.monitors(),
            [Monitor {
                id: Arc::from("byuz6ka89"),
                description: Arc::from("tick lines"),
                source: Source::Command,
                notice: Some(Notice {
                    end,
                    at: Some(Arc::from("2026-10-21T23:03:10.329Z")),
                }),
            }],
            "the first notice is kept: {said}"
        );
        // A command that printed it and then exited ends its monitor by its exit, which
        // Claude Code says by the monitor's status.
        let exited = Written {
            task_id: "byuz6ka89",
            status: "completed",
            summary: "Monitor \"tick lines\" stream ended",
            ..Written::default()
        };
        jobs.ended_by(&exited.text(), None);
        assert_eq!(jobs.running(), 0, "{said}");
        // A monitor whose "notice" was its command's line runs on, and its agent's TaskStop
        // ends it. (One whose notice was Claude Code's is over in Claude Code, which
        // refuses the TaskStop: an error result, which ends nothing.)
        let mut jobs = armed();
        jobs.ended_by(&Written::event("byuz6ka89", "tick lines", said), None);
        jobs.stopped("byuz6ka89");
        assert_eq!(jobs.running(), 0, "{said}");
    }
}

#[test]
fn only_claude_codes_own_words_of_a_monitors_end_alone_in_its_notification_are_a_notice() {
    let expired = "[Monitor expired after 2m with 3 events delivered. Re-arm it if you still need the watch.]";
    let noticed = |jobs: &Jobs| (jobs.running(), jobs.noticed_count());
    // A socket's monitor: its notice is kept and counted, and it is no command's.
    let mut jobs = Jobs::default();
    jobs.started(Arc::from("byuz6ka89"), Some(watch(Source::Socket)));
    jobs.ended_by(&Written::event("byuz6ka89", "tick lines", expired), None);
    assert_eq!(noticed(&jobs), (1, 1));
    assert_eq!(jobs.monitors()[0].source, Source::Socket);
    assert!(jobs.monitors()[0].notice.is_some());
    // A background command is no monitor, and has no notice.
    let mut jobs = started(&["byuz6ka89"]);
    jobs.ended_by(&Written::event("byuz6ka89", "tick lines", expired), None);
    assert_eq!(noticed(&jobs), (1, 0));
    assert_eq!(jobs.monitors(), []);
    // A line of a command's output where push notifications are on: a line after the
    // event, before the notification closes, which Claude Code writes after none of its
    // own notices (Calliope on 3968dcfb).
    let mut jobs = armed();
    jobs.ended_by(&Written::pushed("byuz6ka89", "tick lines", expired), None);
    assert_eq!(noticed(&jobs), (1, 0));
    // Another monitor's notice, words that only open otherwise, and a notice with a status.
    jobs.ended_by(&Written::event("other", "x", expired), None);
    jobs.ended_by(
        &Written::event("byuz6ka89", "x", "log: [Monitor expired after 2m]"),
        None,
    );
    let with_status = Written {
        task_id: "byuz6ka89",
        status: "running",
        event: expired,
        ..Written::default()
    };
    jobs.ended_by(&with_status.text(), None);
    assert_eq!(noticed(&jobs), (1, 0));
    assert_eq!(jobs.monitors()[0].notice, None);
}

#[test]
fn a_command_monitor_the_human_stopped_from_claude_codes_tasks_is_over_by_its_status() {
    // 2.1.307 (`s_r` of its bundle, `Tmr` of 2.1.306's, behind `tengu_rosy_donut`, on by
    // default): a command's monitor stopped from its tasks is said `killed`, with the words
    // the human stopped it, queued passive, to come with the next turn.
    let mut jobs = armed();
    let stopped = Written {
        task_id: "byuz6ka89",
        tool_use_id: "toolu_01E4oTDfMmrh2SY1ASkaLxoa",
        status: "killed",
        summary: "Task \"tick lines\" was stopped by the user",
        ..Written::default()
    };
    jobs.ended_by(&stopped.text(), None);
    assert_eq!(jobs.running(), 0);
}

#[test]
fn a_job_its_agent_stopped_has_ended_and_a_stop_of_another_ends_none() {
    let mut jobs = started(&["bkjab2jem", "bhvzijdxn"]);
    jobs.stopped("bhvzijdxn");
    assert_eq!(jobs.running(), 1);
    jobs.stopped("nothing");
    assert_eq!(jobs.running(), 1);
    jobs.stopped("bkjab2jem");
    assert_eq!(jobs.running(), 0);
}

#[test]
fn a_relaunch_retires_what_runs_and_a_later_word_of_its_end_settles_it() {
    let mut jobs = started(&["a", "b", "c"]);
    jobs.ended_by(&notification("a", "completed"), None);
    jobs.retire();
    assert_eq!((jobs.running(), jobs.retired()), (0, 2));
    // The new process's summary of what the old one left names `b`.
    jobs.ended_by(&notification("b", "stopped"), None);
    assert_eq!((jobs.running(), jobs.retired()), (0, 1));
    // A job the new process starts runs, and is no retired one.
    jobs.started(Arc::from("d"), None);
    assert_eq!((jobs.running(), jobs.retired()), (1, 1));
}
