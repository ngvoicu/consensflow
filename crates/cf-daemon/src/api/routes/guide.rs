//! `GET /api/guide`: the board's guide, which only a chief reads: the chief's text for the
//! board made from the same `chief.md` as its ordinary role, with the staff as it is now and
//! the project's rules for its staff. A chief that does the work with its own hands is given a
//! short text and reads this when it first needs the board (`cf guide`).

use serde_json::json;

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    if caller.participant.role != "chief" {
        return Err(Failure::refuse(
            403,
            "not-the-chief",
            "the board's guide is the chief's to read",
        ));
    }
    let cf = (context.cf)(&caller.participant);
    let staff = cf_engine::roles::staff_of(&caller.project);
    let text =
        cf_engine::roles::guide_instructions(&staff, &cf, caller.project.staff_rules.as_deref())
            .map_err(|failed| Failure::Internal(failed.to_string()))?;
    Ok(Answer::ok(json!({ "text": text })))
}

#[cfg(test)]
mod tests;
