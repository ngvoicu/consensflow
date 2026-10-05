//! Who is asking: the window's token, checked (`callerOf`,
//! `src/core/api.js:405-426`). **Frozen**.
//!
//! The API decides who may do what; the ledger keeps the state rules. A
//! request is a window's only if it carries the bearer token the engine issued
//! that window, and the participant it names is still in the project. The
//! views are copied out: the borrow of the ledger is let go before the handler
//! goes on, so the handler may await its body.

use cf_ledger::{ParticipantView, ProjectView};

use super::answer::Failure;
use super::context::Context;
use super::request::Request;

/// The participant whose window made a request, and the project it is in, as
/// the ledger had them when the token was checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Caller {
    pub project: ProjectView,
    pub participant: ParticipantView,
}

/// The window behind `request`: 401 `unauthorized` for a request with no
/// token, or one that was never issued or has been revoked ("this window has
/// no ConsensFlow access (it may have closed)"), and for one whose project or
/// participant is gone ("this window belongs to a project that no longer
/// exists").
pub fn caller_of(context: &Context, request: &Request) -> Result<Caller, Failure> {
    let Some(identity) = context.credentials.resolve(request.bearer()) else {
        return Err(Failure::refuse(
            401,
            "unauthorized",
            "this window has no ConsensFlow access (it may have closed)",
        ));
    };
    let project = context.ledger.borrow().project(identity.project_id)?;
    let participant = project.as_ref().and_then(|found| {
        found
            .participants
            .iter()
            .find(|candidate| candidate.id == identity.participant_id)
            .cloned()
    });
    match (project, participant) {
        (Some(project), Some(participant)) => Ok(Caller {
            project,
            participant,
        }),
        _ => Err(Failure::refuse(
            401,
            "unauthorized",
            "this window belongs to a project that no longer exists",
        )),
    }
}
