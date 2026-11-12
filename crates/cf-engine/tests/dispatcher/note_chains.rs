//! A chain of ConsensFlow's notes is one message (RS-25; T-1410 counted 43% of the chief's
//! typed notes in chains, one turn a note, the largest 44 in 170 seconds): the notes that
//! wait for the chief together when its window can take the first go in one paste, a line
//! each. Each stays a row of its own, proved by the paste.

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use cf_ledger::NewNote;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::ledger::MessageView;

use crate::chiefs::{to, with_codex};
use crate::fixtures::{after, exhausted, given, now, Tiers};

/// ConsensFlow's own note to the chief.
fn told(context: &Context, project: i64, task: Option<i64>, body: &str) -> MessageView {
    context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                task,
                body: body.to_owned(),
            },
        )
        .unwrap()
}

#[test]
fn the_notes_that_wait_for_a_chief_at_work_go_into_its_window_as_one_message() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is in a turn as one event writes three notes for it.
    context.adapter.busy("chief");
    let before = given(&context, "chief").len();
    let notes = [
        told(&context, tiers.project.id, Some(1), "T-1 needs a look."),
        told(&context, tiers.project.id, Some(2), "T-2 needs a look."),
        told(
            &context,
            tiers.project.id,
            None,
            "The staff's rules changed.",
        ),
    ];
    context.pass().unwrap();
    context.adapter.answer("chief", "Done.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let pasted: Vec<String> = given(&context, "chief").split_off(before);
    assert_eq!(pasted.len(), 1, "one message: {pasted:?}");
    let first = notes[0].id;
    assert!(
        pasted[0].contains(&format!("[ConsensFlow m-{first} · note from ConsensFlow]\n3 notes at once, a line each:\nm-{first} · T-1: T-1 needs a look.\n")),
        "{}",
        pasted[0]
    );
    for note in &notes {
        assert_eq!(context.message(note.id).state, "delivered", "m-{}", note.id);
    }
}

#[test]
fn a_member_out_of_quota_with_seven_tasks_is_one_message_to_the_chief() {
    // RS-25: the first note of a hold is typed, once for each task; seven written together
    // while the chief works go in one paste, a line each.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    for n in 1..=7 {
        tiers.open_body(&format!("Part {n}"));
    }
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!((1..=7).all(|n| tiers.task(n).task.state == "working"));
    context.adapter.busy("chief");
    let before = given(&context, "chief").len();
    let resets_at = after(&context, 120 * 3_600_000);
    for n in 1..=7 {
        context.adapter.refuse(
            &tiers.assignee(n),
            exhausted(Some(&now(&context)), Some(&resets_at)),
        );
    }
    context.pass().unwrap();
    let states: Vec<String> = (1..=7).map(|n| tiers.task(n).task.state).collect();
    assert!(states.iter().all(|state| state == "paused"), "{states:?}");
    context.adapter.answer("chief", "Done.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let pasted: Vec<String> = given(&context, "chief").split_off(before);
    assert_eq!(pasted.len(), 1, "one message: {pasted:?}");
    assert!(
        pasted[0].contains("7 notes at once, a line each:"),
        "{}",
        pasted[0]
    );
    for n in 1..=7 {
        assert!(
            pasted[0].contains(&format!(
                "T-{n}: T-{n} waits with @{}: out of quota until {resets_at}",
                tiers.assignee(n)
            )),
            "{}",
            pasted[0]
        );
        // Each task's line says its hold, typed as it was.
        assert_eq!(
            tiers.task(n).task.aside,
            Some(format!(
                "waits with @{}: out of quota until {resets_at}",
                tiers.assignee(n)
            ))
        );
    }
}

