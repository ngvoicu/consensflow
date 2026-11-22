//! A window whose turn is over has nothing for an interrupt to stop, and is never
//! closed for keys nothing in it could heed (T-551). On 2026-10-25, 3.0.6-alpha.2:
//! an account's weekly limit refused seven Claude windows, ConsensFlow held each
//! task, and pressed Escape into T-502's window, whose look did not read it at
//! rest; three rounds later it closed the window, and the close ended the plant run
//! and the test run it had left in the background (m-3943 to m-3945, two notes of
//! the one close among them). At ~21:05 a `cf tell` of T-604 stopped a turn whose
//! three background agents still ran: Claude says busy while they run, the
//! interrupted turn's record counts none of them, and the window was closed as one
//! that ignored the interrupt (m-4188).
//!
//! The cases are those shapes, and the three ways a task whose turn ended is paused:
//! a refusal for quota, the chief's pause and the chief's tell. In each the window
//! stays open with what it left running, nobody is told it did not stop, and the
//! words that send the task on go into the same window once it rests.

use cf_engine::testing::Context;
#[cfg(target_os = "macos")]
use cf_engine::testing::Made;
use std::rc::Rc;

use cf_harness::contract::{Held, Over, Pane, Readiness, Waiting};
use cf_harness::records::{Pending, Role};
use cf_proto::panes::{Background, BackgroundProcess};
use serde_json::{json, Value};

use crate::fixtures::{after, exhausted, now, Tiers};

/// What pressing Escape into `pane` is, as the host is asked it.
fn escape(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bytes": [27] })
}

/// A project with zeus at work on T-1, its brief received.
fn working(context: &Context) -> i64 {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project.id
}

/// Every note of the project that says a window did not stop, or that a close
/// ended what a window left running, whoever it went to.
fn closes_told(context: &Context, project: i64) -> Vec<String> {
    ["human", "chief"]
        .into_iter()
        .flat_map(|who| context.inbox(context.id(project, who)))
        .filter(|message| message.kind == "note")
        .map(|message| message.body)
        .filter(|body| body.contains("did not stop") || body.contains("its close ended"))
        .collect()
}

/// How many messages of the window `handle` say `words`.
fn given(context: &Context, handle: &str, words: &str) -> usize {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

/// The look reads `handle`'s window at rest again: Claude's status went idle.
fn rests(context: &Context, handle: &str) {
    context.adapter.with(handle, |agent| agent.settled = true);
}

/// Ten passes, three seconds and more apart: every round a stop could have.
fn rounds(context: &Context) {
    for _ in 0..10 {
        context.pass().unwrap();
        context.advance(3_100);
    }
}

#[test]
fn an_accounts_weekly_limit_closes_none_of_its_windows_and_each_goes_on_in_its_own_once_quota_is_back(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana", "juno"]);
    for body in ["Parser", "Lexer", "Printer"] {
        tiers.open_body(body);
    }
    context.pass().unwrap();
    context.pass().unwrap();
    let sessions: Vec<String> = (1..=3).map(|number| tiers.assignee(number)).collect();
    let launches = context.adapter.prepared().len();
    let resets_at = after(&context, 46 * 3_600_000);
    let limit = exhausted(Some(&now(&context)), Some(&resets_at));
    // The weekly limit ends two windows' turns, read at rest on the refusal.
    context.adapter.refuse(&sessions[1], limit.clone());
    context.adapter.refuse(&sessions[2], limit.clone());
    // T-502's: its turn ends on the refusal with a command it left running in the
    // background, and its look does not read it at rest.
    context.adapter.refuse_unsettled(&sessions[0], limit);
    context.adapter.with(&sessions[0], |agent| {
        agent.pending = Pending {
            jobs: 1,
            ..Pending::default()
        };
    });
    rounds(&context);
    for number in 1..=3 {
        assert_eq!(tiers.task(number).task.state, "paused", "T-{number} held");
    }
    assert_eq!(
        context.host.inputs(),
        Vec::<Value>::new(),
        "no key: nothing in a refused turn is left to stop"
    );
    assert!(
        context.host.killed().is_empty(),
        "every window waits with its work"
    );
    assert_eq!(
        closes_told(&context, tiers.project.id),
        Vec::<String>::new()
    );
    let waits = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains(" waits with @"))
        .count();
    assert_eq!(waits, 3, "each requester is told its task waits, once");

    // Quota is back: each task goes on in its own window, which reads at rest again.
    for member in ["zeus", "diana", "juno"] {
        context
            .back_from_quota(tiers.project.id, member)
            .expect("back");
    }
    rests(&context, &sessions[0]);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    for (number, session) in (1..=3).zip(&sessions) {
        assert_eq!(tiers.task(number).task.state, "working", "T-{number}");
        assert_eq!(
            given(&context, session, "Resumed: Go on where you stopped"),
            1,
            "{session}"
        );
    }
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "no window opened again"
    );
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
}

