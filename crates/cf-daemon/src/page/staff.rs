//! The agents the human has and the staff of a project: what the pickers
//! offer, the last staff, the human's Switch chief, and the members added,
//! given other roles, removed and told they are back.

use cf_engine::SwitchWhen;
use cf_harness::detect::missing_harnesses;
use cf_ledger::model::parse_roles;
use cf_ledger::NewMember;
use serde_json::{json, Value};

use super::agents::{last_staff_now, membership, saved};
use super::body::{merged, one, Body, Fields, Said};
use super::chief::chief_asked;
use super::Page;
use crate::roster::offerable;

/// `agents.list`: the pickers offer only agents on a harness installed here,
/// and say which are not.
pub(super) async fn agents(page: &Page) -> Result<Fields, Said> {
    let missing = missing_harnesses(&page.env);
    let agents = saved(&page.env)?;
    let listed = agents.roster().list()?;
    let mut fields = Fields::new();
    fields.insert(
        "agents".to_owned(),
        Value::Array(offerable(&listed, &missing)?),
    );
    fields.insert("missing".to_owned(), serde_json::to_value(&missing)?);
    Ok(fields)
}

/// `staff.last`.
pub(super) async fn last(page: &Page) -> Result<Fields, Said> {
    let agents = saved(&page.env)?;
    let staff = last_staff_now(&page.ledger.borrow(), &agents.roster())?;
    one("staff", staff)
}

/// `chief.switch`: the human's Switch chief, a harness installed here and a model of it
/// (`chief_asked`). `when: 'turn'` lets a chief at work finish its turn; `note` first asks
/// it where things stand.
pub(super) async fn switch_chief(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let when = match body.get("when") {
        None => SwitchWhen::Now,
        Some(Value::String(when)) if when == "now" => SwitchWhen::Now,
        Some(Value::String(when)) if when == "turn" => SwitchWhen::Turn,
        Some(_) => return Err(Said::from("when is now or turn")),
    };
    let note = body.get("note") == Some(&Value::Bool(true));
    let target = chief_asked(page, body, true)?;
    let project = page
        .engine
        .switch_chief(body.whole("project")?, target, when, note)
        .await?;
    one("project", project)
}

/// `member.add`: a saved agent joins the staff, a worker unless the page says
/// otherwise, on a harness the daemon can open a window of.
pub(super) async fn add(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let agents = saved(&page.env)?;
    let member = membership(&agents.roster(), body.get("agent"))?;
    page.engine.require_adapter(&member.harness)?;
    let roles = body
        .get("roles")
        .cloned()
        .unwrap_or_else(|| json!(["worker"]));
    let member = NewMember::from_json(&member.with_roles(Some(&roles)))?;
    let added = page
        .ledger
        .borrow_mut()
        .add_member(body.whole("project")?, &member)?;
    one("member", added)
}

/// `member.roles`: the roles are checked before the member is looked for.
pub(super) async fn roles(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let roles = parse_roles(body.get("roles"))?;
    let member =
        page.ledger
            .borrow_mut()
            .set_roles(body.whole("project")?, &body.text("agent"), &roles)?;
    one("member", member)
}

/// `member.remove`: off the staff once its step in progress is over, its open
/// tasks cancelled.
pub(super) async fn remove(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let removed = page
        .engine
        .remove_member(body.whole("project")?, &body.text("agent"))
        .await?;
    merged(removed)
}

/// `member.back`: a member, or the chief, out of quota is back before its
/// reset: the human ran its harness on another account, or a bigger plan.
pub(super) async fn back(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let member = page
        .engine
        .back_from_quota(body.whole("project")?, &body.text("participant"))?;
    one("member", member)
}