#[test]
fn a_chief_switched_while_a_task_holds_is_told_the_hold_in_its_handoff() {
    // The first note of a hold was typed to the old chief; the new chief starts a new
    // conversation, and the same hold said again is only shown: its handoff names the
    // task and the hold (T-1500's finding 2).
    let context = with_codex();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("Part 1");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let zeus = tiers.assignee(1);
    let resets_at = after(&context, 120 * 3_600_000);
    context
        .adapter
        .refuse(&zeus, exhausted(Some(&now(&context)), Some(&resets_at)));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    let hold = format!("waits with @{zeus}: out of quota until {resets_at}");
    assert!(
        given(&context, "chief")
            .iter()
            .any(|text| text.contains(&format!("T-1 {hold}"))),
        "the old chief was typed the hold: {:?}",
        given(&context, "chief")
    );

    context
        .switch_chief(
            tiers.project.id,
            to("codex", "astraeus"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    let prepared = context.codex.prepared().last().cloned().unwrap();
    let handoff = prepared["message"].as_str().unwrap();
    assert!(
        handoff.contains(&format!(
            "What waits on you now:\n- T-1 \"{}\" {hold} (cf task get T-1)",
            tiers.task(1).task.title
        )),
        "{handoff}"
    );
    assert!(
        !handoff.contains("Nothing on the board waits on you."),
        "{handoff}"
    );
}

#[test]
fn a_chain_is_gathered_by_what_its_paste_says_and_every_note_is_typed_once_in_order() {
    // T-1500's finding 1: the notes' own words fit a chain (10,000), but each line adds its id
    // and task, and each command its `cf` by its whole path: a paste over what a window is
    // given whole is cut to its opening, and every note it carried was marked delivered.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    *context.roles.cf.borrow_mut() = CfWord::new(
        "/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf",
        Shell::Posix,
    );
    context.adapter.busy("chief");
    let before = given(&context, "chief").len();
    let mut notes = Vec::new();
    for n in 0..300 {
        let body = match n {
            100 => "Two lines:\nthe second one".to_owned(),
            200 => "A crab: \u{1F980} cf task get T-1.".to_owned(),
            _ => format!("Look {n}: cf task get T-1."),
        };
        notes.push(told(&context, tiers.project.id, Some(1), &body).id);
    }
    let words: usize = notes
        .iter()
        .map(|id| context.message(*id).body.chars().count())
        .sum();
    assert!(
        words < 10_000,
        "the notes' own words fit one chain: {words}"
    );
    context.pass().unwrap();
    for _ in 0..20 {
        context.adapter.answer("chief", "Done.");
        context.pass().unwrap();
        context.pass().unwrap();
    }
    let pasted: Vec<String> = given(&context, "chief").split_off(before);
    for paste in &pasted {
        assert!(
            !paste.contains("read all of it with"),
            "a paste was cut: {}",
            &paste[paste.len().saturating_sub(300)..]
        );
    }
    assert!(pasted.len() > 1, "{} pastes", pasted.len());
    // Every note is typed once, in the order written.
    let lines: Vec<i64> = pasted
        .iter()
        .flat_map(|paste| paste.lines())
        .filter_map(|line| {
            let rest = line.strip_prefix("m-")?;
            let (id, rest) = rest.split_once(' ')?;
            rest.starts_with("· T-1: ").then(|| id.parse().ok())?
        })
        .chain(pasted.iter().filter_map(|paste| {
            // A paste of one note alone says it under its header.
            let rest = paste.strip_prefix("[ConsensFlow m-")?;
            let (id, rest) = rest.split_once(' ')?;
            (!rest.contains("notes at once")).then(|| id.parse().ok())?
        }))
        .collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, notes, "each note once");
    assert_eq!(lines, notes, "in order");
    for id in &notes {
        assert_eq!(context.message(*id).state, "delivered", "m-{id}");
    }
}