#[test]
fn a_tell_whose_escape_stopped_a_turn_with_background_agents_keeps_the_window_and_goes_in_once_it_rests(
) {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    // T-604's: its turn started agents in the background and works on; the interrupt
    // ends the turn in its record, and Claude stays busy while the agents run.
    context.adapter.busy_with_agents_until_interrupted("zeus");
    context.tell(project, "zeus", 1, "Drop the per-member cap");
    rounds(&context);
    let pane = context.host.last("zeus").expect("its window").pane;
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "one Escape, which stopped the turn"
    );
    assert!(
        context.host.killed().is_empty(),
        "the turn stopped: no close"
    );
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
    assert_eq!(context.task(project, 1).task.state, "paused");

    // The agents end: the window rests, and the words go into it.
    rests(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Drop the per-member cap"), 1);
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    assert_eq!(context.host.inputs().len(), 1, "no key with them");
}

#[test]
fn a_tell_whose_escape_stopped_the_turn_is_paid_though_the_window_began_turns_of_its_own_before_its_rest_stood(
) {
    // T-807, 2026-10-27, Claude Code 2.1.311 (the live suite's tell case): the Escape
    // stopped the turn, and Claude's record said so in a message of its own ("[Request
    // interrupted by user for tool use]") while its status stayed busy for the agents. A
    // second later an agent's notice began a turn of Claude's own, before the record's
    // word had stood the moment it counts after, and every look read the window at work:
    // the stop pressed into the turn the window began itself (two Escapes, +0.0 s and
    // +3.4 s), and with notices coming as fast, round three and the close that ends the
    // agents.
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    let zeus = context.id(project, "zeus");
    context.adapter.busy("zeus");
    context.tell(project, "zeus", 1, "Drop the per-member cap");
    context.pass().unwrap();
    let pane = context.host.last("zeus").expect("its window").pane;
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the turn at work: one Escape"
    );
    // The interrupt ends the turn in the record, in the harness's own words, and the
    // agents' notices begin turns of the window's own, one a round.
    context.adapter.begins_its_own_turn(
        "zeus",
        "[Request interrupted by user for tool use]",
        &now(&context),
    );
    for _ in 0..4 {
        context.adapter.begins_its_own_turn(
            "zeus",
            "<task-notification> Agent \"sleep one\" is waiting on its own background work",
            &now(&context),
        );
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "no key into the turns the window began itself"
    );
    assert!(
        context.host.killed().is_empty(),
        "the turn stopped: no close"
    );
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
    assert_eq!(context.task(project, 1).task.state, "paused");

    // Its own turns end: the window rests, and the words go into it.
    rests(&context, "zeus");
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Drop the per-member cap"), 1);
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    assert_eq!(context.host.inputs().len(), 1, "no key with them");
}

#[test]
fn a_pause_whose_escape_stopped_a_turn_with_background_agents_keeps_the_window_and_its_resume_goes_in(
) {
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    context.adapter.busy_with_agents_until_interrupted("zeus");
    context.pause_task(project, 1);
    rounds(&context);
    assert_eq!(
        context.host.inputs().len(),
        1,
        "one Escape, which stopped the turn"
    );
    assert!(context.host.killed().is_empty());
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    context.resume_task(project, 1, "Go on");
    rests(&context, "zeus");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Resumed: Go on"), 1);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
}

