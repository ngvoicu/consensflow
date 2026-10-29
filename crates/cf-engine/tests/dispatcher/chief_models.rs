//! A chief is a harness and a model (CHF-05, CHF-06): a reopen goes on with what the
//! harness's record says the conversation ran on last, a model the human switched to in
//! the window included, and the model picked for it where the record names none; a model
//! the harness moved to by itself is never taken. A model its harness no longer has starts
//! the chief on the harness's default, with one note to the human; where the harness has
//! none, the chief stays closed until the human switches it. Node's chief ran on a saved
//! agent, so none of these is held to a trace of Node's.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use cf_harness::contract::Used;
use serde_json::{json, Value};

use crate::chiefs::{chief_of, to, to_human};

fn used(model: &str, effort: Option<&str>, by_harness: bool) -> Used {
    Used {
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
        thinking: None,
        by_harness,
    }
}

fn agent(model: &str, effort: Option<&str>) -> Value {
    json!({ "model": model, "effort": effort, "thinking": null, "designer": false })
}

/// A project whose chief answered once, closed and resumed with its record saying `said`.
fn reopened_with(said: Option<Used>) -> (Context, i64) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context.adapter.with("chief", |chief| chief.used = said);
    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();
    (context, project.id)
}

#[test]
fn a_reopen_goes_on_with_the_model_and_effort_the_human_switched_to_in_the_window() {
    let (context, project) = reopened_with(Some(used("claude-sonnet-5-5", Some("low"), false)));
    let launches = context.adapter.prepared();
    let reopened = launches.last().unwrap();
    assert!(reopened["resume"].is_string(), "resumed: {reopened}");
    assert_eq!(reopened["agent"], agent("claude-sonnet-5-5", Some("low")));
    // Asked of the conversation it goes on with, beside what its last launch asked for.
    let asked = context.adapter.asked_used();
    assert_eq!(
        asked
            .last()
            .map(|(session, asked)| (session.as_str(), asked.as_deref())),
        Some((reopened["resume"].as_str().unwrap(), Some("claude-opus-5")))
    );
    // The model picked stays the human's pick (O6); the launch's is kept beside it.
    let chief = chief_of(&context, project);
    assert_eq!(
        (chief.model.as_deref(), chief.launched_model.as_deref()),
        (Some("claude-opus-5"), Some("claude-sonnet-5-5"))
    );
}

#[test]
fn a_record_that_names_no_model_reopens_on_the_model_picked_for_the_chief() {
    let (context, _) = reopened_with(None);
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(reopened["agent"], agent("claude-opus-5", None));
}

#[test]
fn a_model_the_harness_moved_to_by_itself_is_never_taken_and_the_last_launchs_is_asked_again() {
    let (context, project) = reopened_with(Some(used("claude-haiku-5-5", Some("high"), true)));
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(reopened["agent"], agent("claude-opus-5", None));
    assert_eq!(
        chief_of(&context, project).launched_model.as_deref(),
        Some("claude-opus-5")
    );
}

#[test]
fn a_fresh_chief_after_a_switch_opens_on_the_model_picked_and_reads_no_record() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    let asked = context.adapter.asked_used().len();
    context
        .switch_chief(
            project.id,
            to("claude-code", "diana"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    let launched = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launched["resume"], Value::Null);
    assert_eq!(launched["agent"], agent("gpt-5.6-luna", None));
    assert_eq!(
        context.adapter.asked_used().len(),
        asked,
        "a fresh conversation has no record"
    );
    assert!(launched["message"]
        .as_str()
        .unwrap()
        .contains("You are the chief now."));
}

/// A project whose chief's harness says `gone_words` of a model it does not have, and
/// starts on `default` where none is named.
fn harness_without_the_model(default: Result<Option<String>, String>) -> Context {
    let context = Context::new();
    *context.adapter.gone_words.borrow_mut() = Some("Unknown model".to_owned());
    *context.adapter.default.borrow_mut() = default;
    context
}

/// The chief's first window goes as it opens, its screen saying the model is gone.
fn opened_on_a_gone_model(context: &Context) -> i64 {
    context.host.exits_showing(
        Some(1),
        None,
        Some(&["Error: Unknown model: 'claude-opus-5'"]),
    );
    *context.host.exit_after_open.borrow_mut() = Some("chief".to_owned());
    let project = context.with_staff(&["zeus"]);
    context.host.exits_showing(None, None, None);
    project.id
}

#[test]
fn a_gone_model_starts_the_chief_on_its_harnesss_default_and_the_human_is_told_once() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = opened_on_a_gone_model(&context);
    assert_eq!(context.project(project).state, "open", "nothing closed");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(launches[0]["agent"], agent("claude-opus-5", None));
    assert_eq!(launches[1]["agent"], agent("claude-haiku-5-5", None));
    let chief = chief_of(&context, project);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-haiku-5-5"), Some("claude-opus-5"))
    );
    let told: Vec<String> = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .collect();
    assert_eq!(
        told,
        ["The chief's model, claude-opus-5, is no longer available in Claude Code: the chief starts on claude-haiku-5-5, Claude Code's default model, instead. Switch the chief to pick another."]
    );
    assert!(
        to_human(&context, project)
            .iter()
            .all(|body| !body.contains("could not start")),
        "one note, not the launch's failure"
    );
}

