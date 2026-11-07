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

/// The words of a harness that found no default (a Pi read that failed quotes what Pi said,
/// and what Pi said can carry a credential) reach the human's note without it (T-1270). **The
/// words of Pi are screened whole, before any cut** (the third reading, F2): if anything in
/// what was kept of them may hold a credential, none of them is quoted, whatever line it is on
/// and however a value runs over lines. The description of the failure and its exit code stay,
/// in the note's one paragraph.
#[test]
fn the_note_of_a_gone_model_never_quotes_a_credential_the_harness_said() {
    for (said, hidden) in [
        (
            "Error: Failed to load extension\nextension failed: API_KEY: sk-abcd efghijkl mnopqrst\n\
             password: correct horse battery staple\nAuthorization: Bearer SYNTHETICSTDERRTOKEN0123456789",
            vec!["sk-abcd", "efghijkl", "mnopqrst", "horse", "battery", "staple", "SYNTHETICSTDERRTOKEN"],
        ),
        // The third reading's probes: a value that runs over lines, and a label that five lines
        // in a row would have lost to a cut.
        (
            "Error: extension failed\nAPI_KEY: sk-abcd\nefghijkl\nmnopqrst",
            vec!["sk-abcd", "efghijkl", "mnopqrst"],
        ),
        (
            "password: correct\nhorse\nbattery\nstaple\nprovider unavailable",
            vec!["correct", "horse", "battery", "staple"],
        ),
        (
            "{\n  \"error\": {\n    \"apiKey\": \"sk-abcd efghijkl\"\n  }\n}",
            vec!["sk-abcd", "efghijkl"],
        ),
        // T-1276: a label in a line of the shape of a stack frame, and a URL's user and
        // password that a line end splits.
        (
            "at password: correct (native)\nhorse\nbattery\nstaple",
            vec!["correct", "horse", "battery", "staple"],
        ),
        (
            "FetchError: https://alder:copper\nwillow@host.invalid/catalog",
            vec!["alder", "copper", "willow"],
        ),
        // T-1276: what a read of Pi that kept only the end of what Pi said puts in place of it.
        (cf_proto::models::WORDS_WITHHELD, vec![]),
    ] {
        let context = harness_without_the_model(Err(format!(
            "pi --mode rpc ended before it answered (exit 1); it said:\n{said}"
        )));
        let project = opened_on_a_gone_model(&context);
        context.pass().unwrap();
        let told: Vec<String> = to_human(&context, project)
            .into_iter()
            .filter(|body| body.contains("no longer available"))
            .collect();
        assert_eq!(told.len(), 1, "{told:?}");
        for leaked in hidden {
            assert!(!told[0].contains(leaked), "{leaked} in {}", told[0]);
        }
        assert!(
            told[0].contains("(Pi's words withheld: they may hold a credential)"),
            "Pi's words were withheld: {}",
            told[0]
        );
        assert!(
            told[0].contains("ended before it answered (exit 1)"),
            "the description and the exit code are said: {}",
            told[0]
        );
        assert!(!told[0].contains('\n'), "a note is one paragraph: {}", told[0]);
    }
}

/// The words of Pi that hold no credential are in the note, as they were.
#[test]
fn the_note_of_a_gone_model_quotes_the_words_of_pi_that_hold_no_credential() {
    let context = harness_without_the_model(Err(
        "pi --mode rpc ended before it answered (exit 1); it said:\nError: Failed to load \
         extension \"/x/e.ts\": boom\nHint: Start without extensions using \"pi -ne\"."
            .to_owned(),
    ));
    let project = opened_on_a_gone_model(&context);
    context.pass().unwrap();
    let told: Vec<String> = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("Error: Failed to load extension \"/x/e.ts\": boom")
            && told[0].contains("Hint: Start without extensions using \"pi -ne\"."),
        "{}",
        told[0]
    );
    assert!(
        !told[0].contains("withheld") && !told[0].contains('\n'),
        "{}",
        told[0]
    );
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

