//! What the owner does at each look at a project, and in which order.

use super::*;

const TIMEOUT: Duration = Duration::from_secs(40 * 60);

fn scenario(nudge: Option<&'static str>, follow_ups: Vec<FollowUp>) -> Scenario {
    Scenario {
        id: "owner",
        title: "",
        fixture: "site",
        prompt: String::new(),
        follow_ups,
        answers: Vec::new(),
        fallback: String::new(),
        nudge,
        quiet: Duration::from_secs(90),
        expectations: Vec::new(),
    }
}

/// A look at a project at rest, quiet for `quiet` seconds.
fn at_rest(quiet: u64) -> Seen {
    Seen {
        elapsed: Duration::from_secs(300),
        quiet: Duration::from_secs(quiet),
        busy: false,
        dialog: false,
        board_empty: true,
    }
}

fn asked(id: &str) -> OpenQuestion {
    OpenQuestion {
        id: id.to_owned(),
        text: format!("{id}: which one?"),
    }
}

/// The step of `owner` at `seen`, the chief having asked `open`.
fn step(owner: &mut Owner, scenario: &Scenario, seen: &Seen, open: Option<OpenQuestion>) -> Step {
    owner
        .step(scenario, seen, TIMEOUT, &mut || Ok(open.clone()))
        .unwrap()
}

#[test]
fn a_chiefs_dialog_is_answered_once_the_board_has_been_quiet_ten_seconds_and_eight_times_at_most() {
    let scenario = scenario(None, Vec::new());
    let mut owner = Owner::new(&scenario);
    let dialog = |quiet| Seen {
        dialog: true,
        busy: true,
        ..at_rest(quiet)
    };
    assert_eq!(
        step(&mut owner, &scenario, &dialog(10), None),
        Step::Wait,
        "ten is not more than ten"
    );
    for answered in 1..=MAX_ANSWERS {
        assert_eq!(step(&mut owner, &scenario, &dialog(11), None), Step::Dialog);
        assert_eq!(owner.dialogs, answered);
    }
    assert_eq!(step(&mut owner, &scenario, &dialog(11), None), Step::Wait);
    assert_eq!(owner.dialogs, MAX_ANSWERS);
}

#[test]
fn a_question_in_the_terminal_is_answered_once_when_the_chief_is_at_rest_and_the_board_quiet() {
    let scenario = scenario(None, Vec::new());
    let mut owner = Owner::new(&scenario);
    let question = Some(asked("a2"));
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(20), question.clone()),
        Step::Wait
    );
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                busy: true,
                ..at_rest(30)
            },
            question.clone()
        ),
        Step::Wait,
        "not while a task or the chief is at work"
    );
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(21), question.clone()),
        Step::Reply(asked("a2"))
    );
    assert_eq!(owner.replies, 1);
    // The same question again is answered no more; another one is.
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(30), question),
        Step::Wait
    );
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(30), Some(asked("a5"))),
        Step::Reply(asked("a5"))
    );
    assert_eq!(owner.replies, 2);
    // Nothing asked, nothing answered.
    assert_eq!(step(&mut owner, &scenario, &at_rest(30), None), Step::Wait);
}

#[test]
fn the_ledger_is_asked_for_the_open_question_only_where_the_owner_could_answer_it() {
    let scenario = scenario(None, Vec::new());
    let mut owner = Owner::new(&scenario);
    let reads = std::cell::Cell::new(0);
    let look = |owner: &mut Owner, seen: &Seen| {
        owner
            .step(&scenario, seen, TIMEOUT, &mut || {
                reads.set(reads.get() + 1);
                Ok(None)
            })
            .unwrap()
    };
    look(
        &mut owner,
        &Seen {
            busy: true,
            ..at_rest(100)
        },
    );
    look(&mut owner, &at_rest(5));
    assert_eq!(reads.get(), 0);
    look(&mut owner, &at_rest(30));
    assert_eq!(reads.get(), 1);
    // A question that cannot be read is the run's failure, not the owner's silence.
    let failed = owner
        .step(&scenario, &at_rest(30), TIMEOUT, &mut || {
            Err(crate::Error::Refused(
                "the ledger cannot be read".to_owned(),
            ))
        })
        .unwrap_err();
    assert_eq!(failed.to_string(), "the ledger cannot be read");
}

#[test]
fn it_answers_no_more_than_eight_questions_in_the_terminal() {
    let scenario = scenario(None, Vec::new());
    let mut owner = Owner::new(&scenario);
    for n in 0..MAX_ANSWERS {
        let id = format!("q{n}");
        assert_eq!(
            step(&mut owner, &scenario, &at_rest(30), Some(asked(&id))),
            Step::Reply(asked(&id))
        );
    }
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(30), Some(asked("q9"))),
        Step::Wait
    );
}

#[test]
fn a_chief_that_stopped_without_asking_is_nudged_after_a_minute_twice_at_most_and_only_if_it_has_nothing_on_the_board(
) {
    let scenario = scenario(Some("Da, cum recomanzi. Continuă."), Vec::new());
    let mut owner = Owner::new(&scenario);
    let nudge = Step::Nudge("Da, cum recomanzi. Continuă.".to_owned());
    assert_eq!(step(&mut owner, &scenario, &at_rest(60), None), Step::Wait);
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                board_empty: false,
                ..at_rest(70)
            },
            None
        ),
        Step::Wait,
        "work is on the board"
    );
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                busy: true,
                ..at_rest(70)
            },
            None
        ),
        Step::Wait
    );
    assert_eq!(step(&mut owner, &scenario, &at_rest(61), None), nudge);
    assert_eq!(step(&mut owner, &scenario, &at_rest(61), None), nudge);
    assert_eq!((owner.nudges, MAX_NUDGES), (2, 2));
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(61), None),
        Step::Wait,
        "twice at most"
    );
}

