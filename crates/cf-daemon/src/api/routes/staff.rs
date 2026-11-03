//! `GET /api/staff`: the members of the staff, each with its roles and tier, the harness
//! it runs on and the name the page gives that harness, the model as its harness takes it
//! and the name its harness's list gives it, and its effort. A member is a participant who is neither the chief nor the human, runs
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
            json!({
                "handle": member.handle,
                "role": member.role,
                "roles": member.roles,
                "tier": member.tier,
                "harness": member.harness,
                "harnessName": cf_ledger::handles::harness_label(member.harness.as_deref()),
                "model": said(member.model.as_deref()),
                "modelName": cf_ledger::handles::model_shown(
                    member.model_name.as_deref(),
                    member.model.as_deref(),
                    member.harness.as_deref(),
                ),
                "effort": said(member.effort.as_deref()),
            })
        })
        .collect();
    Ok(Answer::ok(json!({ "members": members })))
}

#[cfg(test)]
mod tests;