#[test]
fn a_chain_in_flight_as_the_chief_is_switched_is_one_message_kept_or_given_back_whole() {
    // T-1500's finding 5: a chain is one message across a switch too. Seen arrive by the
    // last look, every note it carried is delivered; gone before it landed, the head and
    // every note behind it are queued again, and the new chief is typed each once.
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let landed: Vec<i64> = ["One went.", "Two went.", "Three went."]
        .into_iter()
        .map(|body| told(&context, project.id, None, body).id)
        .collect();
    context.pass().unwrap();
    assert_eq!(context.message(landed[0]).state, "delivering");
    assert!(
        given(&context, "chief")
            .last()
            .unwrap()
            .contains("3 notes at once"),
        "one paste"
    );
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    for id in &landed {
        assert_eq!(context.message(*id).state, "delivered", "m-{id}");
    }

    context.pass().unwrap();
    context.codex.answer("chief", "Taken over");
    context.pass().unwrap();
    context.codex.with("chief", |agent| agent.arrive = false);
    let lost: Vec<i64> = ["Four went.", "Five went.", "Six went."]
        .into_iter()
        .map(|body| told(&context, project.id, None, body).id)
        .collect();
    context.pass().unwrap();
    assert_eq!(context.message(lost[0]).state, "delivering");
    // The same three notes as the first, in one pass: one paste, which never lands (the
    // fake keeps no item of it); none of the three begins a delivery of its own.
    let begun: Vec<i64> = context
        .ledger
        .borrow()
        .events(project.id, 0, 10_000)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "delivery.begun")
        .filter_map(|event| event.data["message"].as_i64())
        .filter(|id| lost.contains(id))
        .collect();
    assert_eq!(begun, [lost[0]]);
    let before = given(&context, "chief").len();
    context
        .switch_chief(
            project.id,
            to("claude-code", "apollo"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    for id in &lost {
        let row = context.message(*id);
        assert_eq!(
            (row.state.as_str(), row.attempts),
            ("queued", 0),
            "m-{id}: the window went before it could land"
        );
    }
    for _ in 0..6 {
        context.pass().unwrap();
        context.adapter.answer("chief", "Done.");
        context.pass().unwrap();
    }
    let pasted = given(&context, "chief").split_off(before).join("\n");
    assert!(
        pasted.contains("3 notes at once"),
        "one paste again: {pasted}"
    );
    for body in ["Four went.", "Five went.", "Six went."] {
        assert_eq!(pasted.matches(body).count(), 1, "{body} in: {pasted}");
    }
    for id in &lost {
        assert_eq!(context.message(*id).state, "delivered", "m-{id}");
    }
}

#[test]
fn a_chain_a_restart_sends_again_says_so_once_for_all_its_notes() {
    // T-1500's finding 5, at a restart: a chain whose window took it before its record showed
    // it goes again whole, under one header that says it may be there already.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let notes: Vec<i64> = ["One went.", "Two went.", "Three went."]
        .into_iter()
        .map(|body| told(&context, project.id, None, body).id)
        .collect();
    let before = given(&context, "chief").len();
    {
        let mut ledger = context.ledger.borrow_mut();
        let begun = ledger.begin_delivery(notes[0]).unwrap();
        let carried: Vec<i64> = begun.carried.iter().map(|row| row.id).collect();
        assert_eq!(carried, notes[1..]);
        ledger
            .retry_delivery(
                notes[0],
                "the daemon stopped before its record showed it, and its window took it",
                true,
            )
            .unwrap();
    }
    context.pass().unwrap();
    let pasted = given(&context, "chief").split_off(before);
    assert_eq!(pasted.len(), 1, "{pasted:?}");
    assert!(
        pasted[0].starts_with(&format!(
            "[ConsensFlow m-{} · note from ConsensFlow{}]\n3 notes at once",
            notes[0],
            cf_engine::delivery_text::SENT_AGAIN
        )),
        "{}",
        pasted[0]
    );
    for body in ["One went.", "Two went.", "Three went."] {
        assert_eq!(pasted[0].matches(body).count(), 1, "{body}");
    }
}

#[test]
fn the_notes_that_wait_for_a_project_resumed_after_a_restart_go_in_uncut_pastes() {
    // T-1500's finding 1 after a restart: the notes that waited for the project's Resume go
    // in chains measured with the window's own cf, none cut.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    *context.roles.cf.borrow_mut() = CfWord::new(
        "/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf",
        Shell::Posix,
    );
    let notes: Vec<i64> = (0..300)
        .map(|n| {
            told(
                &context,
                project.id,
                None,
                &format!("Look {n}: cf task get T-1."),
            )
            .id
        })
        .collect();
    let before = given(&context, "chief").len();
    after.resume_project(project.id).unwrap();
    for _ in 0..20 {
        after.pass().unwrap();
        context.adapter.answer("chief", "Done.");
        after.pass().unwrap();
    }
    let pasted = given(&context, "chief").split_off(before);
    assert!(!pasted.is_empty(), "the notes went in");
    for paste in &pasted {
        assert!(
            !paste.contains("read all of it with"),
            "a paste was cut: {}",
            &paste[paste.len().saturating_sub(300)..]
        );
    }
    for id in &notes {
        assert_eq!(context.message(*id).state, "delivered", "m-{id}");
    }
}
