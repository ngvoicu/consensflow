//! Who is asking: the window's token, checked. **Frozen**.
//!
//! The API decides who may do what; the ledger keeps the state rules. A
//! request is a window's only if it carries the bearer token the engine issued
//! that window, and the participant it names is still in the project. The
//! views are copied out: the borrow of the ledger is let go before the handler
//! goes on, so the handler may await its body.

use cf_ledger::{ParticipantView, ProjectView, WindowHolding};

use super::answer::Failure;
use super::context::Context;
use super::request::Request;

/// The participant whose window made a request, and the project it is in, as
/// the ledger had them when the token was checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Caller {
    pub project: ProjectView,
    pub participant: ParticipantView,
    /// The holding when this question request was authenticated.
    pub question_holding: Option<WindowHolding>,
    /// The task whose brief this window was launched with, if any.
    pub question_launch_task: Option<(i64, i64)>,
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
        (Some(project), Some(participant)) => {
            let mut question_launch_task = None;
            let question_holding = if request.method == hyper::Method::POST
                && request.path == "/api/questions"
                && participant.role != "chief"
            {
                let mut ledger = context.ledger.borrow_mut();
                if let Some(start) = identity.question_start {
                    if let Some(brief) = start.launch_brief {
                        if let Some(message) = ledger.message(brief)? {
                            if message.project_id == project.id
                                && message.recipient_id == participant.id
                                && message.kind == "task"
                            {
                                if let Some(number) = message.task_number {
                                    if let Some(task) = ledger.task(project.id, number)? {
                                        if start.task_in_hand == Some(task.task.id) {
                                            question_launch_task = Some((task.task.id, number));
                                            // A request with this token proves that the
                                            // process launched with this brief ran.
                                            let prior = ledger.question_holding(participant.id)?;
                                            let later_brief = prior.is_some_and(|h| {
                                                h.last_brief != brief
                                                    && Some(h.last_brief) != start.last_brief
                                            });
                                            if !later_brief {
                                                ledger.question_launch_handed_over(brief)?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(start) = identity.question_start {
                    if let Some(task) = start.task_in_hand {
                        if start.launch_brief.is_none()
                            && start.last_brief == Some(-task)
                            && ledger.question_holding(participant.id)?.is_none()
                        {
                            if let Some((number, resumed)) =
                                ledger.resume_question_holding_for_task(participant.id, task)?
                            {
                                if !resumed {
                                    question_launch_task = Some((task, number));
                                }
                            }
                        }
                    }
                }
                let holding = ledger.question_holding(participant.id)?;
                if let (Some(start), Some(held)) = (identity.question_start, holding) {
                    if held.resumable
                        && start.last_brief == Some(held.last_brief)
                        && start.task_in_hand == Some(held.task_id)
                    {
                        // A request with this newly issued window token proves
                        // the resumed process started, even if the pane host's
                        // answer to open has not arrived yet. The ledger still
                        // checks that the task has not moved meanwhile.
                        ledger.resume_question_holding(participant.id, held.task_number)?;
                    }
                }
                let holding = ledger.question_holding(participant.id)?;
                if let (Some(start), Some(h)) = (identity.question_start, holding) {
                    if start.launch_brief.is_some_and(|brief| {
                        h.last_brief != brief && Some(h.last_brief) != start.last_brief
                    }) {
                        // A later handed brief supersedes the launch's task.
                        question_launch_task = None;
                    }
                }
                holding.filter(|h| match identity.question_start {
                    Some(start) => {
                        start.last_brief != Some(h.last_brief)
                            || start.task_in_hand == Some(h.task_id)
                    }
                    None => true,
                })
            } else {
                None
            };
            Ok(Caller {
                project,
                participant,
                question_holding,
                question_launch_task,
            })
        }
        _ => Err(Failure::refuse(
            401,
            "unauthorized",
            "this window belongs to a project that no longer exists",
        )),
    }
}
