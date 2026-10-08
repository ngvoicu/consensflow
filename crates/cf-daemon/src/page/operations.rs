//! The table of the page's operations: which function serves which. An
//! operation is read and begun in its first poll, in the order its frame came:
//! each arm reads the saved agents and the body, makes its one call on the
//! ledger or the engine, and answers; none waits before it makes it.

use cf_proto::page::PageOperation;
use serde_json::Value;

use super::body::Body;
use super::{board, messages, projects, sessions, staff, tasks, Page, Served};

/// Serves `operation`, asked with `body` (an object the page sent, or `{}`).
/// Its fields follow `ok: true` in the answer; its error is the words the page
/// shows.
pub(super) async fn serve(page: &Page, operation: PageOperation, body: Value) -> Served {
    let body = Body::new(&body);
    let served = match operation {
        // The projects.
        PageOperation::ProjectsList => projects::list(page).await,
        PageOperation::ProjectOpen => projects::open(page, body).await,
        PageOperation::ProjectResume => projects::resume(page, body).await,
        PageOperation::ProjectClose => projects::close(page, body).await,
        PageOperation::ProjectDelete => projects::delete(page, body).await,
        PageOperation::ProjectGate => projects::gate(page, body).await,
        // The agents and the staff.
        PageOperation::AgentsList => staff::agents(page).await,
        PageOperation::StaffLast => staff::last(page).await,
        PageOperation::ChiefSwitch => staff::switch_chief(page, body).await,
        PageOperation::MemberAdd => staff::add(page, body).await,
        PageOperation::MemberRoles => staff::roles(page, body).await,
        PageOperation::MemberRemove => staff::remove(page, body).await,
        PageOperation::MemberBack => staff::back(page, body).await,
        // The sessions' windows.
        PageOperation::SessionOpen => sessions::open(page, body).await,
        PageOperation::SessionHide => sessions::hide(page, body).await,
        PageOperation::SessionEnd => sessions::end(page, body).await,
        // The board and what is on it.
        PageOperation::BoardGet => board::get(page, body).await,
        PageOperation::InboxGet => board::inbox(page, body).await,
        PageOperation::TaskGet => tasks::get(page, body).await,
        PageOperation::TaskTranscript => tasks::transcript(page, body).await,
        PageOperation::TaskCancel => tasks::cancel(page, body).await,
        PageOperation::TaskPause => tasks::pause(page, body).await,
        PageOperation::TaskReassign => tasks::reassign(page, body).await,
        PageOperation::TaskResume => tasks::resume(page, body).await,
        PageOperation::TasksDelete => tasks::delete(page, body).await,
        PageOperation::MessageRead => messages::read(page, body).await,
        PageOperation::MessageApprove => messages::approve(page, body).await,
        PageOperation::MessageDecline => messages::decline(page, body).await,
    };
    served.map_err(|said| said.0)
}
