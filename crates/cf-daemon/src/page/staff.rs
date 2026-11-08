//! The staff of a project as the page asks for it (spec harness-models): the harnesses the
//! pickers offer, the last staff, the human's Switch chief, and the members added, edited,
//! given other roles, removed and told they are back. A member is a harness, a model its
//! harness lists, an effort it lists for that model, and a tier the human picks: no saved
//! agent behind it.

use cf_engine::SwitchWhen;
use cf_harness::detect::missing_harnesses;
use cf_ledger::model::parse_roles;
use cf_ledger::{MemberEdit, NewMember, ParticipantView};
use cf_proto::agents::{Harness, WorkTier};
use serde_json::{json, Value};

use super::body::{merged, one, Body, Fields, Said};
use super::chief::chief_asked;
use super::Page;

/// `harnesses.missing`: the harnesses not installed here, which the pickers leave out.
pub(super) async fn missing(page: &Page) -> Result<Fields, Said> {
    let missing = missing_harnesses(&page.env);
    let mut fields = Fields::new();
    fields.insert("missing".to_owned(), serde_json::to_value(&missing)?);
    Ok(fields)
}

/// `staff.last`: the members of the newest project that has any, each as its own
/// harness, model, effort, model's name, tier and roles: what New project starts from.
pub(super) async fn last(page: &Page) -> Result<Fields, Said> {
    let staff = page.ledger.borrow().last_staff()?;
    one("staff", staff)
}

/// `chief.switch`: the human's Switch chief, a harness installed here, a model of it and
/// its effort (`chief_asked`). `when: 'turn'` lets a chief at work finish its turn; `note`
/// first asks it where things stand.
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

/// `member.add`: Add member (T-1143). A member on a model its harness lists, at an effort
/// it lists for it, in the tier the human chose, a worker unless the page says otherwise,
/// named by the model ([`cf_ledger::handles`]). A member on that harness, model and effort
/// already takes the roles as roles more, in the tier chosen now. An image designer, which
/// draws with Codex's image tool and holds no other role, is that only to a designer: a
/// worker on the same model is a member beside it, whichever came first.
pub(super) async fn add(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let member = picked_member(page, body, true)?;
    let on_it = active_member(page, project, |member_now| {
        member_now.harness.as_deref() == Some(member.harness.as_str())
            && member_now.model == member.model
            && member_now.effort == member.effort
            && member_now.designer == member.designer
    })?;
    let mut ledger = page.ledger.borrow_mut();
    let Some(existing) = on_it else {
        return one("member", ledger.add_member(project, &member)?);
    };
    let mut held = existing.roles.clone();
    held.extend(
        member
            .roles
            .iter()
            .filter(|role| !existing.roles.contains(role))
            .cloned(),
    );
    ledger.set_roles(project, &existing.handle, &held)?;
    one(
        "member",
        ledger.edit_member(project, &existing.handle, &edit_of(&member))?,
    )
}

/// `member.edit`: Edit member (T-1143). The member `member` names runs from its next window
/// on a model its harness lists, at an effort it lists for it, in the tier the human chose;
/// its handle, roles and work stay, and a window open now keeps what it runs on.
pub(super) async fn edit(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let handle = body.text("member").into_owned();
    let found = active_member(page, project, |member| member.handle == handle)?
        .ok_or_else(|| Said(format!("no member @{handle} on this project's staff")))?;
    let designer = found.roles.iter().any(|role| role == "designer");
    if designer {
        return Err(Said::from(
            "an image designer has nothing to edit: it draws on Codex's own model",
        ));
    }
    let picked = picked(page, body, designer, true)?;
    let edited = page
        .ledger
        .borrow_mut()
        .edit_member(project, &handle, &picked.edit())?;
    one("member", edited)
}