/// Notices to the chief about the resume words, excluding the separate notice that a
/// window ignored its interrupt rounds.
fn resume_wait_notices(context: &Context, project: i64) -> Vec<String> {
    context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.body.contains("resume words")
                && message.body.contains("wait")
        })
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_resume_still_waiting_two_minutes_after_its_stop_was_paid_is_told_to_the_chief_once() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");

    // The turn ignores the rounds. Time before the stop is paid does not start
    // the two-minute wait for the distinct resume notice.
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    context.advance(120_000);
    context.pass().unwrap();
    assert!(resume_wait_notices(&context, project).is_empty());

    // The turn's record now says over, paying the stop, but its status still reads
    // at work; there is no recorded count of background agents.
    context
        .adapter
        .with("zeus", |agent| agent.over = Over::Held);
    context.pass().unwrap();
    let zeus = context.id(project, "zeus");
    assert_eq!(
        context.dispatcher.resume_wait(zeus).unwrap(),
        Some(json!({ "task": 1, "reason": "window shows working" }))
    );
    context.advance(119_999);
    context.pass().unwrap();
    assert!(resume_wait_notices(&context, project).is_empty());
    context.advance(1);
    context.pass().unwrap();

    let notices = resume_wait_notices(&context, project);
    assert_eq!(notices.len(), 1, "the chief is told once after two minutes");
    assert!(notices[0].contains("T-1"), "{}", notices[0]);
    assert!(notices[0].contains("@zeus"), "{}", notices[0]);
    assert!(notices[0].contains("window"), "{}", notices[0]);
    assert!(notices[0].contains("working"), "{}", notices[0]);
    assert!(notices[0].contains("when checked"), "{}", notices[0]);
    assert!(!notices[0].contains("background agent"), "{}", notices[0]);
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 0);

    context.advance(120_000);
    context.pass().unwrap();
    assert_eq!(resume_wait_notices(&context, project), notices);
    rests(&context, "zeus");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 1);
    assert_eq!(context.dispatcher.resume_wait(zeus).unwrap(), None);
}

#[test]
fn an_ordinary_pause_and_resume_does_not_tell_the_chief_the_resume_is_waiting() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    for _ in 0..4 {
        context.pass().unwrap();
        context.advance(3_100);
    }
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 1);
    assert_eq!(
        context
            .dispatcher
            .resume_wait(context.id(project, "zeus"))
            .unwrap(),
        None
    );
    context.advance(120_000);
    context.pass().unwrap();
    assert!(resume_wait_notices(&context, project).is_empty());
}

#[test]
fn a_resume_waiting_on_the_members_question_names_the_dialog_not_the_human() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context
        .adapter
        .busy_with_agents_until_interrupted_in_generation("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    context.pass().unwrap();
    context.pass().unwrap();

    // A member's own question is answered through the board, not necessarily
    // by the human. Its dialog holds the resume words at the existing gate.
    context.adapter.with("zeus", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("its own question dialog is open".to_owned()),
        });
    });
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.resume_wait(zeus).unwrap(),
        Some(json!({ "task": 1, "reason": "window's dialog waits for an answer" }))
    );
    context.advance(120_000);
    context.pass().unwrap();
    let notices = resume_wait_notices(&context, project);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("dialog waits for an answer"));
    assert!(!notices[0].contains("the human"));
}

#[test]
fn a_resume_waiting_for_human_action_says_so_without_quoting_its_window() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context.adapter.busy_until_interrupted("zeus");
    *context.adapter.ready.borrow_mut() = Some(Rc::new(|| {
        Ok(Readiness::Held(Held::ForHuman(
            "a private detail in its input box".to_owned(),
        )))
    }));
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.dispatcher.resume_wait(zeus).unwrap(),
        Some(json!({ "task": 1, "reason": "window needs the human" }))
    );
    context.advance(120_000);
    context.pass().unwrap();
    let notices = resume_wait_notices(&context, project);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("window needs the human"));
    assert!(!notices[0].contains("private detail"));
}

#[test]
fn a_resume_long_after_a_paid_pause_gets_a_chance_to_arrive_before_any_wait_notice() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    context.advance(10 * 60_000);
    context.pass().unwrap();
    context.resume_task(project, 1, "Go on with the parser");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 1);
    assert!(resume_wait_notices(&context, project).is_empty());
}

#[test]
fn a_resume_sent_late_in_the_two_minutes_is_told_when_the_paid_stops_clock_reaches_them() {
    let context = Context::new();
    let project = working(&context);
    context
        .adapter
        .busy_with_agents_until_interrupted_in_generation("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    context.advance(119_000);
    context.resume_task(project, 1, "Go on with the parser");
    context.pass().unwrap();
    assert!(resume_wait_notices(&context, project).is_empty());
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(resume_wait_notices(&context, project).len(), 1);
}

#[test]
fn resume_words_still_waiting_when_the_member_goes_out_tell_the_chief_the_quota_reason() {
    let context = Context::new();
    let project = working(&context);
    let zeus = context.id(project, "zeus");
    context
        .adapter
        .busy_with_agents_until_interrupted_in_generation("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, "2099-01-01T00:00:00.000Z", "out of quota")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.resume_wait(zeus).unwrap(),
        Some(json!({ "task": 1, "reason": "member out of quota" }))
    );
    context.advance(120_000);
    context.pass().unwrap();
    let notices = resume_wait_notices(&context, project);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("out of quota"), "{}", notices[0]);
}

