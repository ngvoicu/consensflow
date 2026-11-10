//! The rows the live kit seats for a real harness name an effort wherever the harness lists
//! some for the model (T-1369). The daemon refuses a member or a chief on a model that lists
//! efforts and names none ("pick the effort codex runs gpt-5.6-luna at: low, medium, …", A4-28),
//! and the kit's cheap pick of Codex had none, so `cargo xtask live finish codex` was refused at
//! `member.add` on the Windows release check. The stand-in Codex lists `gpt-6-astra` at low,
//! medium and high whatever the roster says (`stand_in_lists`), as the real one lists
//! `gpt-5.6-luna`: the same refusal, in the field's words, without a model turn.

use cf_e2e::live::agents::{self, Model};
use cf_e2e::live::{Harness, Log};
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, Outcome};

const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// The model the stand-in Codex lists efforts for, as the kit would name it from `--models`
/// with an effort or without.
fn astra(effort: Option<&str>) -> Model {
    Model {
        harness: Harness::Codex,
        name: "gpt-6-astra".to_owned(),
        effort: effort.map(str::to_owned),
    }
}

/// The roster of a live run: the stand-in chief and the kit's row for `model`.
fn roster(model: &Model) -> Vec<Value> {
    vec![
        json!({ "id": "chief", "kind": "claude-code", "model": "fake-chief" }),
        agents::row_on(model, "standard"),
    ]
}

#[test]
fn a_member_on_a_model_that_lists_efforts_is_given_the_least_one_it_lists() -> Outcome {
    let rig = Rig::start(config().codex(FAKE_CODEX))?;
    agents::write_roster(&rig, &roster(&astra(None)), &Log::new("kit efforts"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    // The daemon takes the member: the kit's row named the lowest effort the list gives.
    let added = project.add_member("live-codex")?;
    assert_eq!(added["member"]["effort"], "low", "{added}");
    assert_eq!(added["member"]["model"], "gpt-6-astra", "{added}");
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_on_a_model_that_lists_efforts_is_opened_at_the_least_one_it_lists() -> Outcome {
    let rig = Rig::start(config().codex(FAKE_CODEX))?;
    let rows = roster(&astra(None));
    agents::write_roster(&rig, &rows, &Log::new("kit efforts"))?;
    // New project asks for the chief's harness, model and effort: refused at project.open
    // before anything starts, when the row names no effort.
    let project = Project::open(&rig, "live-codex", json!({}))?;
    let board = project.board()?;
    assert_eq!(board["project"]["id"], json!(project.id()), "{board}");
    let written: Value = serde_json::from_str(&std::fs::read_to_string(rig.stand_ins_file())?)?;
    let row = written["agents"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == "live-codex"))
        .ok_or("the roster has no live-codex")?;
    assert_eq!(row["effort"], "low", "{written}");
    rig.close()?;
    Ok(())
}

#[test]
fn an_effort_the_run_names_is_kept_and_so_is_the_daemons_check_of_it() -> Outcome {
    let rig = Rig::start(config().codex(FAKE_CODEX))?;
    agents::write_roster(
        &rig,
        &roster(&astra(Some("high"))),
        &Log::new("kit efforts"),
    )?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("live-codex")?;
    assert_eq!(added["member"]["effort"], "high", "{added}");
    rig.close()?;
    Ok(())
}

#[test]
fn the_roster_of_the_muse_cases_gives_its_worker_the_least_effort_too() -> Outcome {
    // The Muse cases write their own roster (`muse::members::project`); the worker it is given is
    // read the same way for any harness, and the stand-in Codex lists efforts for this model. Only
    // the handle is Muse's to say, so the case's own check of it is what ends the call.
    let rig = Rig::start(config().codex(FAKE_CODEX))?;
    let worker = agents::agent_on(&astra(None));
    let ended = cf_e2e::muse::members::project(&rig, &worker).err();
    let said = ended.map(|why| why.to_string()).unwrap_or_default();
    assert!(
        said.starts_with("the Muse worker is not @"),
        "the daemon refused the worker: {said}"
    );
    assert!(said.contains(r#""effort":"low""#), "{said}");
    rig.close()?;
    Ok(())
}
