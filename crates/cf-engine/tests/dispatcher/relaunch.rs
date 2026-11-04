//! The relaunch of a window that never began its brief (T-1130's mend, kept by T-1199), as
//! the critical review T-1220 asked it made safe: it is bound to the very message that
//! stalled and to the window it stalled in, it is asked again before the window is retired
//! and after every awaited host call, it resumes only the stall that asked it, it gives the
//! window that exact delivery again and once, it is told as it happened, and it is tried once
//! for a delivery. A tell or a pause takes the task out of its hands. Where any of that fails,
//! the window is kept as any window that ignores a pause, and nothing is relaunched.

use cf_engine::testing::{Context, Gate};
use cf_harness::records::Role;
use serde_json::json;

use crate::brief_never_began::{a_follow_up_whose_window_never_begins, PAST_ARRIVAL};
use crate::fixtures::{given, last_launch, last_message, native_of, now, Tiers};

/// A follow-up, T-2, whose window (read in flight) never began its brief, stalled: paused by
/// ConsensFlow, its requester told. The brief's message.
fn stalled(context: &Context, tiers: &Tiers) -> i64 {
    let brief = a_follow_up_whose_window_never_begins(context, tiers, false);
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "paused", "stalled");
    brief
}

/// The rounds of the stall's stop, and a look past them.
fn rounds(context: &Context) {
    for _ in 0..6 {
        context.advance(5_000);
        context.pass().unwrap();
    }
}

/// The notes the chief has that say a window was relaunched or opened afresh.
fn relaunch_notes(tiers: &Tiers) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("and gave it m-"))
        .collect()
}

/// The window was kept, never retired, as any window that ignores a pause: no kill since
/// `killed`, the board shows it did not stop, and no relaunch was told.
fn kept(context: &Context, tiers: &Tiers, killed: usize) {
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert!(context
        .dispatcher
        .unstopped(tiers.id(&tiers.assignee(2)))
        .is_some());
    assert_eq!(relaunch_notes(tiers), Vec::<String>::new());
}

#[test]
fn a_tell_after_the_stall_takes_the_task_out_of_its_relaunch() {
    // T-1220's P1-1: a tell records a stop as any tell does, and the stall's relaunch is the
    // task's no more.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    context.tell(
        tiers.project.id,
        &tiers.assignee(2),
        2,
        "Use the v2 grammar",
    );
    rounds(&context);
    kept(&context, &tiers, killed);
    assert_eq!(tiers.task(2).task.state, "paused");
}

#[test]
fn a_pause_after_the_stall_takes_the_task_out_of_its_relaunch() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    context.pause_task(tiers.project.id, 2);
    rounds(&context);
    kept(&context, &tiers, killed);
}

#[test]
fn a_window_that_began_its_turn_after_the_stall_is_kept_and_never_relaunched() {
    // T-1220's P1-2, its first probe: the first words of the window's answer after the stall.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    let at = now(&context);
    context.adapter.writes("zeus", "Starting on the lexer", &at);
    rounds(&context);
    kept(&context, &tiers, killed);
}

#[test]
fn a_window_holding_the_humans_unsent_words_is_kept_and_never_relaunched() {
    // T-1220's P1-2, its second probe.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    context.host.set_snapshot(
        json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "unsent": true }),
    );
    rounds(&context);
    kept(&context, &tiers, killed);
}

#[test]
fn a_window_whose_screen_cannot_be_read_is_kept_and_never_relaunched() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    // The host's look at its screen fails.
    context
        .host
        .set_snapshot(json!({ "ok": false, "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    rounds(&context);
    kept(&context, &tiers, killed);
}

#[test]
fn a_relaunch_whose_task_was_paused_again_while_its_window_closed_resumes_nothing() {
    // T-1220's P1-3: the host's answer to the kill is late; meanwhile the chief resumes T-2
    // and the human pauses it again. Once the kill is answered, the newer pause stays.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let pane = context.host.last("zeus").unwrap().pane;
    let held = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((pane.generation, held.clone()));
    context.host.exits_showing(Some(0), None, None);
    // The pass that asks the kill waits for the host's answer: it is the one that does not
    // end once everything else has.
    let mut stepping = None;
    for _ in 0..10 {
        context.advance(5_000);
        let pass = context.begin_pass();
        context.settle();
        match pass.take() {
            Some(done) => done.unwrap(),
            None => {
                stepping = Some(pass);
                break;
            }
        }
    }
    let stepping = stepping.expect("the kill is asked");
    context.resume_task(tiers.project.id, 2, "Carry on");
    context.human_pause(tiers.project.id, 2);
    held.open();
    context.settle();
    stepping.take().unwrap().unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.task(2).task.state,
        "paused",
        "the human's pause stays"
    );
    assert!(relaunch_notes(&tiers)
        .iter()
        .all(|note| !note.contains("goes on by itself")));
}

/// T-1, done; reopened with words its window began no turn on, stalled. The reopen's words.
fn a_reopen_that_stalls(context: &Context, tiers: &Tiers) {
    context.host.exits_showing(Some(0), None, None);
    tiers.open_body("Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser written");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context.adapter.never_begins.set(Some(false));
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "ONLY_FIX_README")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused", "stalled");
}

