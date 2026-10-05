//! Switching the chief to another agent, where the project is deleted
//! while the switch waits (for the old chief, for its turn, for its last
//! look or for its window to close) and another is created meanwhile: the
//! switch says its project is gone, and nothing of the new one is switched,
//! asked, copied or closed.

use cf_engine::testing::Gate;
use cf_engine::SwitchWhen;
use cf_harness::records::Role;

use crate::chiefs::{chief_of, replace_project, to, with_codex};
use crate::traces::held_to;

const SUITES: &[&str] = &["switching the chief to another agent"];

#[test]
fn switches_nothing_once_its_project_is_deleted_while_the_switch_waits_for_the_old_chief_nor_a_project_created_meanwhile(
) {
    let context = with_codex();
    let driver = context.driver();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief takes a paste its harness holds; the switch waits for it.
    let held = Gate::default();
    *context.adapter.hold_deliveries.borrow_mut() = Some(held.clone());
    context.note(old.id, Some("zeus"), "chief", "Held");
    context.pass().unwrap();
    // After the turn, as the page asks by default, and asking where things stand.
    let switched =
        driver.begin_switch_chief(old.id, to("codex", "astraeus"), SwitchWhen::Turn, true);
    let replaced = replace_project(&driver, &old);
    let chief = chief_of(&context, replaced.fresh.id);
    let window = context.host.last("chief").unwrap();
    held.open();
    context.settle();
    replaced.gone();
    let inbox = context.ledger.borrow().inbox(chief.id, 100).unwrap();
    assert!(inbox.is_empty(), "the new chief is asked nothing");
    assert_eq!(
        context.dispatcher.pending_switch(chief.id),
        None,
        "nothing waits to switch it"
    );
    let now = chief_of(&context, replaced.fresh.id);
    assert_eq!(
        (now.harness.as_deref(), context.codex.prepared().len()),
        (Some("claude-code"), 0),
        "the new chief is not switched"
    );
    assert!(
        !context
            .host
            .killed()
            .iter()
            .any(|pane| pane.generation == window.pane.generation),
        "and its window stays"
    );
    assert_eq!(
        switched.answer().unwrap().unwrap_err().to_string(),
        format!("no project {}", old.id),
        "the switch says its project is gone"
    );
    drop(driver);
    held_to(
        context.close(),
        SUITES,
        "switches nothing once its project is deleted while the switch waits for the old chief, nor a project created meanwhile",
    );
}

