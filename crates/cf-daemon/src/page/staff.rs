//! The agents the human has and the staff of a project: what the pickers
//! offer, the last staff, the human's Switch chief, and the members added,
//! given other roles, removed and told they are back.

use cf_catalog::{validate_work_tier, WorkTier};
use cf_engine::SwitchWhen;
use cf_harness::detect::missing_harnesses;
use cf_ledger::model::parse_roles;
use cf_ledger::{MemberEdit, NewMember, ParticipantView};
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::agents::{last_staff_now, membership, saved};
use super::body::{merged, one, Body, Fields, Said};
use super::chief::chief_asked;
use super::Page;
use crate::roster::{offerable, MemberAgent};

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
/// otherwise, on a harness the daemon can open a window of. Add member names no saved
/// agent: it names a harness, a model that harness lists, its effort and the tier the
/// human chose (T-1143, [`add_picked`]).
pub(super) async fn add(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    if body.get("model").is_some() {
        return add_picked(page, body);
    }
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

/// Add member (T-1143): a member on a model its harness lists, at an effort it lists for it,
/// in the tier the human chose, a worker unless the page says otherwise. It runs on the
/// human's saved agent for that model and effort, made where there is none
/// ([`crate::roster::Agents::member_agent`]). A member on that agent already takes the
/// roles as roles more, in the tier chosen now.
fn add_picked(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let roles = parse_roles(Some(body.get("roles").unwrap_or(&json!(["worker"]))))?;
    let designer = roles.contains(&"designer");
    let project = body.whole("project")?;
    let picked = picked(page, body)?;
    let agent = saved(&page.env)?.member_agent(&picked.agent(designer))?;
    let kind = picked.harness.kind().to_owned();
    let tier = picked.tier.as_str().to_owned();
    let on_it = active_member(page, project, |member| {
        member.agent.as_deref() == Some(&agent)
    })?;
    let mut ledger = page.ledger.borrow_mut();
    let Some(member) = on_it else {
        let member = NewMember {
            agent,
            harness: kind,
            designer,
            roles: roles.into_iter().map(str::to_owned).collect(),
            tier,
        };
        return one(
            "member",
            ledger.add_member_in_chosen_tier(project, &member)?,
        );
    };
    let mut held = member.roles.clone();
    held.extend(
        roles
            .into_iter()
            .filter(|role| !member.roles.iter().any(|kept| kept == role))
            .map(str::to_owned),
    );
    ledger.set_roles(project, &member.handle, &held)?;
    let edit = MemberEdit {
        agent,
        harness: kind,
        designer,
        tier,
    };
    one(
        "member",
        ledger.edit_member(project, &member.handle, &edit)?,
    )
}

/// `member.edit`: Edit member (T-1143). The member `member` names runs from its next window
/// on a model its harness lists, at an effort it lists for it, in the tier the human chose;
/// its handle, roles and work stay, and a window open now keeps what it runs on.
pub(super) async fn edit(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let handle = body.text("member").into_owned();
    let picked = picked(page, body)?;
    let member = active_member(page, project, |member| member.handle == handle)?
        .ok_or_else(|| Said(format!("no member @{handle} on this project's staff")))?;
    let designer = member.roles.iter().any(|role| role == "designer");
    let agent = saved(&page.env)?.member_agent(&picked.agent(designer))?;
    let edit = MemberEdit {
        agent,
        harness: picked.harness.kind().to_owned(),
        designer,
        tier: picked.tier.as_str().to_owned(),
    };
    let edited = page
        .ledger
        .borrow_mut()
        .edit_member(project, &handle, &edit)?;
    one("member", edited)
}

/// A member of `project`'s staff now, not a session of one, that `which` picks.
fn active_member(
    page: &Page,
    project: i64,
    which: impl Fn(&ParticipantView) -> bool,
) -> Result<Option<ParticipantView>, Said> {
    let Some(view) = page.ledger.borrow().project(project)? else {
        return Err(Said(format!("no project {project}")));
    };
    Ok(view.participants.into_iter().find(|member| {
        member.role != "chief"
            && member.role != "human"
            && member.member_id.is_none()
            && member.left_at.is_none()
            && which(member)
    }))
}

/// What Add member and Edit member picked (T-1143): a harness, the model as its launch passes
/// it, the effort a launch passes beside it, and the tier the human chose.
struct Picked {
    harness: Harness,
    model: String,
    effort: Option<String>,
    tier: WorkTier,
}

impl Picked {
    fn agent(&self, designer: bool) -> MemberAgent<'_> {
        MemberAgent {
            harness: self.harness,
            model: &self.model,
            effort: self.effort.as_deref(),
            designer,
            tier: self.tier,
        }
    }
}

