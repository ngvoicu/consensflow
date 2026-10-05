//! The table of the page's operations: which function serves which
//! (`pageOperations`, `src/core/page.js:13-190`). **Frozen**: a landing
//! replaces the arm of an operation with a call to its own module, and adds
//! nothing else here.
//!
//! Until an operation lands it answers an error that names it, so the page
//! shows which one is missing.

use cf_proto::page::PageOperation;
use serde_json::Value;

use super::{Page, Served};

/// Serves `operation`, asked with `body` (an object the page sent, or `{}`).
/// Its fields follow `ok: true` in the answer; its error is the words the page
/// shows.
pub(super) async fn serve(page: &Page, operation: PageOperation, body: Value) -> Served {
    let _ = (page, &body);
    match operation {
        // The projects.
        PageOperation::ProjectsList
        | PageOperation::ProjectOpen
        | PageOperation::ProjectResume
        | PageOperation::ProjectClose
        | PageOperation::ProjectDelete
        | PageOperation::ProjectGate
        // The agents and the staff.
        | PageOperation::AgentsList
        | PageOperation::StaffLast
        | PageOperation::ChiefSwitch
        | PageOperation::MemberAdd
        | PageOperation::MemberRoles
        | PageOperation::MemberRemove
        | PageOperation::MemberBack
        // The sessions' windows.
        | PageOperation::SessionOpen
        | PageOperation::SessionHide
        | PageOperation::SessionEnd
        // The board and what is on it.
        | PageOperation::BoardGet
        | PageOperation::InboxGet
        | PageOperation::TaskGet
        | PageOperation::TaskTranscript
        | PageOperation::TaskCancel
        | PageOperation::TaskPause
        | PageOperation::TaskReassign
        | PageOperation::TaskResume
        | PageOperation::TasksDelete
        | PageOperation::MessageRead
        | PageOperation::MessageApprove
        | PageOperation::MessageDecline => unserved(operation),
    }
}

/// An operation that has not landed: an error that names it.
fn unserved(operation: PageOperation) -> Served {
    Err(format!(
        "the page operation {} is not served by this daemon yet",
        operation.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operation_not_landed_says_which_one_it_is() {
        for operation in PageOperation::ALL {
            let words = unserved(operation).unwrap_err();
            assert!(words.contains(operation.as_str()), "{words}");
        }
    }
}