/// What the chief's screen shows is never its harness's word that its model is gone, once
/// the window came up (Calliope's P1, T-955): the chief grepped this repository for the
/// words, the human quit its window, and the project goes as a human's quit takes it.
#[test]
fn a_chief_that_came_up_and_ended_with_a_gone_models_words_on_its_screen_keeps_its_model() {
    // Ended by the human's quit (0) or by a crash (1): a window that came up either way.
    for code in [0, 1] {
        let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        // It works: it is handed words and takes them, its record shows.
        let note = context.note_from_consensflow(project.id, "chief", "Look at T-1");
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(context.message(note.id).state, "delivered");
        context.host.exits_showing(
            Some(code),
            None,
            Some(&["tests/models.rs:12: Error: Unknown model: 'claude-opus-5'"]),
        );
        context.exit("chief");
        context.pass().unwrap();
        let chief = chief_of(&context, project.id);
        assert_eq!(
            (chief.model.as_deref(), chief.gone_model.as_deref()),
            (Some("claude-opus-5"), None),
            "exit code {code}"
        );
        assert_eq!(
            context.project(project.id).state,
            "suspended",
            "exit code {code}"
        );
        assert!(to_human(&context, project.id)
            .iter()
            .all(|body| !body.contains("no longer available")));
    }
}

/// A window that never came up and was given up on, its harness's words on its screen,
/// ended with no code of its own: it is tried again as any launch that did not come up
/// (Pi warns "not found for provider" of a model its offline catalog does not know yet,
/// and runs it all the same).
#[test]
fn a_window_given_up_on_with_a_gone_models_words_is_tried_again_and_keeps_its_model() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
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
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), None)
    );
    assert!(to_human(&context, project.id)
        .iter()
        .all(|body| !body.contains("no longer available")));
    assert!(context
        .adapter
        .prepared()
        .iter()
        .all(|launch| launch["agent"] == agent("claude-opus-5", None)));
}

/// A failed turn is read in its record's own words, never its screen's.
#[test]
fn a_failed_turn_whose_record_says_nothing_of_the_model_keeps_it_whatever_the_screen_shows() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.host.set_snapshot(json!({
        "tail": ["$ rg model_not_found", "Error: Unknown model: 'claude-opus-5'"]
    }));
    context.adapter.with("chief", |chief| {
        chief.failed = true;
        chief.failure = None;
    });
    context.adapter.answer("chief", "…");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), None)
    );
}

/// Every harness's refusal names the model it refused: words of a gone model that name
/// another are no word of the chief's.
#[test]
fn a_failed_turn_whose_record_names_another_gone_model_keeps_the_chiefs() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |chief| {
        chief.failed = true;
        chief.failure = Some("400 model_not_found: Unknown model 'claude-instant-1'".to_owned());
    });
    context.adapter.answer("chief", "…");
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), None)
    );
}

/// The hold of a chief whose model is gone and whose harness has no other to start it on
/// is the ledger's, not the engine's memory (Calliope's P2, T-955): after a restart and a
/// Resume it still opens no window on the gone model and tells the human nothing more. Its
/// model stays the human's pick: never none, which would be the harness's own default.
#[test]
fn a_chief_held_for_no_default_stays_held_across_a_restart_and_is_told_once() {
    let context = harness_without_the_model(Err("no model to start on".to_owned()));
    let project = opened_on_a_gone_model(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let launched = context.adapter.prepared().len();
    let chief = chief_of(&context, project);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), Some("claude-opus-5")),
        "held in the ledger"
    );
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project).unwrap();
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        context.adapter.prepared().len(),
        launched,
        "no launch on it"
    );
    let told = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .count();
    assert_eq!(told, 1);
    // Held, it costs a pass nothing: no event, no window asked for, pass after pass.
    let events = |context: &Context| {
        let mut count = 0;
        context.edit_ledger(|file| {
            let db = rusqlite::Connection::open(file).unwrap();
            count = db
                .query_row("SELECT COUNT(*) FROM event", [], |row| row.get::<_, i64>(0))
                .unwrap();
        });
        count
    };
    let (before, opened) = (events(&context), context.host.opened().len());
    for _ in 0..50 {
        after.pass().unwrap();
    }
    assert_eq!(
        (events(&context), context.host.opened().len()),
        (before, opened)
    );
}