#[test]
fn hands_nothing_to_an_old_chief_whose_switch_waited_for_its_turns_end_once_its_project_is_deleted_nor_switches_a_project_created_meanwhile(
) {
    let context = with_codex();
    let driver = context.driver();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.busy("chief");
    driver
        .switch_chief(old.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();
    let old_chief = chief_of(&context, old.id).id;
    let asked = context
        .ledger
        .borrow()
        .inbox(old_chief, 100)
        .unwrap()
        .into_iter()
        .find(|message| message.body.contains("Write down where things stand"))
        .unwrap();
    assert_eq!(asked.state, "queued", "the note waits for the turn to end");
    // The turn ends, and the next look at the old window waits on its harness.
    context.adapter.answer("chief", "Done with that");
    let launch = context.adapter.agent("chief").launch;
    let held = Gate::default();
    *context.adapter.hold_observes.borrow_mut() = Some((launch, held.clone()));
    let looking = driver.begin_pass();
    let replaced = replace_project(&driver, &old);
    let welcome = context.note(replaced.fresh.id, None, "chief", "Welcome");
    assert_ne!(
        welcome.id, asked.id,
        "the ledger never gives the note's id again"
    );
    held.open();
    context.settle();
    looking.answer().unwrap().unwrap();
    replaced.gone();
    assert_eq!(
        context.message(welcome.id).state,
        "queued",
        "it waits for the new chief"
    );
    let now = chief_of(&context, replaced.fresh.id);
    assert_eq!(
        (now.harness.as_deref(), context.codex.prepared().len()),
        (Some("claude-code"), 0),
        "the new chief is not switched"
    );
    drop(driver);
    held_to(
        context.close(),
        SUITES,
        "hands nothing to an old chief whose switch waited for its turn's end once its project is deleted, nor switches a project created meanwhile",
    );
}

#[test]
fn copies_and_confirms_nothing_of_the_old_chiefs_last_look_once_its_project_is_deleted_nor_switches_a_project_created_meanwhile(
) {
    let context = with_codex();
    let driver = context.driver();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // A note is on its way to the old chief, and the human told it something
    // since the last look: the switch's last look, which shows both, waits
    // on its harness.
    context.note(old.id, Some("zeus"), "chief", "Parser done");
    context.pass().unwrap();
    let told = "The codeword is tern";
    let said = context.adapter.item(Role::User, told);
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
    let launch = context.adapter.agent("chief").launch;
    let held = Gate::default();
    *context.adapter.hold_observes.borrow_mut() = Some((launch, held.clone()));
    let switched =
        driver.begin_switch_chief(old.id, to("codex", "astraeus"), SwitchWhen::Now, false);
    context.settle();
    let replaced = replace_project(&driver, &old);
    held.open();
    context.settle();
    replaced.gone();
    let chief = chief_of(&context, replaced.fresh.id).id;
    let before: Vec<_> = context
        .ledger
        .borrow()
        .chief_history(replaced.fresh.id)
        .unwrap()
        .into_iter()
        .flat_map(|conversation| conversation.items)
        .collect();
    assert_eq!(
        (
            context
                .ledger
                .borrow()
                .copied_item_with(chief, told)
                .unwrap(),
            before.iter().filter(|copied| copied.text == told).count()
        ),
        (None, 0),
        "the new chief's conversations, now or before, have nothing of the old window"
    );
    let now = chief_of(&context, replaced.fresh.id);
    assert_eq!(
        (now.harness.as_deref(), context.codex.prepared().len()),
        (Some("claude-code"), 0),
        "the new chief is not switched"
    );
    assert_eq!(
        switched.answer().unwrap().unwrap_err().to_string(),
        format!("no project {}", old.id),
        "the switch says its project is gone, and confirmed none of its messages"
    );
    drop(driver);
    held_to(
        context.close(),
        SUITES,
        "copies and confirms nothing of the old chief's last look once its project is deleted, nor switches a project created meanwhile",
    );
}

#[test]
fn switches_nothing_once_its_project_is_deleted_while_the_old_chiefs_window_closes_nor_a_project_created_meanwhile(
) {
    let context = with_codex();
    let driver = context.driver();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief's window takes its time to close.
    let window = context.host.last("chief").unwrap();
    let held = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((window.pane.generation, held.clone()));
    let switched =
        driver.begin_switch_chief(old.id, to("codex", "astraeus"), SwitchWhen::Now, false);
    context.settle();
    let replaced = replace_project(&driver, &old);
    held.open();
    context.settle();
    replaced.gone();
    let now = chief_of(&context, replaced.fresh.id);
    assert_eq!(
        (now.harness.as_deref(), context.codex.prepared().len()),
        (Some("claude-code"), 0),
        "the new chief is not switched"
    );
    assert_eq!(
        switched.answer().unwrap().unwrap_err().to_string(),
        format!("no project {}", old.id),
        "the switch says its project is gone"
    );
    drop(driver);
    held_to(
        context.close(),
        SUITES,
        "switches nothing once its project is deleted while the old chief's window closes, nor a project created meanwhile",
    );
}

#[test]
fn switches_nothing_once_its_project_is_deleted_while_the_old_chiefs_window_closes_after_its_turn_nor_a_project_created_meanwhile(
) {
    let context = with_codex();
    let driver = context.driver();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The chief is at work, so the switch waits for the end of its turn.
    context.adapter.busy("chief");
    driver
        .switch_chief(old.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    // The turn ends, and the step that switches the chief closes its old
    // window, which takes its time.
    context.adapter.answer("chief", "Done with that");
    let window = context.host.last("chief").unwrap();
    let held = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((window.pane.generation, held.clone()));
    let stepping = driver.begin_pass();
    context.settle();
    let replaced = replace_project(&driver, &old);
    held.open();
    // The step stops there, and fails nothing: the deleted project's chief
    // has nothing left to switch.
    context.settle();
    stepping.answer().unwrap().unwrap();
    replaced.gone();
    let chief = chief_of(&context, replaced.fresh.id);
    assert_eq!(
        (
            chief.harness.as_deref(),
            context.codex.prepared().len(),
            context.dispatcher.pending_switch(chief.id)
        ),
        (Some("claude-code"), 0, None),
        "the new chief is not switched, and nothing waits to switch it"
    );
    drop(driver);
    held_to(
        context.close(),
        SUITES,
        "switches nothing once its project is deleted while the old chief's window closes after its turn, nor a project created meanwhile",
    );
}
