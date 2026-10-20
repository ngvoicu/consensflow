//! When each disruption is played, and where: no harness, the moments written by
//! hand.

use super::*;

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

fn staff(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn worker(handle: &str, harness: &str, working: bool, tool: bool) -> Worker {
    Worker {
        handle: handle.to_owned(),
        harness: harness.to_owned(),
        task: Some(4),
        working,
        tool,
    }
}

fn moment(elapsed: Duration, workers: Vec<Worker>) -> Moment {
    Moment {
        elapsed,
        chief: "claude".to_owned(),
        chief_idle: true,
        chief_compacted: false,
        workers,
    }
}

/// What `next` chose: the disruption, where, and at which moment.
fn chose(choice: Option<Choice>) -> Option<(Disruption, Target, &'static str)> {
    choice.map(|choice| (choice.disruption, choice.target, choice.how))
}

#[test]
fn the_restart_after_a_compaction_waits_for_a_chief_whose_record_still_ends_in_it() {
    // A5's shape: the Resume reads a record that ends in the idle compaction. Once the
    // chief has had a turn since, the strict moment waits for the next compaction it
    // will never have, and the loose one comes half an interval late, lacking it.
    let mut schedule = Schedule::new(
        &[Disruption::Compact, Disruption::Restart],
        minutes(4),
        "claude",
        &staff(&["claude"]),
    );
    schedule.played(Disruption::Compact, minutes(4), "strict".to_owned());
    let tooling = vec![worker("w", "claude", true, true)];
    let mut after = moment(minutes(8), tooling.clone());
    assert_eq!(schedule.next(&after), None, "the chief had a turn since");
    after.chief_compacted = true;
    let strict = schedule.next(&after).unwrap();
    assert_eq!((strict.how, strict.lacks), ("strict", None));
    after.chief_compacted = false;
    after.elapsed = minutes(10);
    let loose = schedule.next(&after).unwrap();
    assert_eq!(
        (loose.how, loose.lacks),
        ("loose", Some(NO_IDLE_COMPACTION))
    );
    // Played so, it is kept as played without A5's shape.
    schedule.played(Disruption::Restart, minutes(10), "loose".to_owned());
    schedule.lacked(Disruption::Restart, NO_IDLE_COMPACTION);
    assert_eq!(schedule.without_their_shape().len(), 1);
    assert_eq!(
        schedule.planned[1].lacked.as_deref(),
        Some(NO_IDLE_COMPACTION)
    );
    // With no compaction played, a worker mid-tool is the restart's strict moment, and
    // it lacks nothing.
    let alone = Schedule::new(
        &[Disruption::Restart],
        minutes(4),
        "claude",
        &staff(&["claude"]),
    );
    let choice = alone.next(&moment(minutes(4), tooling)).unwrap();
    assert_eq!((choice.how, choice.lacks), ("strict", None));
}

#[test]
fn the_chosen_disruptions_are_due_one_interval_after_another_in_the_runs_order() {
    let schedule = Schedule::new(
        &[Disruption::Suspend, Disruption::Pause, Disruption::Kill],
        minutes(10),
        "claude",
        &staff(&["claude", "devin"]),
    );
    let planned: Vec<(Disruption, Duration)> = schedule
        .planned
        .iter()
        .map(|planned| (planned.disruption, planned.due))
        .collect();
    assert_eq!(
        planned,
        [
            (Disruption::Pause, minutes(10)),
            (Disruption::Kill, minutes(20)),
            (Disruption::Suspend, minutes(30))
        ]
    );
    let flags: Vec<&str> = Disruption::ALL.iter().map(|one| one.flag()).collect();
    assert_eq!(
        flags,
        [
            "pause",
            "kill",
            "compact",
            "restart",
            "switch",
            "devin-cut",
            "suspend"
        ]
    );
}

#[test]
fn each_disruption_is_due_the_interval_after_the_one_before_it_was_played_however_late() {
    let mut schedule = Schedule::new(
        &[Disruption::Pause, Disruption::Kill, Disruption::Suspend],
        minutes(8),
        "claude",
        &staff(&["claude"]),
    );
    let tooling = vec![worker("w", "claude", true, true)];
    // The pause's moment came only at minute 25.
    assert!(schedule
        .next(&moment(minutes(25), tooling.clone()))
        .is_some());
    schedule.played(Disruption::Pause, minutes(25), "strict".to_owned());
    assert_eq!(schedule.planned[1].due, minutes(33));
    // The kill is not overdue at the next look: it waits its interval.
    assert_eq!(schedule.next(&moment(minutes(26), tooling.clone())), None);
    assert!(schedule
        .next(&moment(minutes(33), tooling.clone()))
        .is_some());
    // One that failed moves the next as one that was played does.
    schedule.failed(
        Disruption::Kill,
        minutes(34),
        "the window did not exit".to_owned(),
    );
    assert_eq!(schedule.planned[2].due, minutes(42));
    assert!(matches!(schedule.planned[1].state, State::Failed { .. }));
    assert_eq!(schedule.next(&moment(minutes(41), tooling)), None);
}

#[test]
fn the_compaction_is_typed_only_into_a_chief_that_is_claude_code_at_its_moment() {
    let compact = Schedule::new(
        &[Disruption::Compact],
        minutes(1),
        "claude",
        &staff(&["claude", "devin"]),
    );
    let mut switched = moment(minutes(5), Vec::new());
    switched.chief = "devin".to_owned();
    assert_eq!(
        compact.next(&switched),
        None,
        "/compact is ordinary input to Devin"
    );
    switched.chief = "claude".to_owned();
    assert!(compact.next(&switched).is_some());
}

#[test]
fn what_the_staff_or_the_chief_cannot_play_is_said_from_the_start() {
    let schedule = Schedule::new(&Disruption::ALL, minutes(10), "devin", &staff(&["devin"]));
    let impossible: Vec<(&str, &State)> = schedule
        .planned
        .iter()
        .filter(|planned| matches!(planned.state, State::Impossible(_)))
        .map(|planned| (planned.disruption.flag(), &planned.state))
        .collect();
    assert_eq!(
        impossible,
        [
            (
                "compact",
                &State::Impossible(
                    "the chief's harness, devin, is not Claude Code, whose record says a compaction"
                        .to_owned()
                )
            ),
            (
                "switch",
                &State::Impossible(
                    "no harness on the staff other than the chief's, devin".to_owned()
                )
            ),
        ]
    );
    let schedule = Schedule::new(
        &Disruption::ALL,
        minutes(10),
        "claude",
        &staff(&["claude", "codex"]),
    );
    assert_eq!(
        schedule.planned[5].state,
        State::Impossible("no Devin on the staff".to_owned())
    );
}

#[test]
fn a_disruption_waits_for_its_time_then_its_strict_moment_then_takes_a_loose_one_late() {
    let schedule = Schedule::new(
        &[Disruption::Kill],
        minutes(10),
        "claude",
        &staff(&["claude"]),
    );
    let thinking = vec![worker("long-claude-1-worker", "claude", true, false)];
    let tooling = vec![worker("long-claude-1-worker", "claude", true, true)];
    // Not yet due.
    assert_eq!(schedule.next(&moment(minutes(9), tooling.clone())), None);
    // Due, and a worker is in a tool call: the strict moment.
    assert_eq!(
        chose(schedule.next(&moment(minutes(10), tooling))),
        Some((
            Disruption::Kill,
            Target::Window {
                handle: "long-claude-1-worker".to_owned(),
                task: Some(4)
            },
            "strict"
        ))
    );
    // Due, only a turn at work: it waits half an interval, then takes it.
    assert_eq!(schedule.next(&moment(minutes(12), thinking.clone())), None);
    assert_eq!(
        chose(schedule.next(&moment(minutes(15), thinking))).map(|(_, _, said)| said),
        Some("loose")
    );
    // Nobody at work: no moment at all.
    assert_eq!(schedule.next(&moment(minutes(40), Vec::new())), None);
}

#[test]
fn one_disruption_at_a_time_and_each_after_the_one_before_was_played() {
    let mut schedule = Schedule::new(
        &[Disruption::Pause, Disruption::Compact, Disruption::DevinCut],
        minutes(5),
        "claude",
        &staff(&["claude", "devin"]),
    );
    let workers = vec![
        worker("long-claude-1-worker", "claude", true, true),
        worker("long-devin-1-worker", "devin", true, true),
    ];
    // The compact and the Devin cut are due, but the pause before them was not played.
    let (first, target, _) = chose(schedule.next(&moment(minutes(16), workers.clone()))).unwrap();
    assert_eq!((first, target), (Disruption::Pause, Target::Task(4)));
    schedule.played(first, minutes(16), "strict".to_owned());
    assert_eq!(schedule.next(&moment(minutes(16), workers.clone())), None);
    assert_eq!(
        schedule
            .next(&moment(minutes(21), workers.clone()))
            .unwrap()
            .disruption,
        Disruption::Compact
    );
    schedule.played(Disruption::Compact, minutes(21), "strict".to_owned());
    let (devin, target, _) = chose(schedule.next(&moment(minutes(26), workers))).unwrap();
    assert_eq!(devin, Disruption::DevinCut);
    assert_eq!(
        target,
        Target::Window {
            handle: "long-devin-1-worker".to_owned(),
            task: Some(4)
        }
    );
    assert!(!schedule.done());
    schedule.close("the project ended before its moment came");
    assert!(schedule.done());
    assert_eq!(schedule.not_played().len(), 1);
    assert_eq!(schedule.not_played()[0].disruption, Disruption::DevinCut);
}

#[test]
fn the_switch_wants_a_job_running_the_compact_an_idle_chief_and_the_suspend_any_work() {
    let schedule = Schedule::new(
        &[Disruption::Switch],
        minutes(1),
        "claude",
        &staff(&["claude", "codex"]),
    );
    let thinking = vec![worker("w", "codex", true, false)];
    assert_eq!(schedule.next(&moment(minutes(1), thinking.clone())), None);
    assert_eq!(
        chose(schedule.next(&moment(minutes(1), vec![worker("w", "codex", true, true)]))),
        Some((Disruption::Switch, Target::Project, "strict"))
    );
    assert_eq!(
        chose(schedule.next(&moment(minutes(2), thinking))).map(|(_, _, said)| said),
        Some("loose")
    );
    let compact = Schedule::new(
        &[Disruption::Compact],
        minutes(1),
        "claude",
        &staff(&["claude"]),
    );
    let mut busy = moment(minutes(9), Vec::new());
    busy.chief_idle = false;
    assert_eq!(compact.next(&busy), None);
    busy.chief_idle = true;
    assert!(compact.next(&busy).is_some());
    let suspend = Schedule::new(
        &[Disruption::Suspend],
        minutes(1),
        "claude",
        &staff(&["claude"]),
    );
    assert!(suspend
        .next(&moment(
            minutes(1),
            vec![worker("w", "claude", true, false)]
        ))
        .is_some());
    assert!(suspend
        .next(&moment(
            minutes(9),
            vec![worker("w", "claude", false, false)]
        ))
        .is_none());
}