#[test]
fn a_relaunch_gives_the_window_the_very_words_that_stalled_a_reopens_and_not_the_brief() {
    // T-1220's P2-5, its first probe.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    a_reopen_that_stalls(&context, &tiers);
    context.adapter.never_begins.set(None);
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working", "it goes on by itself");
    let last = given(&context, "zeus").pop().unwrap();
    assert!(last.contains("ONLY_FIX_README"), "{last}");
    assert!(!last.contains("Write the parser"), "{last}");
}

#[test]
fn a_relaunch_on_a_conversation_its_harness_lost_gives_the_brief_once_and_says_it_opened_afresh() {
    // T-1220's P2-5, its second probe.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    context.adapter.never_begins.set(None);
    context.adapter.holds_nothing("zeus");
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let (_, resume) = last_launch(&context);
    let message = last_message(&context);
    assert_eq!(resume, None, "opened afresh");
    assert_eq!(message.matches("Now the lexer").count(), 1, "{message}");
    let told = relaunch_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains("opened it afresh") && !told[0].contains("on its conversation"),
        "{}",
        told[0]
    );
}

#[test]
fn a_relaunch_of_a_brief_as_long_as_a_message_may_be_gives_it_again_whole_and_goes_on() {
    // T-1220's P2-6: a brief of 999,990 characters, the recovery's words no part of it.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let project = tiers.project.id;
    context.host.exits_showing(Some(0), None, None);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    let body = "x".repeat(999_990);
    context.create_task(
        project,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: body.clone(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.adapter.never_begins.set(Some(false));
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "paused", "stalled");
    let before = given(&context, "zeus")
        .into_iter()
        .rev()
        .find(|text| text.contains("· T-2 · task"))
        .expect("its paste");
    context.adapter.never_begins.set(None);
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "working", "it goes on by itself");
    // The very message again, as it went in the first time: the relaunched window's first.
    assert_eq!(last_message(&context), before, "the same delivery, whole");
    let _ = &body;
    let told = relaunch_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:#?}");
}

#[test]
fn a_delivery_that_stalls_again_after_its_relaunch_is_kept_and_the_human_told_what_to_do() {
    // T-1220's P3-7: one relaunch for a delivery.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let killed = context.host.killed().len();
    assert_eq!(relaunch_notes(&tiers).len(), 1, "relaunched once");
    // The window, relaunched, begins nothing on it again: stalled again, it is kept.
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(2).task.state, "paused", "stalled again");
    for _ in 0..30 {
        context.advance(5_000);
        context.pass().unwrap();
    }
    assert_eq!(context.host.killed().len(), killed, "not relaunched twice");
    assert_eq!(relaunch_notes(&tiers).len(), 1);
    let human: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("did not stop"))
        .collect();
    assert_eq!(human.len(), 1, "{human:#?}");
    assert!(
        human[0].contains("relaunched it once") && human[0].contains("delete @"),
        "{}",
        human[0]
    );
}

#[test]
fn a_relaunched_window_is_told_on_its_conversation_once_the_window_is_back() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let brief = stalled(&context, &tiers);
    let before = given(&context, "zeus")
        .into_iter()
        .rev()
        .find(|text| text.contains(&format!("m-{brief} · T-2 · task")))
        .expect("its paste");
    let session = tiers.assignee(2);
    let native = native_of(&context, tiers.id(&session));
    context.adapter.never_begins.set(None);
    context.host.exits_showing(Some(0), None, None);
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(native_of(&context, tiers.id(&session)), native);
    let told = relaunch_notes(&tiers);
    assert_eq!(
        told,
        [format!(
            "T-2's window (@{session}) began no turn on m-{brief} and went on through the interrupt 3 times with nothing of its own running, so ConsensFlow relaunched it on its conversation and gave it m-{brief} again. T-2 goes on by itself."
        )]
    );
    // m-{brief} went in again, as it was, the relaunched window's first message.
    assert_eq!(last_message(&context), before);
    let _ = Role::User;
}

