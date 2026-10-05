//! `GET /api/staff` (`api.js:123-143`): the members of the staff, each with
//! its roles, tier and harness, and the model and effort its saved agent has.
//! A member is a participant who is neither the chief nor the human (it has an
//! agent) and is not a session of one (it belongs to no member).

use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let mut members = Vec::new();
    for member in caller.project.participants.iter().filter(|member| {
        member.role != "chief" && member.agent.is_some() && member.member_id.is_none()
    }) {
        // The agent the human deleted has no row, and says no model.
        let row = match &member.agent {
            Some(agent) => context
                .roster
                .row(agent)
                .map_err(|unreadable| Failure::Internal(unreadable.message))?,
            None => None,
        };
        // Only `effort`: a Pi agent keeps its level as `thinking`, which this
        // route never read, so it says none.
        let said = |field: Option<&str>| field.map_or(Value::Null, Value::from);
        members.push(json!({
            "handle": member.handle,
            "role": member.role,
            "roles": member.roles,
            "tier": member.tier,
            "harness": member.harness,
            "model": said(row.as_ref().and_then(|row| row.model())),
            "effort": said(row.as_ref().and_then(|row| row.effort())),
        }));
    }
    Ok(Answer::ok(json!({ "members": members })))
}

#[cfg(test)]
mod tests;