/// A launch that ended before its window came up with no code of failure is no refusal of
/// its harness's, whatever its screen shows: only a refusal ends a launch with one
/// (Calliope's P1, T-955).
#[test]
fn a_launch_that_ended_with_code_0_before_it_came_up_keeps_its_model() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    context.host.exits_showing(
        Some(0),
        None,
        Some(&["Error: Unknown model: 'claude-opus-5'"]),
    );
    *context.host.exit_after_open.borrow_mut() = Some("chief".to_owned());
    let project = context.with_staff(&["zeus"]);
    context.host.exits_showing(None, None, None);
    context.pass().unwrap();
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), None)
    );
    assert!(to_human(&context, project.id)
        .iter()
        .all(|body| !body.contains("no longer available")));
}

/// A chief that worked, was closed and resumed, and whose relaunch its harness refuses
/// (Devin's resume on a model it no longer has ends at once): the window before took work,
/// this one none, and its refusal is heard (CHF-06), whatever the conversation's history.
#[test]
fn a_resumed_chief_whose_relaunch_is_refused_starts_on_the_default() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let note = context.note_from_consensflow(project.id, "chief", "Look at T-1");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(note.id).state, "delivered");
    // Its turn over, the Close cuts nothing: the relaunch is handed nothing first.
    context.adapter.answer("chief", "On it");
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    context.host.exits_showing(
        Some(1),
        None,
        Some(&["Error: Unknown model: 'claude-opus-5'"]),
    );
    *context.host.exit_after_open.borrow_mut() = Some("chief".to_owned());
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-haiku-5-5"), Some("claude-opus-5"))
    );
}

/// A Pi chief's model is `provider/id`, and its provider refuses the bare id: Anthropic's
/// `not_found_error` names `claude-fable-5`, never `anthropic/claude-fable-5`. The harness
/// says whether its words refuse the model asked for, so the chief is heard and opens on
/// the default, with one note (Calliope's P1, T-994).
#[test]
fn a_pi_chiefs_gone_model_named_by_its_provider_without_the_provider_starts_it_on_the_default() {
    let context = harness_without_the_model(Ok(Some("anthropic/claude-haiku-5-5".to_owned())));
    *context.adapter.gone_words.borrow_mut() = Some("not_found_error".to_owned());
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "pi", "model": "anthropic/claude-fable-5" },
            "staff": [],
        }))
        .unwrap()
        .expect("the project");
    context.pass().unwrap();
    context.adapter.with("chief", |chief| {
        chief.failed = true;
        chief.failure = Some(
            r#"404 {"type":"error","error":{"type":"not_found_error","message":"model: claude-fable-5"}}"#
                .to_owned(),
        );
    });
    context.adapter.answer("chief", "…");
    context.pass().unwrap();
    context.pass().unwrap();
    let launches = context.adapter.prepared();
    assert_eq!(
        launches.last().unwrap()["agent"],
        agent("anthropic/claude-haiku-5-5", None),
        "{launches:?}"
    );
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (
            Some("anthropic/claude-haiku-5-5"),
            Some("anthropic/claude-fable-5")
        )
    );
    let told = to_human(&context, project.id)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .count();
    assert_eq!(told, 1);
    assert!(to_human(&context, project.id)
        .iter()
        .all(|body| !body.contains("The chief's turn failed")));
}

/// Everything a pass of a held chief could cost: the ledger's events, what the fake host
/// was asked to open, and each of the adapter's calls and reads.
fn costs(context: &Context) -> (i64, usize, usize, usize, usize) {
    let mut events = 0;
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        events = db
            .query_row("SELECT COUNT(*) FROM event", [], |row| row.get::<_, i64>(0))
            .unwrap();
    });
    let adapter = context
        .recorder
        .events()
        .iter()
        .filter(|event| event["seam"] == "adapter")
        .count();
    (
        events,
        context.host.opened().len(),
        adapter,
        context.adapter.asked_used().len(),
        context.adapter.defaults_asked(),
    )
}

