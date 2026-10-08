//! What the board page asks of the daemon, and what the daemon says first.
//!
//! The page names an operation and its body; the app forwards the 28 names
//! below and nothing else (`DAEMON_OPERATIONS` in `commands.rs`), and the
//! daemon answers each over the bridge (`pageOperations`, `src/core/page.js`).
//! The daemon's first line on its standard output is the [`HandleLine`] the
//! app reads to find it.

use serde::{Deserialize, Serialize};

/// One of the page's operations, in the order `src/core/page.js` lists them.
/// `ping`, which the daemon also answers, is no operation of the page's: the
/// app's check that the bridge is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageOperation {
    ProjectsList,
    ProjectOpen,
    ProjectResume,
    ProjectClose,
    ProjectDelete,
    AgentsList,
    StaffLast,
    ChiefSwitch,
    MemberAdd,
    MemberRoles,
    SessionOpen,
    SessionHide,
    SessionEnd,
    MemberRemove,
    BoardGet,
    InboxGet,
    TaskGet,
    TaskTranscript,
    ProjectGate,
    TaskCancel,
    TaskPause,
    TaskReassign,
    MemberBack,
    TaskResume,
    TasksDelete,
    MessageRead,
    MessageApprove,
    MessageDecline,
}

impl PageOperation {
    /// Every operation, in the order `page.js` lists them.
    pub const ALL: [PageOperation; 28] = [
        Self::ProjectsList,
        Self::ProjectOpen,
        Self::ProjectResume,
        Self::ProjectClose,
        Self::ProjectDelete,
        Self::AgentsList,
        Self::StaffLast,
        Self::ChiefSwitch,
        Self::MemberAdd,
        Self::MemberRoles,
        Self::SessionOpen,
        Self::SessionHide,
        Self::SessionEnd,
        Self::MemberRemove,
        Self::BoardGet,
        Self::InboxGet,
        Self::TaskGet,
        Self::TaskTranscript,
        Self::ProjectGate,
        Self::TaskCancel,
        Self::TaskPause,
        Self::TaskReassign,
        Self::MemberBack,
        Self::TaskResume,
        Self::TasksDelete,
        Self::MessageRead,
        Self::MessageApprove,
        Self::MessageDecline,
    ];

    /// The name the page, the app and the bridge say it by (`project.open`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectsList => "projects.list",
            Self::ProjectOpen => "project.open",
            Self::ProjectResume => "project.resume",
            Self::ProjectClose => "project.close",
            Self::ProjectDelete => "project.delete",
            Self::AgentsList => "agents.list",
            Self::StaffLast => "staff.last",
            Self::ChiefSwitch => "chief.switch",
            Self::MemberAdd => "member.add",
            Self::MemberRoles => "member.roles",
            Self::SessionOpen => "session.open",
            Self::SessionHide => "session.hide",
            Self::SessionEnd => "session.end",
            Self::MemberRemove => "member.remove",
            Self::BoardGet => "board.get",
            Self::InboxGet => "inbox.get",
            Self::TaskGet => "task.get",
            Self::TaskTranscript => "task.transcript",
            Self::ProjectGate => "project.gate",
            Self::TaskCancel => "task.cancel",
            Self::TaskPause => "task.pause",
            Self::TaskReassign => "task.reassign",
            Self::MemberBack => "member.back",
            Self::TaskResume => "task.resume",
            Self::TasksDelete => "tasks.delete",
            Self::MessageRead => "message.read",
            Self::MessageApprove => "message.approve",
            Self::MessageDecline => "message.decline",
        }
    }

    /// The operation a name says; none for any other word.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operation| operation.as_str() == name)
    }

    /// Whether the operation changes what the dispatcher acts on, so that
    /// once it has succeeded the daemon wakes the dispatcher (`change` in
    /// `page.js`): the human's change shows in the panes at once. One that
    /// only reads, or that failed, wakes nothing.
    pub const fn kicks(self) -> bool {
        !matches!(
            self,
            Self::ProjectsList
                | Self::AgentsList
                | Self::StaffLast
                | Self::BoardGet
                | Self::InboxGet
                | Self::TaskGet
                | Self::TaskTranscript
        )
    }
}

/// The first line the daemon prints (`{"url":"http://127.0.0.1:<port>/",
/// "token":"<48 hex>"}`): where its HTTP front listens and the token the app
/// opens the human's agents screens with. The url ends in a slash; a
/// window's `CONSENSFLOW_URL` does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleLine {
    pub url: String,
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_operations_are_the_twenty_eight_the_app_forwards() {
        let source = include_str!("../../../app/src-tauri/src/commands.rs");
        let list = source
            .split("const DAEMON_OPERATIONS: &[&str] = &[")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap();
        let mut forwarded: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        let mut named: Vec<&str> = PageOperation::ALL.map(PageOperation::as_str).to_vec();
        assert_eq!(named.len(), 28);
        forwarded.sort_unstable();
        named.sort_unstable();
        assert_eq!(named, forwarded);
    }

    #[test]
    fn a_name_says_one_operation_and_every_operation_has_one_name() {
        for operation in PageOperation::ALL {
            assert_eq!(
                PageOperation::from_name(operation.as_str()),
                Some(operation)
            );
        }
        for word in ["ping", "", "project", "Project.open", "project.open "] {
            assert_eq!(PageOperation::from_name(word), None, "{word:?}");
        }
    }

    /// The order Node's `pageOperations` listed them in, frozen when Node was
    /// the daemon: the page and the traces name the operations in it. A change
    /// to the list is a change to this one, made on purpose.
    #[test]
    fn the_list_is_the_order_node_listed_them_in_and_holds_each_operation_once() {
        const RECORDED: [&str; 28] = [
            "projects.list",
            "project.open",
            "project.resume",
            "project.close",
            "project.delete",
            "agents.list",
            "staff.last",
            "chief.switch",
            "member.add",
            "member.roles",
            "session.open",
            "session.hide",
            "session.end",
            "member.remove",
            "board.get",
            "inbox.get",
            "task.get",
            "task.transcript",
            "project.gate",
            "task.cancel",
            "task.pause",
            "task.reassign",
            "member.back",
            "task.resume",
            "tasks.delete",
            "message.read",
            "message.approve",
            "message.decline",
        ];
        let listed: Vec<&str> = PageOperation::ALL.map(PageOperation::as_str).to_vec();
        assert_eq!(listed, RECORDED);
    }

    #[test]
    fn the_operations_that_only_read_wake_nothing() {
        let reads: Vec<&str> = PageOperation::ALL
            .into_iter()
            .filter(|operation| !operation.kicks())
            .map(PageOperation::as_str)
            .collect();
        assert_eq!(
            reads,
            [
                "projects.list",
                "agents.list",
                "staff.last",
                "board.get",
                "inbox.get",
                "task.get",
                "task.transcript",
            ]
        );
    }

    #[test]
    fn the_handle_line_is_written_url_first_as_the_daemon_wrote_it() {
        let line = HandleLine {
            url: "http://127.0.0.1:43517/".to_owned(),
            token: "ab".repeat(24),
        };
        let written = serde_json::to_string(&line).unwrap();
        assert_eq!(
            written,
            format!(
                r#"{{"url":"http://127.0.0.1:43517/","token":"{}"}}"#,
                "ab".repeat(24)
            )
        );
        assert_eq!(serde_json::from_str::<HandleLine>(&written).unwrap(), line);
    }
}