#[test]
fn a_resume_after_the_stall_moves_its_delivery_and_nothing_is_relaunched() {
    // A changed delivery revokes the relaunch: the chief resumes T-2 after the stall, and the
    // task is no longer the stall's to give back.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    context.resume_task(tiers.project.id, 2, "Begin it now");
    rounds(&context);
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
}

#[test]
fn a_window_that_began_while_the_host_showed_its_screen_is_kept_and_never_relaunched() {
    // Asked again after every awaited host call (T-1220's P1-2): the window's first words
    // come while the host answers the look at its screen that comes before the retirement.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    let adapter = std::rc::Rc::clone(&context.adapter);
    let host = std::rc::Rc::clone(&context.host);
    *context.host.on_request.borrow_mut() = Some(std::rc::Rc::new(
        move |op: &str, body: &serde_json::Value| {
            // The relaunch's own look at the screen asks for no tail.
            if op == "pane.snapshot" && body.get("tail").is_none() {
                // What begins prints: the host's answer says so, of the screen as it is now.
                host.set_snapshot(json!({ "outputQuietMs": 0, "screenQuietMs": 0 }));
                let item = adapter.item(Role::Assistant, "Starting on the lexer");
                adapter.with("zeus", |agent| {
                    if !agent
                        .items
                        .iter()
                        .any(|known| &*known.text == "Starting on the lexer")
                    {
                        agent.items.push(item);
                    }
                });
            }
        },
    ));
    rounds(&context);
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
}

/// `change` is made of zeus's window (its record, through the fake adapter, or its screen,
/// through the fake host) as the relaunch reads its record afresh: the look that comes after
/// the round's own ask of what the window runs (`left_running`), whatever the order of the
/// relaunch's other steps. Made as the read is asked, it is in what the read answers; and
/// the host's answers after it say it.
fn during_the_fresh_read(
    context: &Context,
    change: impl Fn(&cf_engine::testing::FakeAdapter, &cf_engine::testing::FakeHost) + 'static,
) {
    let background = std::rc::Rc::clone(&context.background);
    let adapter = std::rc::Rc::clone(&context.adapter);
    let host = std::rc::Rc::clone(&context.host);
    let seen = std::cell::Cell::new(background.left_asked().len());
    let made = std::cell::Cell::new(false);
    let _ = context.adapter.observe_holds.hold(move |_| {
        let asked = background.left_asked().len();
        let fresh = asked > seen.get();
        seen.set(asked);
        if fresh && !made.get() {
            made.set(true);
            change(&adapter, &host);
        }
        false
    });
}

#[test]
fn a_window_whose_record_cannot_be_read_afresh_is_kept_and_never_relaunched() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    during_the_fresh_read(&context, |adapter, _| adapter.unread("zeus", true));
    rounds(&context);
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
}

#[test]
fn a_window_showing_a_dialog_of_its_own_when_read_afresh_is_kept_and_never_relaunched() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    during_the_fresh_read(&context, |adapter, _| {
        adapter.with("zeus", |agent| {
            agent.waiting = Some(cf_harness::contract::Waiting {
                reason: Some("Allow this command?".to_owned()),
            });
        });
    });
    rounds(&context);
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
}

/// The relaunch asked of a stalled window whose `change` comes as said: kept, never retired.
fn kept_through(context: &Context, tiers: &Tiers, change: impl FnOnce(&Context)) {
    stalled(context, tiers);
    let killed = context.host.killed().len();
    change(context);
    rounds(context);
    kept(context, tiers, killed);
}

#[test]
fn a_job_begun_while_the_host_shows_the_screen_keeps_the_window() {
    // T-1223's P1-1, its first probe: the harness starts a job of its own as the host answers
    // the relaunch's look at the screen. A job begun prints its line: the screen says so.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        let adapter = std::rc::Rc::clone(&context.adapter);
        let host = std::rc::Rc::clone(&context.host);
        *context.host.on_request.borrow_mut() = Some(std::rc::Rc::new(
            move |op: &str, body: &serde_json::Value| {
                if op == "pane.snapshot" && body.get("tail").is_none() {
                    adapter.with("zeus", |agent| agent.pending.jobs = 1);
                    host.set_snapshot(json!({ "outputQuietMs": 0, "screenQuietMs": 0 }));
                }
            },
        ));
    });
}