#[test]
fn a_later_pause_and_resume_has_its_own_single_wait_notice() {
    let context = Context::new();
    let project = working(&context);
    for words in ["First direction", "Second direction"] {
        context
            .adapter
            .busy_with_agents_until_interrupted_in_generation("zeus");
        context.pause_task(project, 1);
        context.resume_task(project, 1, words);
        context.pass().unwrap();
        context.pass().unwrap();
        context.advance(120_000);
        context.pass().unwrap();
        rests(&context, "zeus");
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(given(&context, "zeus", &format!("Resumed: {words}")), 1);
    }
    let notices = resume_wait_notices(&context, project);
    assert_eq!(notices.len(), 2);
    assert_ne!(notices[0], notices[1], "each names its own resume message");
    context.advance(120_000);
    context.pass().unwrap();
    assert_eq!(resume_wait_notices(&context, project), notices);
}

#[test]
fn the_existing_long_hold_notice_does_not_tell_the_chief_the_same_resume_wait_twice() {
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy_until_interrupted("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    let words = context.task(project, 1).messages.last().unwrap().id;
    *context.adapter.ready.borrow_mut() = Some(std::rc::Rc::new(|| {
        Ok(cf_harness::contract::Readiness::Held(
            cf_harness::contract::Held::Unsent,
        ))
    }));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let chief = context.id(project, "chief");
    let notices = || {
        context
            .inbox(chief)
            .into_iter()
            .filter(|message| {
                message.kind == "note"
                    && message.body.contains(&format!("m-{words}"))
                    && (message.body.contains("resume words")
                        || message.body.contains("has waited"))
            })
            .map(|message| message.body)
            .collect::<Vec<_>>()
    };
    context.advance(120_000);
    context.pass().unwrap();
    assert_eq!(notices().len(), 1, "the two-minute notice");
    context.advance(9 * 60_000);
    context.pass().unwrap();
    assert_eq!(notices().len(), 1, "the chief is told once for these words");
    let human = context.id(project, "human");
    let human_notices = context
        .inbox(human)
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.body.contains(&format!("m-{words}"))
                && message.body.contains("has waited")
        })
        .count();
    assert_eq!(human_notices, 1, "the existing human notice still goes");
}

#[test]
fn a_record_that_says_the_turn_is_over_only_a_moment_ago_is_pressed_no_more_and_paid_once_it_stands(
) {
    // Claude says busy a moment before the message of a turn it begins is in its
    // record, so the record's word that a turn is over counts once it has stood a
    // moment (Calliope on T-551, T-644). Until then the stop neither presses again
    // nor is paid, however late the next look comes.
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    context.pause_task(project, 1);
    context.pass().unwrap();
    assert_eq!(
        context.host.inputs().len(),
        1,
        "the turn at work: one Escape"
    );
    context
        .adapter
        .with("zeus", |agent| agent.over = Over::Pending);
    for _ in 0..3 {
        context.advance(10_000);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.inputs().len(),
        1,
        "no second key while it stands"
    );
    assert!(context.host.killed().is_empty());
    let zeus = context.id(project, "zeus");
    context
        .adapter
        .with("zeus", |agent| agent.over = Over::Held);
    context.pass().unwrap();
    context.resume_task(project, 1, "Go on");
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.pass().unwrap();
    assert_eq!(
        given(&context, "zeus", "Resumed: Go on"),
        1,
        "paid, and resumed in it"
    );
    assert_eq!(context.host.inputs().len(), 1);
    assert_eq!(context.dispatcher.unstopped(zeus), None);
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
}

/// zeus's turn on T-1 ended and left a job running in the background: the task
/// goes on in the background, held for it.
fn held_for_its_job(context: &Context) -> i64 {
    let project = working(context);
    context
        .adapter
        .answer_leaving_a_job("zeus", "The plants run; I report when they end");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "working", "held");
    project
}

