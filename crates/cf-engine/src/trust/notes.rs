//! What the trust watch tells the human: each note once for the windows that ask
//! the same of one folder, and the words of each.

use cf_harness::contract::Question;
use cf_ledger::{NewNote, ParticipantView, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::handoff::is_handoff;
use crate::record::Record;
use crate::seams::EngineError;

impl Dispatcher {
    /// An added-directory question the watch could not answer.
    pub(super) fn tell_added_held(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        self.note_to_human(
            project,
            format!(
                "@{}'s window asks about a directory ConsensFlow added, but ConsensFlow could not answer it. Answer it in that window; its work will go on afterwards.",
                participant.handle
            ),
        )
    }

    /// The note for a question ConsensFlow answered yes to in the project's folder: the
    /// first time the project's windows ask of `folder`, in the words of the question
    /// asked (the grants of the folder's settings, or the folder's trust alone).
    pub(super) fn tell_yes_pressed(
        &self,
        project: &ProjectView,
        folder: &str,
        participant: &ParticipantView,
        question: Question,
    ) -> Result<(), EngineError> {
        self.tell_trust_once(project, folder, true, || {
            if matches!(question, Question::Grants(_)) {
                format!(
                    "@{}'s window asked whether to trust {folder} and let the settings of the project's folder {} apply without asking; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                    participant.handle, project.directory
                )
            } else {
                format!(
                    "@{}'s window asked whether to trust {folder}; ConsensFlow answered yes, as the project's folder is trusted by choosing it.",
                    participant.handle
                )
            }
        })
    }

    /// The note for the first question of a folder that stayed on a screen and that
    /// ConsensFlow did not answer: the human is to.
    pub(super) fn tell_trust_held(
        &self,
        project: &ProjectView,
        folder: &str,
        inside: bool,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        // Once a project and folder: a conversation that quotes the
        // question for longer (under load) is told as asking, and a
        // real question of the same folder in another window then
        // gets no note of its own, only its lane.
        self.tell_trust_once(project, folder, false, || {
            if inside {
                format!(
                    "@{}'s window asks whether to trust {folder}, and ConsensFlow could not answer it. Answer it in that window: what waits for @{} goes on by itself once you have.",
                    participant.handle, participant.handle
                )
            } else {
                format!(
                    "@{}'s window asks whether to trust {folder}, which is outside the project's folder {}. Answer it in that window: what waits for @{} goes on by itself once you have.",
                    participant.handle, project.directory, participant.handle
                )
            }
        })
    }

    /// A note to the human about the question of `folder`'s grants that
    /// `participant`'s window asks, which ConsensFlow does not answer: `folder`, which a
    /// yes records, is outside the project's folder, or, `inside` it, the question's
    /// choice could not be placed. The first time the project's windows ask it since the
    /// last that did was past it.
    pub(super) fn tell_grants_once(
        &self,
        project: &ProjectView,
        folder: &str,
        inside: bool,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        if !self
            .trust
            .told_grants
            .borrow_mut()
            .insert((project.id, folder.to_owned()))
        {
            return Ok(());
        }
        let handle = &participant.handle;
        self.note_to_human(
            project,
            format!(
                "@{handle}'s window asks whether the settings of its folder {directory} may apply without asking. Claude Code takes the folder as trusted, by a folder above it or by a record of its own, but has none for {folder}, which its yes records as trusted (the root of the repository the folder is in, or the folder itself out of one), and the folder's own settings grant more than its trust: tools they pre-approve, folders they add, or commands that mint the headers of MCP servers. {why} Answer it in that window: its no continues without them, its yes trusts {folder} and lets them apply. What waits for @{handle} goes on by itself once you have.",
                directory = project.directory,
                why = if inside {
                    "ConsensFlow could not answer it.".to_owned()
                } else {
                    format!("{folder} is outside the project's folder, so ConsensFlow does not answer it.")
                },
            ),
        )
    }

    /// That the human was told of the question of `folder`'s grants no longer: the
    /// next window that asks it is told of again.
    pub(super) fn forget_grants_told(&self, project: &ProjectView, folder: &str) {
        self.trust
            .told_grants
            .borrow_mut()
            .remove(&(project.id, folder.to_owned()));
    }

    /// That the human was told of the dialog of new servers of `folder` no longer: the
    /// next window that asks it is told of again.
    pub(super) fn forget_servers_told(&self, project: &ProjectView, folder: &str) {
        self.trust
            .told_servers
            .borrow_mut()
            .remove(&(project.id, folder.to_owned()));
    }

    /// A note to the human about the dialog of new servers that `participant`'s window
    /// asks, which ConsensFlow does not answer: `folder`, which a yes to the first
    /// question records, is outside the project's folder, or a folder above the
    /// project's has servers of its own (`servers_above`), or, neither, its choice could
    /// not be placed. The first time the project's windows ask it since the last that
    /// did was past it.
    pub(super) fn tell_servers_held(
        &self,
        project: &ProjectView,
        folder: &str,
        inside: bool,
        servers_above: Option<&str>,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        let why = match (inside, servers_above) {
            (false, _) => format!(
                "The project's folder {} is in the repository {folder}, which is outside it, so ConsensFlow does not answer it.",
                project.directory
            ),
            (true, Some(above)) => format!(
                "{above}, above the project's folder, has a .mcp.json of its own, whose servers Claude Code asks of in the same dialog, so ConsensFlow does not answer it."
            ),
            (true, None) => "ConsensFlow could not answer it.".to_owned(),
        };
        if !self
            .trust
            .told_servers
            .borrow_mut()
            .insert((project.id, folder.to_owned()))
        {
            return Ok(());
        }
        self.note_to_human(
            project,
            format!(
                "@{handle}'s window asks whether to use a new MCP server that its folder's .mcp.json, or a folder above it, names: a server it uses starts its command on this machine. {why} Answer it in that window: \"Use this MCP server\" uses it, \"Continue without using this MCP server\" goes on without it. What waits for @{handle} goes on by itself once you have.",
                handle = participant.handle
            ),
        )
    }

    /// Notes to the human that ConsensFlow had `participant`'s window use the servers
    /// `named` (as the dialog of new servers named them, a plugin's with its plugin),
    /// each the first time it was used in `folder`; one of a server it could not name.
    pub(super) fn tell_used(
        &self,
        project: &ProjectView,
        folder: &str,
        participant: &ParticipantView,
        named: Vec<String>,
    ) -> Result<(), EngineError> {
        let named = if named.is_empty() {
            vec![String::new()]
        } else {
            named
        };
        for name in named {
            if !self
                .trust
                .used
                .borrow_mut()
                .insert((project.id, folder.to_owned(), name.clone()))
            {
                continue;
            }
            let server = if name.is_empty() {
                "a new MCP server".to_owned()
            } else {
                format!("the MCP server {name}")
            };
            self.note_to_human(
                project,
                format!(
                    "@{}'s window asked whether to use {server}, which the project's .mcp.json names; ConsensFlow said to use it, as the project's folder is trusted by choosing it. Claude Code starts the server's command, and keeps the choice in {folder}/.claude/settings.local.json.",
                    participant.handle
                ),
            )?;
        }
        Ok(())
    }

    /// A note to the human that a server the window's folder left out while only
    /// ConsensFlow pressed into the dialog of new servers: a key that answered what it
    /// was not pressed for (T-415). No later window asks of it, so the human is told.
    pub(super) fn tell_left_out(
        &self,
        project: &ProjectView,
        folder: &str,
        participant: &ParticipantView,
        refused: &[String],
    ) -> Result<(), EngineError> {
        self.note_to_human(
            project,
            format!(
                "@{}'s window left the MCP server {} out of the project's while ConsensFlow pressed toward using it: Claude Code wrote it in disabledMcpjsonServers of {folder}/.claude/settings.local.json, and no later window asks of it. If the project is to use it, take it out of that list.",
                participant.handle,
                refused.join(", ")
            ),
        )
    }

    /// The note to the human that a window whose host showed no screen of it, once it
    /// asked, could not be read for `unseen_ms`: nothing goes into it until it can.
    pub(super) fn tell_unread(
        &self,
        project: &ProjectView,
        folder: &str,
        participant: &ParticipantView,
        unseen_ms: i64,
    ) -> Result<(), EngineError> {
        let body = format!(
            "@{}'s window asked whether to trust {folder}, and its screen could not be read for {} s since. Nothing goes into it until its screen can be read and shows it past the question; if it still asks, answer it in that window.",
            participant.handle,
            unseen_ms / 1_000
        );
        self.note_to_human(project, body)
    }

    /// A note to the human that says `body`.
    fn note_to_human(&self, project: &ProjectView, body: String) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The note to the human that the chief's window, owed its first message, has shown
    /// no prompt for [`NO_PROMPT_MS`](super::NO_PROMPT_MS): the message waits, and the window is theirs to look
    /// at (T-629).
    pub(super) fn tell_no_prompt(
        &self,
        project: &ProjectView,
        record: &Record,
    ) -> Result<(), EngineError> {
        let owed = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .map(|delivering| delivering.message);
        let message = match owed {
            Some(id) => self.seams.ledger.borrow().message(id)?,
            None => None,
        };
        let what = if message.as_ref().is_some_and(is_handoff) {
            "its handoff"
        } else {
            "the message for it"
        };
        self.note_to_human(
            project,
            format!(
                "The chief's window has shown no prompt for 2 minutes; {what} is waiting. Look at its terminal."
            ),
        )
    }

    /// A note to the human about `folder`, the first time the project's windows
    /// ask of it: that ConsensFlow `answered` it, or that the human is to.
    fn tell_trust_once(
        &self,
        project: &ProjectView,
        folder: &str,
        answered: bool,
        body: impl FnOnce() -> String,
    ) -> Result<(), EngineError> {
        if !self
            .trust
            .told
            .borrow_mut()
            .insert((project.id, folder.to_owned(), answered))
        {
            return Ok(());
        }
        self.note_to_human(project, body())
    }
}