#[test]
fn a_job_begun_while_its_record_is_read_afresh_keeps_the_window() {
    // The job is in the record the relaunch reads afresh (`pending.jobs`): work a kill ends.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |adapter, _| {
            adapter.with("zeus", |agent| agent.pending.jobs = 1);
        });
    });
}

#[test]
fn the_humans_words_typed_while_its_record_is_read_afresh_keep_the_window() {
    // T-1223's P1-1, its second probe: the screen held none of the human's words unsent until
    // the record's fresh read, and holds some once it is answered.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |_, host| {
            host.set_snapshot(
                json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "unsent": true }),
            );
        });
    });
}

#[test]
fn a_window_that_printed_while_its_record_was_read_afresh_is_kept() {
    // The screen is the last look before the kill: it has printed nothing since the record's
    // fresh read was asked, or the window is kept.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |_, host| {
            host.set_snapshot(json!({ "outputQuietMs": 0, "screenQuietMs": 0 }));
        });
    });
}

#[test]
fn a_key_the_human_pressed_while_its_record_was_read_afresh_keeps_the_window() {
    // Any key of the human's since the record's fresh read was asked, sent or not.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |_, host| {
            host.set_snapshot(json!({
                "outputQuietMs": 600_000,
                "screenQuietMs": 600_000,
                "humanWrites": [{ "agoMs": 0, "length": 1, "classes": ["control"] }],
            }));
        });
    });
}

#[test]
fn a_paste_on_its_way_into_the_window_keeps_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |_, host| {
            host.set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000, "pasteInFlight": true }));
        });
    });
}

#[test]
fn a_screen_that_says_nothing_of_its_output_keeps_the_window() {
    // No evidence the window printed nothing since: none to retire it on.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    kept_through(&context, &tiers, |context| {
        during_the_fresh_read(context, |_, host| host.set_snapshot(json!({})));
    });
}

/// zeus's record's items of the user's: their ids, in order.
fn users_items(context: &Context) -> Vec<String> {
    context
        .adapter
        .agent("zeus")
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.id.to_string())
        .collect()
}