#[test]
fn a_pause_of_a_task_held_for_its_job_presses_no_key_and_its_resume_goes_into_the_same_window() {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    context.pause_task(project, 1);
    rounds(&context);
    assert_eq!(context.task(project, 1).task.state, "paused");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    context.resume_task(project, 1, "Go on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given(&context, "zeus", "Resumed: Go on"), 1);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.adapter.prepared().len(), launches);
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
}

#[test]
fn a_tell_of_a_task_held_for_its_job_presses_no_key_and_its_words_go_into_the_same_window() {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    context.tell(project, "zeus", 1, "Run the daemon table too");
    rounds(&context);
    assert_eq!(given(&context, "zeus", "Run the daemon table too"), 1);
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());
    assert_eq!(context.adapter.prepared().len(), launches);
}

#[test]
fn a_task_held_for_its_job_whose_next_turn_is_refused_presses_no_key_and_goes_on_there_at_the_reset(
) {
    let context = Context::new();
    let project = held_for_its_job(&context);
    let launches = context.adapter.prepared().len();
    // The job ends, the window takes a turn for it, and the limit refuses that turn;
    // a second job still runs, and the look does not read the window at rest.
    context.adapter.wake(
        "zeus",
        "<task-notification><status>completed</status></task-notification>",
    );
    context.pass().unwrap();
    let resets_at = after(&context, 2 * 3_600_000);
    context
        .adapter
        .refuse_unsettled("zeus", exhausted(Some(&now(&context)), Some(&resets_at)));
    context.adapter.with("zeus", |agent| {
        agent.pending = Pending {
            jobs: 1,
            ..Pending::default()
        };
    });
    rounds(&context);
    assert_eq!(context.task(project, 1).task.state, "paused", "held");
    assert_eq!(context.host.inputs(), Vec::<Value>::new());
    assert!(context.host.killed().is_empty(), "its job runs on");
    assert_eq!(closes_told(&context, project), Vec::<String>::new());

    // The reset: the words go into the same window once it reads at rest.
    context.advance(2 * 3_600_000);
    rests(&context, "zeus");
    context.adapter.quota("zeus", None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        given(&context, "zeus", "Resumed: Go on where you stopped"),
        1
    );
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(context.adapter.prepared().len(), launches);
}

#[test]
fn a_window_kept_for_a_stop_it_ignored_is_told_once_to_the_human_who_gave_its_task() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.human_pause(project.id, 1);
    rounds(&context);
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty(), "a hung turn is kept");
    assert_eq!(
        closes_told(&context, project.id),
        [KEPT[0]],
        "one note, in the human's words, and none to the chief"
    );
}

/// What the human and the chief read of a window kept for a stop it ignored, running
/// nothing ConsensFlow can see, its task still paused.
const KEPT: [&str; 2] = [
    "@zeus did not stop for T-1: its turn went on through the interrupt 3 times, and ConsensFlow can see nothing it runs: the host lists no work of its turn, and its harness no job or agent, so its harness may be hung (in a model request, or a tool it does not name). ConsensFlow keeps the window open and T-1 paused, closes nothing, and interrupts the turn again every minute. To end it now, delete @zeus's session.",
    "T-1's window (@zeus) did not stop: its turn went on through the interrupt, and ConsensFlow can see nothing it runs (the host lists no work of its turn, its harness no job or agent): its harness may be hung. ConsensFlow keeps the window open and T-1 paused, closes nothing, and interrupts the turn again every minute; until then what it writes is not taken as T-1's result. `cf task resume T-1 \"…\"` gives the window your words once its turn ends; only the human can end it sooner, by deleting its session.",
];

#[test]
fn the_human_whose_pause_kept_a_chief_given_tasks_window_is_told_too_once() {
    // The chief gave the task and hears that the window is kept; the human paused it, and
    // is told too: only the human can end it (the chief's call on T-644, T-1199).
    let context = Context::new();
    let project = working(&context);
    context.adapter.busy("zeus");
    context.human_pause(project, 1);
    rounds(&context);
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty(), "a hung turn is kept");
    assert_eq!(closes_told(&context, project), KEPT, "one note to each");
}

#[test]
fn the_chief_whose_tell_kept_a_human_given_tasks_window_is_told_too_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "human".to_owned(),
            to: Some("zeus".to_owned()),
            body: "Parser".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    context.tell(project.id, "zeus", 1, "Use the v2 grammar");
    rounds(&context);
    context.host.exits_showing(Some(0), None, None);
    context.pass().unwrap();
    assert!(context.host.killed().is_empty(), "a hung turn is kept");
    assert_eq!(closes_told(&context, project.id), KEPT, "one note to each");
}