/// A chief held for want of a model, with a message queued for it and no Resume since:
/// a pass steps it to nothing, so no delivery is begun and given back pass after pass, and
/// no launch reads its harness (Calliope's P2a, T-994).
#[test]
fn a_held_chief_with_a_queued_message_costs_a_pass_nothing() {
    let context = harness_without_the_model(Err("no model to start on".to_owned()));
    let project = opened_on_a_gone_model(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let note = context.note_from_consensflow(project, "chief", "Look at T-1");
    context.pass().unwrap();
    let before = costs(&context);
    for _ in 0..50 {
        context.pass().unwrap();
    }
    assert_eq!(costs(&context), before);
    let message = context.message(note.id);
    assert_eq!(message.state, "queued", "held, not lost");
}

/// A harness whose default is the very model that is gone (Pi's settings, Devin's config,
/// where the human picked their default): the chief is held, with the hold's note, never
/// told it starts on the default and launched on the gone model again (Calliope's P2b,
/// T-994).
#[test]
fn a_harness_default_that_is_the_gone_model_holds_the_chief_with_the_holds_note() {
    let context = harness_without_the_model(Ok(Some("claude-opus-5".to_owned())));
    let project = opened_on_a_gone_model(&context);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let launches = context.adapter.prepared();
    assert_eq!(
        launches.len(),
        1,
        "no launch after the refusal: {launches:?}"
    );
    let told: Vec<String> = to_human(&context, project)
        .into_iter()
        .filter(|body| body.contains("no longer available"))
        .collect();
    assert_eq!(
        told,
        ["The chief's model, claude-opus-5, is no longer available in Claude Code, and ConsensFlow found no other to start it on (its default is the model that is gone): switch the chief to another model."]
    );
    let chief = chief_of(&context, project);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), Some("claude-opus-5")),
        "held"
    );
}

/// A message to the chief quotes a gone model's words beside its model (a handoff about this
/// very rule would), pasted into its window reopened at rest and handed nothing else, which ends with a failure code
/// before its record shows the paste: what ConsensFlow pasted is no word of the harness's, so the chief keeps its
/// model and the project closes as for any window that went (Calliope's P2c, T-994).
#[test]
fn a_pasted_message_quoting_a_gone_models_words_on_screen_is_no_refusal_of_its_window() {
    let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    // Handed over, and gone before its record shows it.
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(|_| {
        Box::pin(async { Ok(cf_harness::contract::Admission::Admitted { queued: false }) })
    }));
    let body = "Pi said: Error: Unknown model: 'claude-opus-5'. Why?";
    let note = context.note_from_consensflow(project.id, "chief", body);
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering", "pasted");
    let header = format!("[ConsensFlow m-{} · note from ConsensFlow]", note.id);
    context
        .host
        .exits_showing(Some(1), None, Some(&[header.as_str(), body]));
    context.exit("chief");
    context.pass().unwrap();
    let chief = chief_of(&context, project.id);
    assert_eq!(
        (chief.model.as_deref(), chief.gone_model.as_deref()),
        (Some("claude-opus-5"), None)
    );
    assert_eq!(context.project(project.id).state, "suspended");
}