#[test]
fn a_replay_into_a_conversation_that_kept_its_first_copy_is_received_as_the_new_one() {
    // T-1223's P2-2, its first probe: the conversation keeps the stalled copy (`i-3`) through
    // the relaunch, and the replay goes in after it (`i-5`). The receipt names the replay: what
    // reads the window's turns from it reads them from there (result-after-held-turn's
    // `paste_of`).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.adapter.resumes_records.set(true);
    let brief = stalled(&context, &tiers);
    let old = context.message(brief).receipt["item"]
        .as_str()
        .expect("received once")
        .to_owned();
    context.adapter.never_begins.set(None);
    context.host.exits_showing(Some(0), None, None);
    rounds(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let users = users_items(&context);
    assert!(users.contains(&old), "the first copy kept: {users:?}");
    let replay = context.message(brief);
    assert_eq!(replay.state, "delivered");
    let new = replay.receipt["item"].as_str().expect("received again");
    assert_ne!(new, old, "the replay's own receipt");
    assert_eq!(
        users.iter().rposition(|id| id == new),
        users.iter().rposition(|id| *id == old).map(|at| at + 1),
        "the copy after the first: {users:?}"
    );
    assert_eq!(relaunch_notes(&tiers).len(), 1);
}

#[test]
fn a_replay_the_reopened_harness_drops_is_not_received_and_nothing_says_it_went_in() {
    // T-1223's P2-2, its second probe: the reopened harness takes nothing of its first
    // message; only the stalled copy is in its record. The replay is no arrival: not delivered,
    // the task not at work, and no note says it was given again.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.adapter.resumes_records.set(true);
    let brief = stalled(&context, &tiers);
    context.adapter.never_begins.set(None);
    context.host.exits_showing(Some(0), None, None);
    let kills = context.host.killed().len();
    context.adapter.forgets_first_messages.set(true);
    rounds(&context);
    assert!(context.host.killed().len() > kills, "relaunched");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_ne!(context.message(brief).state, "delivered");
    assert_ne!(tiers.task(2).task.state, "working");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
}

#[test]
fn a_pause_while_the_host_shows_the_screen_keeps_the_window() {
    // After the last look, before the kill, every check of the ledger is asked again: the
    // human's pause that comes as the host answers the screen keeps the window, never retired;
    // its own stop is asked of it afresh.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    stalled(&context, &tiers);
    let killed = context.host.killed().len();
    let ledger = std::rc::Rc::clone(&context.ledger);
    let project = tiers.project.id;
    *context.host.on_request.borrow_mut() = Some(std::rc::Rc::new(
        move |op: &str, body: &serde_json::Value| {
            if op == "pane.snapshot" && body.get("tail").is_none() {
                let _ = ledger
                    .borrow_mut()
                    .pause_task(project, 2, Some("human"), None);
            }
        },
    ));
    rounds(&context);
    assert_eq!(context.host.killed().len(), killed, "kept, never retired");
    assert_eq!(relaunch_notes(&tiers), Vec::<String>::new());
    assert_eq!(tiers.task(2).task.state, "paused");
}

/// T-1229's probe: the conversation keeps the stalled copy through the relaunch, and the
/// reopened harness takes nothing of its first message, so the replay is on its way and in no
/// record but as the old copy. The brief's message and the old copy's item.
fn replay_dropped(context: &Context, tiers: &Tiers) -> (i64, String) {
    context.adapter.resumes_records.set(true);
    let brief = stalled(context, tiers);
    let old = context.message(brief).receipt["item"]
        .as_str()
        .expect("received once")
        .to_owned();
    context.adapter.never_begins.set(None);
    context.host.exits_showing(Some(0), None, None);
    context.adapter.forgets_first_messages.set(true);
    let kills = context.host.killed().len();
    for _ in 0..8 {
        context.advance(5_000);
        context.pass().unwrap();
        if context.host.killed().len() > kills && context.message(brief).state == "delivering" {
            break;
        }
    }
    assert_eq!(
        context.message(brief).state,
        "delivering",
        "the replay on its way"
    );
    assert_eq!(context.message(brief).receipt["before"], old.as_str());
    (brief, old)
}

/// The replay was not taken for the old copy: not delivered by it, the task not at work.
fn not_taken_for_the_old_copy(context: &Context, tiers: &Tiers, brief: i64, old: &str) {
    let message = context.message(brief);
    assert_ne!(message.receipt["item"], old, "{:?}", message.receipt);
    assert_ne!(message.state, "delivered", "{:?}", message.receipt);
    assert_ne!(tiers.task(2).task.state, "working");
}

/// m-`brief`'s copies in zeus's record after the old copy `old`, where the record holds it
/// (a conversation kept), else all (one opened afresh).
fn given_after(context: &Context, brief: i64, old: &str) -> usize {
    let marker = format!("m-{brief} · T-2 · task");
    let items = context.adapter.agent("zeus").items;
    let from = items
        .iter()
        .rposition(|item| &*item.id == old)
        .map_or(0, |at| at + 1);
    items[from..]
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(&marker))
        .count()
}

/// m-`brief`'s copies among zeus's record's items of the user's.
fn copies(context: &Context, brief: i64) -> usize {
    given(context, "zeus")
        .iter()
        .filter(|text| text.contains(&format!("m-{brief} · T-2 · task")))
        .count()
}

#[test]
fn a_restart_takes_no_replay_for_the_copy_that_never_began_and_the_resume_gives_it_once() {
    // T-1229's probe, the restart (`settle_after_restart`): the old copy is in the
    // conversation's copied transcript and in its harness's own record, before the boundary.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replay_dropped(&context, &tiers);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    not_taken_for_the_old_copy(&context, &tiers, brief, &old);
    assert_eq!(
        context.message(brief).state,
        "queued",
        "back for another try"
    );
    context.adapter.forgets_first_messages.set(false);
    after.resume_project(tiers.project.id).unwrap();
    for _ in 0..6 {
        after.pass().unwrap();
        context.advance(3_000);
    }
    let message = context.message(brief);
    assert_eq!(message.state, "delivered");
    assert_ne!(
        message.receipt["item"],
        old.as_str(),
        "the new copy's receipt"
    );
    assert_eq!(
        given_after(&context, brief, &old),
        1,
        "given once after the old copy"
    );
}

#[test]
fn a_projects_close_takes_no_replay_for_the_copy_that_never_began_and_the_resume_gives_it_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replay_dropped(&context, &tiers);
    context.close_project(tiers.project.id).unwrap();
    context.settle();
    not_taken_for_the_old_copy(&context, &tiers, brief, &old);
    assert_eq!(
        context.message(brief).state,
        "queued",
        "back for the Resume"
    );
    context.adapter.forgets_first_messages.set(false);
    context.resume_project(tiers.project.id).unwrap();
    for _ in 0..6 {
        context.pass().unwrap();
        context.advance(3_000);
    }
    let message = context.message(brief);
    assert_eq!(message.state, "delivered");
    assert_ne!(
        message.receipt["item"],
        old.as_str(),
        "the new copy's receipt"
    );
    assert_eq!(
        given_after(&context, brief, &old),
        1,
        "given once after the old copy"
    );
}