#[test]
fn a_scenario_with_no_nudge_never_nudges_and_a_chief_that_was_answered_is_not_nudged() {
    let plain = scenario(None, Vec::new());
    let mut owner = Owner::new(&plain);
    assert_eq!(step(&mut owner, &plain, &at_rest(80), None), Step::Wait);
    let nudging = scenario(Some("Continuă."), Vec::new());
    // It asked, and was answered: what it does next is its own.
    let mut owner = Owner::new(&nudging);
    assert_eq!(
        step(&mut owner, &nudging, &at_rest(30), Some(asked("a1"))),
        Step::Reply(asked("a1"))
    );
    assert_eq!(step(&mut owner, &nudging, &at_rest(80), None), Step::Wait);
    // Its dialog was answered: the same.
    let mut owner = Owner::new(&nudging);
    let dialog = Seen {
        dialog: true,
        busy: true,
        ..at_rest(30)
    };
    assert_eq!(step(&mut owner, &nudging, &dialog, None), Step::Dialog);
    assert_eq!(step(&mut owner, &nudging, &at_rest(80), None), Step::Wait);
}

#[test]
fn a_conversation_goes_on_in_turn_each_message_once_the_chief_is_done_with_the_last() {
    let steps = vec![
        FollowUp::Say("one".to_owned()),
        FollowUp::Switch,
        FollowUp::Say("two".to_owned()),
    ];
    let scenario = scenario(None, steps);
    let mut owner = Owner::new(&scenario);
    assert_eq!(step(&mut owner, &scenario, &at_rest(20), None), Step::Wait);
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                busy: true,
                ..at_rest(100)
            },
            None
        ),
        Step::Wait
    );
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(21), None),
        Step::Next(FollowUp::Say("one".to_owned()))
    );
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(21), None),
        Step::Next(FollowUp::Switch)
    );
    // A switch is no message the owner typed.
    assert_eq!(owner.follow_ups_sent, 1);
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(21), None),
        Step::Next(FollowUp::Say("two".to_owned()))
    );
    assert_eq!(owner.follow_ups_sent, 2);
    // None left: the run is over after the quiet time, not before.
    assert_eq!(step(&mut owner, &scenario, &at_rest(90), None), Step::Wait);
    assert_eq!(step(&mut owner, &scenario, &at_rest(91), None), Step::Over);
}

#[test]
fn the_run_is_not_over_while_a_message_is_left_to_say_or_the_chief_is_at_work() {
    let scenario = scenario(None, vec![FollowUp::Say("one".to_owned())]);
    let mut owner = Owner::new(&scenario);
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                busy: true,
                ..at_rest(1000)
            },
            None
        ),
        Step::Wait
    );
    // A follow-up is said first, however long it has been quiet.
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(1000), None),
        Step::Next(FollowUp::Say("one".to_owned()))
    );
    assert_eq!(
        step(
            &mut owner,
            &scenario,
            &Seen {
                busy: true,
                ..at_rest(1000)
            },
            None
        ),
        Step::Wait
    );
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(1000), None),
        Step::Over
    );
}

#[test]
fn time_is_up_when_the_run_has_gone_on_too_long_even_with_the_chief_at_work_and_quiet_wins() {
    let scenario = scenario(None, Vec::new());
    let mut owner = Owner::new(&scenario);
    let long = Seen {
        elapsed: TIMEOUT + Duration::from_secs(1),
        busy: true,
        ..at_rest(0)
    };
    assert_eq!(step(&mut owner, &scenario, &long, None), Step::TimeUp);
    let not_yet = Seen {
        elapsed: TIMEOUT,
        ..long
    };
    assert_eq!(step(&mut owner, &scenario, &not_yet, None), Step::Wait);
    // Over and out of time at once: it is over.
    let both = Seen {
        busy: false,
        ..at_rest(100)
    };
    let late = Seen {
        elapsed: TIMEOUT + Duration::from_secs(5),
        ..both
    };
    assert_eq!(step(&mut owner, &scenario, &late, None), Step::Over);
}

#[test]
fn the_dialog_comes_before_the_terminal_and_the_terminal_before_the_nudge_and_the_follow_ups() {
    let scenario = scenario(Some("Continuă."), vec![FollowUp::Say("one".to_owned())]);
    let mut owner = Owner::new(&scenario);
    let busy_dialog = Seen {
        dialog: true,
        busy: true,
        ..at_rest(70)
    };
    assert_eq!(
        step(&mut owner, &scenario, &busy_dialog, Some(asked("a1"))),
        Step::Dialog
    );
    // At rest with a question open and a nudge and a message waiting: the answer first.
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(70), Some(asked("a1"))),
        Step::Reply(asked("a1"))
    );
    // The nudge is for a chief that was answered by no one; this one was, so the message goes.
    assert_eq!(
        step(&mut owner, &scenario, &at_rest(70), None),
        Step::Next(FollowUp::Say("one".to_owned()))
    );
}