#[test]
fn a_reopen_never_asks_for_a_gone_model_again_though_the_record_still_names_it() {
    let context = harness_without_the_model(Ok(None));
    let project = opened_on_a_gone_model(&context);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    // Its record still says the gone model, as Claude Code's last real entry would.
    context.adapter.with("chief", |chief| {
        chief.used = Some(used("claude-opus-5", Some("high"), false))
    });
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(reopened["agent"], Value::Null, "the harness's own default");
    let told = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .count();
    assert_eq!(told, 1);
}

#[test]
fn a_gone_model_the_harness_has_no_default_for_keeps_the_chief_closed_until_a_switch() {
    let context = harness_without_the_model(Err(
        "Pi offers no model that is not a contributor model".to_owned(),
    ));
    let project = opened_on_a_gone_model(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.adapter.prepared().len(),
        1,
        "no launch on a contributor model"
    );
    let told: Vec<String> = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .collect();
    assert_eq!(
        told,
        ["The chief's model, claude-opus-5, is no longer available in Claude Code, and ConsensFlow found no other to start it on (Pi offers no model that is not a contributor model): switch the chief to another model."]
    );
    // The human switches it to another model: it opens on that one.
    context
        .switch_chief(project, to("claude-code", "diana"), SwitchWhen::Now, false)
        .unwrap();
    context.pass().unwrap();
    assert_eq!(chief_of(&context, project).gone_model, None);
    let launched = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launched["agent"], agent("gpt-5.6-luna", None));
}

#[test]
fn a_turn_that_fails_on_a_gone_model_closes_the_window_and_starts_the_chief_on_the_default() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // As Pi's provider answers a model it does not have: the turn fails in the record.
    context.adapter.with("chief", |chief| {
        chief.failed = true;
        chief.failure = Some("400 model_not_found: Unknown model 'claude-opus-5'".to_owned());
    });
    context.adapter.answer("chief", "…");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared();
    assert_eq!(
        launches.last().unwrap()["agent"],
        agent("claude-haiku-5-5", None)
    );
    assert_eq!(context.project(project.id).state, "open");
    assert!(to_human(&context, project.id)
        .iter()
        .all(|body| !body.contains("The chief's turn failed")));
}

#[test]
fn what_waits_for_a_chief_held_on_a_gone_model_waits_for_the_chief_switched_in() {
    let context = harness_without_the_model(Err("no model to start on".to_owned()));
    let project = opened_on_a_gone_model(&context);
    // The human's word to the chief, queued before its window opens again.
    let note = context.note_from_consensflow(project, "chief", "Look at T-1");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).state, "queued", "held, not lost");
    context
        .switch_chief(project, to("claude-code", "diana"), SwitchWhen::Now, false)
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_ne!(
        context.message(note.id).state,
        "queued",
        "given to the chief switched in"
    );
}

#[test]
fn a_chief_opened_again_on_its_harnesss_default_is_told_no_turn_was_cut() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |chief| {
        chief.failed = true;
        chief.failure = Some("400 model_not_found: Unknown model 'claude-opus-5'".to_owned());
    });
    context.adapter.answer("chief", "…");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let told: Vec<String> = context
        .adapter
        .prepared()
        .iter()
        .filter_map(|launch| launch["message"].as_str().map(str::to_owned))
        .collect();
    assert!(
        told.iter()
            .all(|message| !message.contains("ConsensFlow stopped in the middle of your turn")),
        "{told:?}"
    );
    let notes = to_human(&context, project.id);
    assert_eq!(
        notes
            .iter()
            .filter(|body| body.contains("no longer available"))
            .count(),
        1,
        "{notes:?}"
    );
}

#[test]
fn a_window_that_never_came_up_on_a_gone_model_starts_the_chief_on_the_default_at_once() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    // The window does not come up: its harness refuses the model it no longer has, in its
    // own words; the next window does.
    let first = Rc::new(std::cell::Cell::new(true));
    *context.adapter.started.borrow_mut() = Some(Rc::new(move || {
        if first.replace(false) {
            Err("Error: Unknown model: 'claude-opus-5'".to_owned())
        } else {
            Ok(())
        }
    }));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let launched = context.adapter.prepared();
    assert_eq!(
        launched.last().unwrap()["agent"],
        agent("claude-haiku-5-5", None),
        "at once, not a backoff later: {launched:?}"
    );
    let told = to_human(&context, project.id);
    assert!(
        told.iter().all(|body| !body.contains("could not start")),
        "{told:?}"
    );
    assert_eq!(
        told.iter()
            .filter(|body| body.contains("no longer available"))
            .count(),
        1
    );
}

/// A chief from before a chief was a harness and a model names a saved agent and no model
/// (CHF-04): the roster is never read for it, so one the human deleted holds it nowhere.
#[test]
fn a_chief_that_still_names_a_deleted_agent_opens_on_its_record_and_the_roster_is_never_asked() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute_batch(
            "UPDATE participant SET agent = 'apollo', model = NULL, launched_model = NULL
             WHERE role = 'chief'",
        )
        .unwrap();
    });
    context.roster.gone.borrow_mut().insert("apollo".to_owned());
    context.adapter.with("chief", |chief| {
        chief.used = Some(used("claude-sonnet-5-5", Some("low"), false))
    });
    let opened = context.host.opened().len();
    context.resume_project(project.id).unwrap();
    assert_eq!(context.host.opened().len(), opened + 1, "it opens");
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(reopened["agent"], agent("claude-sonnet-5-5", Some("low")));
    assert!(to_human(&context, project.id)
        .iter()
        .all(|body| !body.contains("among your agents")));
}