#[test]
fn a_window_that_exits_with_its_relaunch_on_its_way_fails_that_launch_and_takes_no_old_copy() {
    // A control: a member's launch whose window ends by itself in an open project fails its
    // first message, reading no record (`settle_gone`, T-1076), so the old copy proves nothing.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replay_dropped(&context, &tiers);
    context.exit("zeus");
    context.settle();
    not_taken_for_the_old_copy(&context, &tiers, brief, &old);
}

/// The stalled brief taken back to be given again while its window is still up (the ledger's
/// `relaunch_stalled`, which the engine asks only once the window is retired): the next pass
/// pastes it into that window, an ordinary paste, which the record shows after the old copy.
/// What reads its arrival before any look confirms it is the reader under test: the
/// brief's message and the old copy's item.
fn replayed_into_the_open_window(context: &Context, tiers: &Tiers) -> (i64, String) {
    let brief = stalled(context, tiers);
    let old = context.message(brief).receipt["item"]
        .as_str()
        .expect("received once")
        .to_owned();
    let session = tiers.assignee(2);
    let participant = tiers.id(&session);
    let stop = context
        .ledger
        .borrow()
        .stop_of(participant)
        .unwrap()
        .expect("the stall's stop");
    context
        .ledger
        .borrow_mut()
        .relaunch_stalled(tiers.project.id, 2, participant, brief, stop.seq, &old)
        .unwrap();
    context.adapter.never_begins.set(None);
    // The window comes to rest, so a paste goes in.
    context.adapter.answer("zeus", "Ready.");
    for _ in 0..6 {
        context.pass().unwrap();
        if copies(context, brief) == 2 {
            break;
        }
        context.advance(3_000);
    }
    assert_eq!(copies(context, brief), 2, "pasted after the old copy");
    assert_eq!(
        context.message(brief).state,
        "delivering",
        "no look confirmed it yet"
    );
    (brief, old)
}

/// The replay that went in was received by its own copy, the one after the old.
fn received_by_the_new_copy(context: &Context, brief: i64, old: &str) {
    let message = context.message(brief);
    assert_eq!(message.state, "delivered", "{:?}", message.receipt);
    let new = message.receipt["item"].as_str().expect("its receipt");
    assert_ne!(new, old, "the new copy's receipt, not the old one's");
}

#[test]
fn a_window_that_exits_with_its_replay_pasted_receives_it_by_the_new_copy() {
    // The window ends before a look confirmed the paste: its end reads the copied transcript,
    // then its harness's own record (`in_flight::arrived`), after the boundary.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replayed_into_the_open_window(&context, &tiers);
    context.exit("zeus");
    context.settle();
    received_by_the_new_copy(&context, brief, &old);
}

#[test]
fn a_restart_with_a_replay_pasted_receives_it_by_the_new_copy() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replayed_into_the_open_window(&context, &tiers);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    received_by_the_new_copy(&context, brief, &old);
}

#[test]
fn a_switched_window_receives_its_replay_by_the_new_copy_in_the_record_it_left() {
    // The paste waits in its harness's queue (Devin's, behind a turn) when the human switches
    // the window to another conversation; the harness runs it in the conversation it left,
    // whose record holds the old copy before it (`watch_left`).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let (brief, old) = replayed_into_the_open_window(&context, &tiers);
    let marker = format!("m-{brief} · T-2 · task");
    let queued = context.adapter.with("zeus", |agent| {
        let at = agent
            .items
            .iter()
            .rposition(|item| item.role == Role::User && item.text.contains(&marker))
            .expect("the replay");
        agent.items.remove(at)
    });
    let left = native_of(&context, tiers.id(&tiers.assignee(2))).expect("its conversation");
    context.adapter.switch_to("zeus", "native-elsewhere");
    context.pass().unwrap();
    context.settle();
    assert_eq!(
        context.message(brief).state,
        "delivering",
        "not in the look before the switch"
    );
    context.adapter.with("zeus", |agent| {
        agent
            .records
            .get_mut(&left)
            .expect("the record it left")
            .push(queued)
    });
    for _ in 0..3 {
        context.pass().unwrap();
        context.settle();
    }
    received_by_the_new_copy(&context, brief, &old);
}