/// A project with zeus at work on T-1, a pause and a resume of T-1 sent after the
/// stop's one Escape ended the turn in its record while its status stayed busy for a
/// background agent (Claude Code, T-604): the engine's third pass has read the end
/// stand its second.
fn paused_and_resumed_with_an_agent_running(context: &Context, project: i64) -> Pane {
    // The host still sees work from this window. Like Claude's interrupted turn, the
    // stand-in's record reads over but has no pending-agent count, while its live status
    // remains busy. Nothing else ends the work or makes it idle.
    context.background.leaves(Some(Background {
        total: 1,
        processes: vec![BackgroundProcess {
            pid: 4711,
            command: "background-agent".to_owned(),
        }],
        ..Background::default()
    }));
    context.adapter.busy_with_agents_until_interrupted("zeus");
    context.pause_task(project, 1);
    context.resume_task(project, 1, "Go on with the parser");
    context.pass().unwrap();
    let pane = context.host.last("zeus").expect("its window").pane;
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "the first turn was interrupted"
    );
    pane
}

#[cfg(target_os = "macos")]
#[test]
fn an_immediate_resume_reaches_the_stopped_window_while_its_background_agent_runs() {
    // N23, on the Mac (Claude Code 2.1.329, T-1857's recording): after the interrupt for
    // a tool use the prompt takes a typed line though the status says busy for the agent.
    let context = Context::new();
    let project = working(&context);
    let launches = context.adapter.prepared().len();
    let pane = paused_and_resumed_with_an_agent_running(&context, project);
    for _ in 0..4 {
        context.advance(3_100);
        context.pass().unwrap();
    }

    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "no later pause round"
    );
    assert!(
        context.host.killed().is_empty(),
        "background work was not ended"
    );
    assert_eq!(
        given(&context, "zeus", "Resumed: Go on with the parser"),
        1,
        "the resume must reach the same window while its background work still runs"
    );
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the same window"
    );
    for _ in 0..4 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(
        context.host.inputs(),
        [escape(&pane)],
        "no later pause round cuts the resumed turn"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_paced_resume_goes_into_the_stopped_window_once_the_settle_has_stood_and_a_fresh_look_reads_it_so(
) {
    // The pacing's gate reads the same word as the step's: the rest holds over looks of
    // one frontier for the settle, and the fresh look before the paste must read the
    // window taking words too (it is not settled: its status says busy for the agent).
    let context = Context::made(Made {
        settle_ms: 3_000,
        ..Made::default()
    });
    let project = working(&context);
    let pane = paused_and_resumed_with_an_agent_running(&context, project);
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 0);
    for _ in 0..6 {
        context.advance(1_100);
        context.pass().unwrap();
    }
    assert_eq!(
        given(&context, "zeus", "Resumed: Go on with the parser"),
        1,
        "once the settle stood"
    );
    assert_eq!(context.host.inputs(), [escape(&pane)], "no more keys");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn an_immediate_resume_waits_where_the_window_was_never_recorded_taking_words_after_the_interrupt()
{
    // Windows (and every platform nothing recorded): the words wait as they did, and
    // nothing is pressed or ended meanwhile.
    let context = Context::new();
    let project = working(&context);
    let pane = paused_and_resumed_with_an_agent_running(&context, project);
    for _ in 0..4 {
        context.advance(3_100);
        context.pass().unwrap();
    }
    assert_eq!(context.host.inputs(), [escape(&pane)]);
    assert!(context.host.killed().is_empty());
    assert_eq!(given(&context, "zeus", "Resumed: Go on with the parser"), 0);
}

#[test]
fn a_window_that_takes_words_for_its_background_agent_has_no_result_taken_while_it_runs() {
    // The prompt takes words (the Mac, after the interrupt for a tool use), but the
    // turn's answer is not final while the agent runs: `settled` decides a result, and it
    // stays false.
    let context = Context::new();
    let project = working(&context);
    context.adapter.answer("zeus", "Parser done.");
    context.adapter.with("zeus", |agent| {
        agent.settled = false;
        agent.over = Over::Held;
        agent.busy_when_interrupted = true;
    });
    rounds(&context);
    assert_eq!(context.task(project, 1).task.state, "working");
    assert_eq!(
        context.inbox(context.id(project, "chief")).len(),
        0,
        "nothing of it reached the chief"
    );
}