/// The harness (by its kind, `claude-code`, as the harness select says it), the model, the
/// effort and the tier `body` names, each checked: the harness one ConsensFlow opens windows
/// of; the model one it lists and runs, at one of the efforts it lists for it (none where it
/// lists none), where the home keeps its listing (a harness never read is taken at its word,
/// as a chief's model is); and a tier, which the human must pick. Devin's effort is part of
/// the id it lists for it (`model_uid`): the member's model is that id, never the model and
/// the effort joined, and no effort goes beside it.
fn picked(page: &Page, body: Body<'_>) -> Result<Picked, Said> {
    let kind = body.text("harness").into_owned();
    let harness = Harness::from_kind(&kind)
        .ok_or_else(|| Said(format!("{kind} is no harness ConsensFlow runs")))?;
    page.engine.require_adapter(harness.kind())?;
    let model = match body.get("model") {
        Some(Value::String(model)) if !model.is_empty() => model.as_str(),
        _ => return Err(Said::from("pick the member's model: one its harness lists")),
    };
    cf_ledger::model::require_model_id(model)?;
    let effort = match body.get("effort") {
        None | Some(Value::Null) => None,
        Some(Value::String(effort)) if !effort.is_empty() => Some(effort.as_str()),
        Some(_) => {
            return Err(Said::from(
                "an effort is one of the words its harness lists",
            ))
        }
    };
    let tier = match validate_work_tier(body.get("tier")) {
        Ok(Some(tier)) => tier,
        Ok(None) => {
            return Err(Said::from(
                "pick the member's tier: critical, complex, standard or light",
            ))
        }
        Err(refused) => return Err(Said(refused.message)),
    };
    let kept = page
        .ledger
        .borrow()
        .model_listing(harness.as_str())
        .map_err(|failed| failed.to_string())?
        .filter(|listing| !listing.models.is_empty());
    let Some(listing) = kept else {
        return Ok(Picked {
            harness,
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
            tier,
        });
    };
    let Some(listed) = listing.model(model) else {
        return Err(Said(format!(
            "{kind} does not list {model}: pick one of its models"
        )));
    };
    if listed.disabled {
        return Err(Said(format!(
            "{kind} lists {model} but will not run it: pick another of its models"
        )));
    }
    match effort {
        None if !listed.efforts.is_empty() => {
            return Err(Said(format!(
                "pick the effort {kind} runs {model} at: {}",
                listed.efforts.join(", ")
            )))
        }
        Some(effort) if !listed.efforts.iter().any(|offered| offered == effort) => {
            return Err(Said(if listed.efforts.is_empty() {
                format!("{kind} takes no effort for {model}")
            } else {
                format!(
                    "{kind} runs {model} at {}, not {effort}",
                    listed.efforts.join(", ")
                )
            }))
        }
        _ => {}
    }
    let (model, effort) = match effort.and_then(|effort| listed.launch_ids.get(effort)) {
        Some(launch) => (launch.clone(), None),
        None => (listed.model(), effort.map(str::to_owned)),
    };
    Ok(Picked {
        harness,
        model,
        effort,
        tier,
    })
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
