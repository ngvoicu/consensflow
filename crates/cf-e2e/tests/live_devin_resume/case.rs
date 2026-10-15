//! The cases of the check, and the task each begins with.

use cf_e2e::live::window::Member;

use crate::asks::TASK;
use crate::words::{cut_note, delivery, ROLE};
use crate::LONG;

/// The number of the message that tells the window its turn was cut: the daemon
/// numbers a task's messages as it writes them.
pub const NOTE: u32 = 2;

/// A case of the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// The window is killed at rest, after a turn that wrote a file with its shell.
    AtRest,
    /// The window is killed in the middle of a tool call: its shell runs a long
    /// command.
    MidTool,
    /// As `MidTool`, and the window is opened again with the note that its turn was
    /// cut as its first message, in the prompt file of its launch: how the daemon
    /// gives a member a note when the human resumes the project.
    AfterNote,
    /// As `MidTool`, and the note is pasted into the window once it has drawn, as
    /// the daemon pastes a message it gives a window that is open.
    AfterNotePasted,
}

impl Case {
    pub const ALL: [Case; 4] = [
        Case::AtRest,
        Case::MidTool,
        Case::AfterNote,
        Case::AfterNotePasted,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Case::AtRest => "at-rest",
            Case::MidTool => "mid-tool",
            Case::AfterNote => "after-note",
            Case::AfterNotePasted => "after-note-pasted",
        }
    }

    /// Whether the window is cut inside a command.
    pub const fn in_command(self) -> bool {
        !matches!(self, Case::AtRest)
    }

    /// The first message of the window, as the daemon gives a member its task: the
    /// shell writes a file (which proves it may, before anything cuts it), and in the
    /// cases that cut a command the window then runs the long one.
    pub fn first_message(self, tag: &str) -> String {
        let write = format!("echo first-{tag} > first-{tag}.txt");
        let body = if self.in_command() {
            format!(
                "Do these two steps in order, with your shell tool.\nStep 1: run this exact command: {write}\nStep 2: run this exact command, wait for it to finish, and then reply with exactly one line: FINISHED\n{}",
                LONG.text()
            )
        } else {
            format!(
                "Run this exact command with your shell tool: {write}\nThen reply with exactly one line: FIRST_DONE"
            )
        };
        delivery(1, TASK, "task", "@chief", &body)
    }

    /// The window the case opens again: a member as the first was, and, where the
    /// case has the note be its first message, with it in the prompt file of its
    /// launch.
    pub fn member_again(self) -> Member {
        let member = Member::new("worker", ROLE);
        if self == Case::AfterNote {
            member.first_message(&cut_note(NOTE, TASK))
        } else {
            member
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_are_named_as_they_are_asked_for_and_only_the_first_is_cut_at_rest() {
        let names: Vec<&str> = Case::ALL.iter().map(|case| case.name()).collect();
        assert_eq!(
            names,
            ["at-rest", "mid-tool", "after-note", "after-note-pasted"]
        );
        let in_command: Vec<bool> = Case::ALL.iter().map(|case| case.in_command()).collect();
        assert_eq!(in_command, [false, true, true, true]);
    }

    #[test]
    fn the_window_opened_again_has_the_note_as_its_first_message_only_where_the_case_gives_it_so() {
        for case in [Case::AtRest, Case::MidTool, Case::AfterNotePasted] {
            assert_eq!(case.member_again().message(), None, "{}", case.name());
        }
        assert_eq!(
            Case::AfterNote.member_again().message(),
            Some("[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.")
        );
    }

    #[test]
    fn the_task_a_window_starts_with_is_a_file_with_its_shell_and_where_it_is_cut_in_a_command_the_long_one(
    ) {
        let rest = Case::AtRest.first_message("ab12");
        assert_eq!(
            rest,
            "[ConsensFlow m-1 · T-1 · task from @chief]\nRun this exact command with your shell tool: echo first-ab12 > first-ab12.txt\nThen reply with exactly one line: FIRST_DONE"
        );
        for case in [Case::MidTool, Case::AfterNote, Case::AfterNotePasted] {
            let task = case.first_message("ab12");
            assert!(
                task.starts_with("[ConsensFlow m-1 · T-1 · task from @chief]\n"),
                "{task}"
            );
            assert!(task.contains("echo first-ab12 > first-ab12.txt"), "{task}");
            assert!(
                task.contains("reply with exactly one line: FINISHED\nnode -e "),
                "{task}"
            );
            assert!(task.ends_with("cf-devin-resume-long-command"), "{task}");
        }
    }
}