/// What `member` runs on, as Edit member changes it.
fn edit_of(member: &NewMember) -> MemberEdit {
    MemberEdit {
        harness: member.harness.clone(),
        model: member.model.clone(),
        effort: member.effort.clone(),
        model_name: member.model_name.clone(),
        tier: member.tier.clone(),
    }
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

/// The member `body` asks for, as Add member and New project's staff give one: its roles
/// (a worker where it names none), and what it runs on ([`picked`]); `adapter` where the
/// page asks the engine whether it opens windows of the harness (Add member: New project's
/// engine says so as it opens the project).
pub(super) fn picked_member(page: &Page, body: Body<'_>, adapter: bool) -> Result<NewMember, Said> {
    let roles = parse_roles(Some(body.get("roles").unwrap_or(&json!(["worker"]))))?;
    let designer = roles.contains(&"designer");
    let picked = picked(page, body, designer, adapter)?;
    Ok(NewMember {
        harness: picked.harness.kind().to_owned(),
        model: picked.model,
        effort: picked.effort,
        model_name: picked.model_name,
        designer,
        roles: roles.into_iter().map(str::to_owned).collect(),
        tier: picked.tier.as_str().to_owned(),
    })
}

/// What Add member and Edit member picked (T-1143): a harness, the model as its launch
/// passes it, the effort it was picked at, the name the harness's list gives the model, and
/// the tier the human chose.
struct Picked {
    harness: Harness,
    model: Option<String>,
    effort: Option<String>,
    model_name: Option<String>,
    tier: WorkTier,
}

impl Picked {
    fn edit(&self) -> MemberEdit {
        MemberEdit {
            harness: self.harness.kind().to_owned(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            model_name: self.model_name.clone(),
            tier: self.tier.as_str().to_owned(),
        }
    }
}

/// The tier `value` names: one of the four words, which the human must pick.
pub(super) fn tier_of(value: Option<&Value>) -> Result<WorkTier, Said> {
    match value {
        Some(Value::String(word)) => WorkTier::from_word(word)
            .ok_or_else(|| Said::from("Work tier must be critical, complex, standard or light")),
        None | Some(Value::Null) => Err(Said::from(
            "pick the member's tier: critical, complex, standard or light",
        )),
        Some(_) => Err(Said::from(
            "Work tier must be critical, complex, standard or light",
        )),
    }
}

/// The harness (by its kind, `claude-code`, as the harness select says it), the model, the
/// effort and the tier `body` names, each checked: the harness one ConsensFlow opens windows
/// of, and Codex for an image designer, which draws with Codex's image tool; the model one
/// it lists and runs, at one of the efforts it lists for it (none where it lists none),
/// where the home keeps its listing (a harness never read is taken at its word, as a
/// chief's model is), named as the listing names it; and a tier, which the human must
/// pick. Devin's effort is part of the id it lists for it (`model_uid`): the member's model
/// is that id, never the model and the effort joined, and its effort is said beside it.
fn picked(page: &Page, body: Body<'_>, designer: bool, adapter: bool) -> Result<Picked, Said> {
    let kind = body.text("harness").into_owned();
    let harness = Harness::from_kind(&kind)
        .ok_or_else(|| Said(format!("{kind} is no harness ConsensFlow runs")))?;
    if designer && harness != Harness::Codex {
        return Err(Said::from("an image designer draws on Codex"));
    }
    if adapter {
        page.engine.require_adapter(harness.kind())?;
    }
    if designer {
        return designer_picked(body);
    }
    let model = match body.get("model") {
        Some(Value::String(model)) if !model.is_empty() => model.as_str(),
        _ => return Err(Said::from("pick the member's model: one its harness lists")),
    };
    let effort = effort_of(body)?;
    let tier = tier_of(body.get("tier"))?;
    let resolved = resolve(page, harness, &kind, model, effort)?;
    Ok(Picked {
        harness,
        model: Some(resolved.model),
        effort: resolved.effort,
        model_name: resolved.model_name,
        tier,
    })
}

/// An image designer: Codex's own default model at no effort, whatever it was asked for.
/// Its window is launched with neither (the Codex adapter drops what a designer's row holds)
/// and a design task has no tier, so a model, an effort or a tier asked for it is refused
/// rather than kept as what it does not run on (designer P2, T-1280). The ledger keeps every
/// member in a tier: a designer is kept in the lowest, which decides nothing.
fn designer_picked(body: Body<'_>) -> Result<Picked, Said> {
    let given = |field: &str| !matches!(body.get(field), None | Some(Value::Null));
    if ["model", "effort", "tier"].into_iter().any(given) {
        return Err(Said::from(
            "an image designer draws on Codex's own model: it takes no model, effort or tier",
        ));
    }
    Ok(Picked {
        harness: Harness::Codex,
        model: None,
        effort: None,
        model_name: None,
        tier: DESIGNER_TIER,
    })
}

/// The tier an image designer is kept in.
const DESIGNER_TIER: WorkTier = WorkTier::Light;

/// The effort `body` names: none where it names none, else a word its harness lists.
pub(super) fn effort_of<'b>(body: Body<'b>) -> Result<Option<&'b str>, Said> {
    match body.get("effort") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(effort)) if !effort.is_empty() => Ok(Some(effort.as_str())),
        Some(_) => Err(Said::from(
            "an effort is one of the words its harness lists",
        )),
    }
}

/// A model and an effort picked on a harness, as its harness takes them.
pub(super) struct Resolved {
    /// The model as the launch passes it: Devin's the exact id it lists for the effort.
    pub(super) model: String,
    pub(super) effort: Option<String>,
    /// The model as the harness's list names it, its id (Pi's `provider/id`, Devin's listed id
    /// and not the launch id `model` holds); none where its list was never kept.
    pub(super) model_name: Option<String>,
}

/// `model` at `effort` on `harness` (`kind`, as the body said it), checked as Add member,
/// Edit member, New project and Switch chief all check it: a model id the ledger takes; where
/// the home keeps the harness's listing, one it lists and runs, at one of the efforts it
/// lists for it (none where it lists none), named as the listing names it, and held as the
/// launch passes it (Devin's effort is part of the id it lists for it, `model_uid`). A harness
/// never read is taken at its word, as it was asked.
pub(super) fn resolve(
    page: &Page,
    harness: Harness,
    kind: &str,
    model: &str,
    effort: Option<&str>,
) -> Result<Resolved, Said> {
    cf_ledger::model::require_model_id(model)?;
    let kept = page
        .ledger
        .borrow()
        .model_listing(harness.as_str())
        .map_err(|failed| failed.to_string())?
        .filter(|listing| !listing.models.is_empty());
    let Some(listing) = kept else {
        return Ok(Resolved {
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
            model_name: None,
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
    Ok(Resolved {
        model: listed.launch_model(effort),
        effort: effort.map(str::to_owned),
        // The model as the list names it, its id: Devin's listed id, not the launch id the
        // member's `model` holds, and Pi's `provider/id`. What the member is named by and shown as.
        model_name: Some(listed.model()),
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