/// A chief reopened at rest, handed nothing, lives past its launch's budget and ends with a
/// failure code (on Windows the human's Ctrl+C is one, `0xC000013A`), a gone model's words
/// on its screen: no refusal at launch, so its screen is read in no such words, and it keeps
/// its model (Calliope's P2c, T-994).
#[test]
fn a_chief_at_rest_that_ends_failing_past_its_launch_budget_keeps_its_model() {
    for code in [1, 0xC000_013A] {
        let context = harness_without_the_model(Ok(Some("claude-haiku-5-5".to_owned())));
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        context.adapter.answer("chief", "Hello");
        context.pass().unwrap();
        context.close_project(project.id).unwrap();
        context.resume_project(project.id).unwrap();
        context.pass().unwrap();
        context.advance(10 * 60 * 1000);
        context.host.exits_showing(
            Some(code),
            None,
            Some(&["Error: Unknown model: 'claude-opus-5'"]),
        );
        context.exit("chief");
        context.pass().unwrap();
        let chief = chief_of(&context, project.id);
        assert_eq!(
            (chief.model.as_deref(), chief.gone_model.as_deref()),
            (Some("claude-opus-5"), None),
            "exit code {code:#X}"
        );
        assert_eq!(
            context.project(project.id).state,
            "suspended",
            "exit code {code:#X}"
        );
    }
}

/// A chief the update could not tell the model of (T-1202) whose harness's record names
/// what its conversation ran on last: it keeps that model and effort, needs no edit, and
/// opens on them.
#[test]
fn a_chief_that_needs_an_edit_takes_the_model_its_record_names_and_opens_on_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context.close_project(project).unwrap();
    let chief = chief_of(&context, project).id;
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute_batch(&format!(
            "UPDATE participant SET model = NULL, effort = NULL, needs_edit = 1 WHERE id = {chief}"
        ))
        .unwrap();
    });
    context.adapter.with("chief", |chief| {
        chief.used = Some(used("claude-sonnet-5-5", Some("low"), false))
    });
    context.resume_project(project).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let held = chief_of(&context, project);
    assert_eq!(
        (
            held.model.as_deref(),
            held.effort.as_deref(),
            held.needs_edit
        ),
        (Some("claude-sonnet-5-5"), Some("low"), false)
    );
    let reopened = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(reopened["role"], "chief");
    assert!(
        reopened["resume"].is_string(),
        "on its conversation: {reopened}"
    );
    assert_eq!(reopened["agent"], agent("claude-sonnet-5-5", Some("low")));
}

/// A chief the update could not tell the model of, with no record that names one, never
/// opens on its harness's default: what is written to it waits, queued, and its record is
/// read once. Switch chief gives it a model, takes the mark off, and it opens with what
/// waited.
#[test]
fn a_chief_that_needs_an_edit_with_no_record_waits_closed_for_switch_chief() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]).id;
    let chief = chief_of(&context, project).id;
    // Its window never had a conversation of its harness's, which is closed with the
    // project: there is no record to read.
    context.pass().unwrap();
    context.close_project(project).unwrap();
    let before = context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == "chief")
        .count();
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute_batch(&format!(
            "UPDATE participant SET model = NULL, effort = NULL, needs_edit = 1 WHERE id = {chief}"
        ))
        .unwrap();
    });
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project,
            &cf_ledger::NewNote {
                from: Some("human".to_owned()),
                to: "chief".to_owned(),
                body: "Plan the release".to_owned(),
                task: None,
            },
        )
        .unwrap();
    context.resume_project(project).unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let chiefs = |context: &Context| {
        context
            .adapter
            .prepared()
            .into_iter()
            .filter(|launch| launch["role"] == "chief")
            .count()
    };
    assert_eq!(chiefs(&context), before, "no window on a default");
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "what was written waits"
    );
    assert!(chief_of(&context, project).needs_edit);
    assert!(
        context.adapter.asked_used().len() <= 1,
        "its record is read once: {:?}",
        context.adapter.asked_used()
    );
    // The human picks a model in Switch chief: the mark goes, and the chief opens on it.
    context
        .switch_chief(project, to("claude-code", "diana"), SwitchWhen::Now, false)
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let switched = chief_of(&context, project);
    assert!(!switched.needs_edit);
    assert_eq!(chiefs(&context), before + 1);
    let launched = context
        .adapter
        .prepared()
        .into_iter()
        .rfind(|launch| launch["role"] == "chief")
        .unwrap();
    assert_eq!(launched["agent"], agent("gpt-5.6-luna", None));
    assert_ne!(
        context.message(note.id).state,
        "queued",
        "what waited went in"
    );
}
