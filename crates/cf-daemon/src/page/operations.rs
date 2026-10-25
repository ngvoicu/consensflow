//! The table of the page's operations: which function serves which. An
//! operation is read and begun in its first poll, in the order its frame came:
//! each arm reads the saved agents and the body, makes its one call on the
//! ledger or the engine, and answers; none waits before it makes it.

use cf_proto::page::PageOperation;
use serde_json::Value;

use super::body::Body;
use super::{
    app_specs, board, history, messages, projects, sessions, specs, staff, tasks, viewer, wake_ups,
    Page, Served,
};

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
        PageOperation::ProjectRename => projects::rename(page, body).await,
        PageOperation::ProjectDescribe => projects::describe(page, body).await,
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
        PageOperation::TaskFinish => tasks::finish(page, body).await,
        PageOperation::TaskResume => tasks::resume(page, body).await,
        PageOperation::TasksDelete => tasks::delete(page, body).await,
        PageOperation::MessageRead => messages::read(page, body).await,
        PageOperation::MessageApprove => messages::approve(page, body).await,
        PageOperation::MessageDecline => messages::decline(page, body).await,
        // The human's wake-up calls.
        PageOperation::WakeUpGet => wake_ups::get(page, body).await,
        PageOperation::WakeUpSet => wake_ups::set(page, body).await,
        // What a project says of specs, and what the app says for every project.
        PageOperation::ProjectSpecs => specs::set(page, body).await,
        PageOperation::SpecsGet => app_specs::get(page).await,
        PageOperation::SpecsSet => app_specs::set(page, body).await,
        // What a project's specs folder holds, for the specs viewer.
        PageOperation::SpecsTree => viewer::tree(page, body).await,
        PageOperation::SpecsFile => viewer::file(page, body).await,
        PageOperation::SpecsPresent => viewer::present(page, body).await,
        PageOperation::SpecsOpen => viewer::open(page, body).await,
        // A participant's whole history, for the viewer of its window's History button.
        PageOperation::HistoryGet => history::get(page, body).await,
        PageOperation::HistoryPart => history::part(page, body).await,
    };
    served.map_err(|said| said.0)
}
