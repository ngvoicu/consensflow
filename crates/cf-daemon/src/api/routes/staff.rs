//! `GET /api/staff`: the members of the staff, each with its roles and tier (an image
//! designer has none, a design task having none), the harness
//! it runs on and the name the page gives that harness, the model as its harness takes it
//! and as its harness's list names it (its id), its effort, and whether its name holds
//! the model and the effort already. A member is a participant who is neither the chief nor the human, runs
//! on a harness, and is not a session of one (it belongs to no member).

use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn handle(
    _context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let said = |field: Option<&str>| field.map_or(Value::Null, Value::from);
    let members: Vec<Value> = caller
        .project
        .participants
        .iter()
        .filter(|member| {
            member.role != "chief"
                && member.role != "human"
                && member.harness.is_some()
                && member.member_id.is_none()
        })
        .map(|member| {
            let mut row = json!({
                "handle": member.handle,
                "role": member.role,
                "roles": member.roles,
                // A member whose roles have no tier is said to have none (an image designer:
                // a design task has none).
                "tier": if cf_ledger::roles::takes_no_tier(&member.roles) {
                    None
                } else {
                    member.tier.as_deref()
                },
                "harness": member.harness,
                "harnessName": cf_ledger::handles::harness_label(member.harness.as_deref()),
                "model": said(member.model.as_deref()),
                "nameApart": member.name_apart,
                "modelName": if member.needs_edit {
                    cf_ledger::handles::NEEDS_AN_EDIT.to_owned()
                } else {
                    cf_ledger::handles::model_shown(
                        member.model_name.as_deref(),
                        member.model.as_deref(),
                        member.harness.as_deref(),
                    )
                },
                "effort": said(member.effort.as_deref()),
            });
            if let Some(owner) = member.backup_of.and_then(|id| {
                caller
                    .project
                    .participants
                    .iter()
                    .find(|other| other.id == id)
            }) {
                row["backupOf"] = json!(owner.handle);
            }
            if let Some(since) = &member.vacation_since {
                row["vacationSince"] = json!(since);
            }
            row
        })
        .collect();
    let mut answer = json!({ "members": members });
    if caller.project.own_hands {
        answer["ownHands"] = json!(true);
    }
    Ok(Answer::ok(answer))
}

#[cfg(test)]
mod tests;
