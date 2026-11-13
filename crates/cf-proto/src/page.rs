//! What the board page asks of the daemon, and what the daemon says first.
//!
//! The page names an operation and its body; the app forwards the names
//! below and nothing else (`DAEMON_OPERATIONS` in `commands.rs`), and the
//! daemon answers each over the bridge. The daemon's first line on its standard
//! output is the [`HandleLine`] the app reads to find it.

use serde::{Deserialize, Serialize};

/// One of the page's operations, in the order Node's page listed them, and
/// after them the ones this build added. `ping`, which the daemon also answers,
/// is no operation of the page's: the app's check that the bridge is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageOperation {
    ProjectsList,
    ProjectOpen,
    ProjectResume,
    ProjectClose,
    ProjectDelete,
    /// Node's `agents.list`: the harnesses not installed here, which the pickers
    /// leave out (no saved agents are listed: harness-models).
    HarnessesMissing,
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
    WakeUpGet,
    WakeUpSet,
    /// What a project says of specs: whether they are on, the folder and the
    /// Specmint variant its chief plans with.
    ProjectSpecs,
    /// The name the human gives a project in place of the one it was opened with.
    ProjectRename,
    /// The line the human writes under a project's name, or takes away.
    ProjectDescribe,
    /// What the app says of specs for every project (whether, the folder and the
    /// variant), and the Specmint this build bundles.
    SpecsGet,
    /// The human's word on specs for every project: each part given is kept.
    SpecsSet,
    /// What a project's specs folder holds, for the specs viewer: its files
    /// and folders, read and never written.
    SpecsTree,
    /// One file of a project's specs folder, for the specs viewer, read and
    /// never written.
    SpecsFile,
    /// Whether specs are on for a project and whether its specs folder holds a
    /// file: either shows the board's Specs button.
    SpecsPresent,
    /// An HTML file of a project's specs folder, opened in the browser,
    /// outside ConsensFlow.
    SpecsOpen,
    /// The human takes what the worker of a working task said last as its result,
    /// ending nothing it left running (Finish, in the task's drawer).
    TaskFinish,
    /// A participant's whole history, every conversation and the current one,
    /// a page at a time: the viewer of a window's History button, read and
    /// never written.
    HistoryGet,
    /// A long item of a participant's history, a part at a time, for the
    /// viewer to show it whole: read and never written.
    HistoryPart,
    /// A project's computer use, for Project settings: the system, ConsensFlow's
    /// macOS grants, and each harness's word for the project's folder, read and
    /// never written.
    ComputerUseGet,
    /// One of the System Settings panes where the human gives ConsensFlow a grant,
    /// opened outside ConsensFlow.
    ComputerUseOpen,
    /// The human starts or stops a meeting with the project's chief (Start
    /// meeting, Stop meeting): while one is on, what is queued for the chief waits.
    ProjectMeeting,
    /// The rules the human writes for a project's staff, or takes away: every
    /// window of the project is given them when it next starts.
    ProjectRules,
    /// The size the human chooses for a project's Claude member windows to
    /// compact at, or takes away: every member window of the project is given it
    /// when it next starts.
    ProjectCompaction,
    /// Whether the human has a project's members run with Claude Code's own advisor tool
    /// (Project settings, "Advisor tool"): every member's Claude Code window is launched with
    /// it switched off or on from its next start; the chief's never is switched.
    ProjectAdvisorTool,
    /// Whether the human leaves Claude Code's prompt suggestion on in a project's windows
    /// (Project settings, "Prompt suggestion"): every Claude Code window of the project, the
    /// chief's and each member's, is launched with it switched off or left to the owner's own
    /// setting from its next start.
    ProjectPromptSuggestion,
    /// The models a harness lists and the efforts each takes, as the home keeps them: read
    /// from the harness at the first ask of the daemon's life, and again at the human's
    /// Refresh (CHF-03). For the chief's picker and the staff chains' editor.
    ModelsGet,
    /// Edit member: a member runs on another harness, model and effort from its next window,
    /// in the tier the human chose (T-1143).
    MemberEdit,
    /// Add, replace or remove the backup of a staff member.
    BackupAdd,
    BackupChange,
    BackupRemove,
}

impl PageOperation {
    /// Every operation, in the order Node's page listed them, then those this
    /// build added, in the order it added them.
    pub const ALL: [PageOperation; 54] = [
        Self::ProjectsList,
        Self::ProjectOpen,
        Self::ProjectResume,
        Self::ProjectClose,
        Self::ProjectDelete,
        Self::HarnessesMissing,
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
        Self::WakeUpGet,
        Self::WakeUpSet,
        Self::ProjectSpecs,
        Self::ProjectRename,
        Self::ProjectDescribe,
        Self::SpecsGet,
        Self::SpecsSet,
        Self::SpecsTree,
        Self::SpecsFile,
        Self::SpecsPresent,
        Self::SpecsOpen,
        Self::TaskFinish,
        Self::HistoryGet,
        Self::HistoryPart,
        Self::ComputerUseGet,
        Self::ComputerUseOpen,
        Self::ProjectMeeting,
        Self::ProjectRules,
        Self::ProjectCompaction,
        Self::ProjectAdvisorTool,
        Self::ProjectPromptSuggestion,
        Self::ModelsGet,
        Self::MemberEdit,
        Self::BackupAdd,
        Self::BackupChange,
        Self::BackupRemove,
    ];

    /// The name the page, the app and the bridge say it by (`project.open`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectsList => "projects.list",
            Self::ProjectOpen => "project.open",
            Self::ProjectResume => "project.resume",
            Self::ProjectClose => "project.close",
            Self::ProjectDelete => "project.delete",
            Self::HarnessesMissing => "harnesses.missing",
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
            Self::WakeUpGet => "wakeup.get",
            Self::WakeUpSet => "wakeup.set",
            Self::ProjectSpecs => "project.specs",
            Self::ProjectRename => "project.rename",
            Self::ProjectDescribe => "project.describe",
            Self::SpecsGet => "specs.get",
            Self::SpecsSet => "specs.set",
            Self::SpecsTree => "specs.tree",
            Self::SpecsFile => "specs.file",
            Self::SpecsPresent => "specs.present",
            Self::SpecsOpen => "specs.open",
            Self::TaskFinish => "task.finish",
            Self::HistoryGet => "history.get",
            Self::HistoryPart => "history.part",
            Self::ComputerUseGet => "computer_use.get",
            Self::ComputerUseOpen => "computer_use.open",
            Self::ProjectMeeting => "project.meeting",
            Self::ProjectRules => "project.rules",
            Self::ProjectCompaction => "project.compaction",
            Self::ProjectAdvisorTool => "project.advisor_tool",
            Self::ProjectPromptSuggestion => "project.prompt_suggestion",
            Self::ModelsGet => "models.get",
            Self::MemberEdit => "member.edit",
            Self::BackupAdd => "backup.add",
            Self::BackupChange => "backup.change",
            Self::BackupRemove => "backup.remove",
        }
    }

    /// The operation a name says; none for any other word.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operation| operation.as_str() == name)
    }

    /// Whether the operation changes what the dispatcher acts on, so that once
    /// it has succeeded the daemon wakes the dispatcher: the human's change
    /// shows in the panes at once. One that only reads, or that failed, wakes
    /// nothing.
    pub const fn kicks(self) -> bool {
        !matches!(
            self,
            Self::ProjectsList
                | Self::HarnessesMissing
                | Self::StaffLast
                | Self::BoardGet
                | Self::InboxGet
                | Self::SpecsTree
                | Self::SpecsFile
                | Self::SpecsPresent
                | Self::SpecsOpen
                | Self::TaskGet
                | Self::TaskTranscript
                | Self::WakeUpGet
                | Self::SpecsGet
                | Self::HistoryGet
                | Self::HistoryPart
                | Self::ComputerUseGet
                | Self::ComputerUseOpen
                | Self::ModelsGet
        )
    }
}

/// The first line the daemon prints (`{"url":"http://127.0.0.1:<port>/",
/// "token":"<48 hex>"}`): where its HTTP front listens and the token the app
/// opens the human's harnesses screen with. The url ends in a slash; a
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
    fn the_operations_are_the_ones_the_app_forwards() {
        let source = include_str!("../../../app/src-tauri/src/commands.rs");
        let list = source
            .split("const DAEMON_OPERATIONS: &[&str] = &[")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap();
        let mut forwarded: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        let mut named: Vec<&str> = PageOperation::ALL.map(PageOperation::as_str).to_vec();
        assert_eq!(named.len(), 54);
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
    /// to the list is a change to this one, made on purpose; what this build
    /// added follows, in the order it was added.
    #[test]
    fn the_list_is_the_order_node_listed_them_in_then_what_was_added_and_holds_each_once() {
        const RECORDED: [&str; 28] = [
            "projects.list",
            "project.open",
            "project.resume",
            "project.close",
            "project.delete",
            // Node's `agents.list`, renamed with the saved agents gone (harness-models).
            "harnesses.missing",
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
        const ADDED: [&str; 26] = [
            "wakeup.get",
            "wakeup.set",
            "project.specs",
            "project.rename",
            "project.describe",
            "specs.get",
            "specs.set",
            "specs.tree",
            "specs.file",
            "specs.present",
            "specs.open",
            "task.finish",
            "history.get",
            "history.part",
            "computer_use.get",
            "computer_use.open",
            "project.meeting",
            "project.rules",
            "project.compaction",
            "project.advisor_tool",
            "project.prompt_suggestion",
            "models.get",
            "member.edit",
            "backup.add",
            "backup.change",
            "backup.remove",
        ];
        let listed: Vec<&str> = PageOperation::ALL.map(PageOperation::as_str).to_vec();
        assert_eq!(listed[..RECORDED.len()], RECORDED);
        assert_eq!(listed[RECORDED.len()..], ADDED);
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
                "harnesses.missing",
                "staff.last",
                "board.get",
                "inbox.get",
                "task.get",
                "task.transcript",
                "wakeup.get",
                "specs.get",
                "specs.tree",
                "specs.file",
                "specs.present",
                "specs.open",
                "history.get",
                "history.part",
                "computer_use.get",
                "computer_use.open",
                "models.get",
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
