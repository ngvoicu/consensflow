//! The cases of the check: where the first window is cut, how the window opened again
//! begins, and what is done to it before the asks.
//!
//! Since B1's fix the daemon puts a resumed window's first message behind `/bypass`,
//! and readies a window its wire says is in the normal mode with `/bypass` before it
//! gives it anything. `after-note` and `mid-tool` have the product's way whole: the
//! note is the engine's, the launch the adapter's (which puts the note behind
//! `/bypass`), and before the asks the daemon's readiness is asked what it does
//! (`Step::Ready`). The other cases are as they were, the evidence of what Devin
//! does: their asks are pasted with no readiness before them, and the note behind
//! the check's own `/bypass` now has the adapter's before it too.

use cf_e2e::live::window::Member;

use crate::asks::TASK;
use crate::words::{delivery, ROLE};
use crate::LONG;

/// The numbers of the messages the daemon writes for a task, as it numbers them: the
/// note that its turn was cut, and the second task of a window that had a turn before.
pub const NOTE: u32 = 2;
pub const SECOND: u32 = 5;

/// What switches a Devin window to full permission, as the window's own commands list
/// it: with a prompt behind it, the prompt runs under it.
pub const BYPASS: &str = "/bypass";

/// Where the first window is cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// At rest, after a turn that wrote a file with its shell.
    AtRest,
    /// In its first turn: its shell runs a long command.
    InFirstTurn,
    /// In its second turn, after a first that ended: its shell runs a long command.
    InSecondTurn,
}

/// What the window opened again begins with, in the prompt file of its launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Nothing: it opens on its conversation.
    Nothing,
    /// The note that its turn was cut: how the daemon gives a member the note when
    /// the human resumes the project.
    Note,
    /// The note behind the command that switches to full permission.
    BypassNote,
}

/// What is done to the window opened again, one step after the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// It is left alone for a while, to be seen doing what it does by itself.
    Watch,
    /// The keys that interrupt a turn are pressed, as the app presses them.
    Interrupt,
    /// The command that switches it to full permission is pasted.
    Bypass,
    /// The note that its turn was cut is pasted, as the daemon pastes a message it gives
    /// a window that is open.
    Note,
    /// The daemon's readiness is asked what it makes of the window (the mode its wire
    /// says), and what it would give the window before anything else (`/bypass`, where
    /// the wire says the normal mode) is given, as the daemon gives it: the wire is
    /// waited for to say the mode changed.
    Ready,
}

/// A case of the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    name: &'static str,
    pub cut: Cut,
    pub opening: Opening,
    pub then: &'static [Step],
    /// Whether the launches have none of the owner's own hooks, but ConsensFlow's.
    pub alone: bool,
}

impl Case {
    const fn new(name: &'static str, cut: Cut, opening: Opening, then: &'static [Step]) -> Self {
        Self {
            name,
            cut,
            opening,
            then,
            alone: false,
        }
    }

    const fn alone(mut self) -> Self {
        self.alone = true;
        self
    }

    pub const ALL: [Case; 13] = [
        Case::new("at-rest", Cut::AtRest, Opening::Nothing, &[Step::Watch]),
        Case::new(
            "mid-tool",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Ready],
        ),
        Case::new(
            "after-note",
            Cut::InFirstTurn,
            Opening::Note,
            &[Step::Ready],
        ),
        Case::new(
            "after-note-pasted",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Note],
        ),
        Case::new(
            "mid-tool-interrupted",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Interrupt],
        ),
        Case::new(
            "after-note-interrupted",
            Cut::InFirstTurn,
            Opening::Note,
            &[Step::Interrupt],
        ),
        Case::new(
            "interrupted-then-note",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Interrupt, Step::Note],
        ),
        Case::new(
            "second-turn",
            Cut::InSecondTurn,
            Opening::Nothing,
            &[Step::Watch],
        ),
        Case::new(
            "mid-tool-bypassed",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Bypass],
        ),
        Case::new(
            "bypassed-then-note",
            Cut::InFirstTurn,
            Opening::Nothing,
            &[Step::Watch, Step::Bypass, Step::Note],
        ),
        Case::new(
            "after-note-bypass",
            Cut::InFirstTurn,
            Opening::BypassNote,
            &[],
        ),
        Case::new("after-note-no-hooks", Cut::InFirstTurn, Opening::Note, &[]).alone(),
        Case::new(
            "second-turn-note-bypass",
            Cut::InSecondTurn,
            Opening::BypassNote,
            &[],
        ),
    ];

    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Whether the first window is cut inside a command.
    pub const fn in_command(self) -> bool {
        !matches!(self.cut, Cut::AtRest)
    }

    /// The first message of the window, as the daemon gives a member its task: the
    /// shell writes a file (which proves it may, before anything cuts it), and in a
    /// case cut in its first turn the window then runs the long command.
    pub fn first_message(self, tag: &str) -> String {
        let write = format!("echo first-{tag} > first-{tag}.txt");
        let body = if self.cut == Cut::InFirstTurn {
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

    /// The second task of a window that is cut in its second turn: the long command.
    pub fn second_message() -> String {
        delivery(
            SECOND,
            TASK,
            "note",
            "@chief",
            &format!(
                "Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: FINISHED\n{}",
                LONG.text()
            ),
        )
    }

    /// The member a window of the case is: as the daemon launches it, with the first
    /// message of the launch where there is one.
    pub fn member(self, first: Option<String>) -> Member {
        let member = Member::new("worker", ROLE);
        let member = match first {
            Some(message) => member.first_message(&message),
            None => member,
        };
        if self.alone {
            member.without_the_owners_hooks()
        } else {
            member
        }
    }

    /// The window opened again, `note` the note the engine gives it that its turn was cut
    /// (`live::ask::cut_note`): what its launch begins with, where the case has it begin
    /// with something. The adapter puts it behind `/bypass` as it prepares the launch.
    pub fn member_again(self, note: &str) -> Member {
        self.member(match self.opening {
            Opening::Nothing => None,
            Opening::Note => Some(note.to_owned()),
            Opening::BypassNote => Some(format!("{BYPASS} {note}")),
        })
    }

    /// The message and the task of the note the window opened again is given.
    pub const fn note_of() -> (i64, i64) {
        (NOTE as i64, TASK as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_are_named_as_they_are_asked_for_each_once() {
        let names: Vec<&str> = Case::ALL.iter().map(|case| case.name()).collect();
        assert_eq!(
            names,
            [
                "at-rest",
                "mid-tool",
                "after-note",
                "after-note-pasted",
                "mid-tool-interrupted",
                "after-note-interrupted",
                "interrupted-then-note",
                "second-turn",
                "mid-tool-bypassed",
                "bypassed-then-note",
                "after-note-bypass",
                "after-note-no-hooks",
                "second-turn-note-bypass",
            ]
        );
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn only_the_first_case_is_cut_at_rest_and_two_are_cut_in_the_second_turn() {
        let cuts: Vec<Cut> = Case::ALL.iter().map(|case| case.cut).collect();
        assert_eq!(cuts.iter().filter(|cut| **cut == Cut::AtRest).count(), 1);
        assert_eq!(cuts[0], Cut::AtRest);
        assert_eq!(
            cuts.iter().filter(|cut| **cut == Cut::InSecondTurn).count(),
            2
        );
        assert!(!Case::ALL[0].in_command());
        assert!(Case::ALL[1].in_command());
    }

    #[test]
    fn the_window_opened_again_begins_with_a_message_only_where_the_case_gives_it_one() {
        let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.";
        let message = |name: &str| {
            Case::ALL
                .iter()
                .find(|case| case.name() == name)
                .map(|case| case.member_again(note).message().map(str::to_owned))
                .unwrap()
        };
        assert_eq!(Case::note_of(), (2, 1));
        for name in ["at-rest", "mid-tool", "after-note-pasted", "second-turn"] {
            assert_eq!(message(name), None, "{name}");
        }
        assert_eq!(message("after-note").as_deref(), Some(note));
        assert_eq!(message("after-note-no-hooks").as_deref(), Some(note));
        for name in ["after-note-bypass", "second-turn-note-bypass"] {
            assert_eq!(message(name), Some(format!("/bypass {note}")), "{name}");
        }
    }

    #[test]
    fn the_daemons_readiness_is_asked_in_the_two_cases_that_have_the_products_way_whole() {
        let ready: Vec<&str> = Case::ALL
            .iter()
            .filter(|case| case.then.contains(&Step::Ready))
            .map(|case| case.name())
            .collect();
        assert_eq!(ready, ["mid-tool", "after-note"]);
        // Before the asks, after what else the case does.
        for case in Case::ALL
            .iter()
            .filter(|case| case.then.contains(&Step::Ready))
        {
            assert_eq!(case.then.last(), Some(&Step::Ready), "{}", case.name());
        }
    }

    #[test]
    fn only_the_case_that_asks_for_it_has_the_launches_without_the_owners_hooks() {
        let alone: Vec<&str> = Case::ALL
            .iter()
            .filter(|case| case.alone)
            .map(|case| case.name())
            .collect();
        assert_eq!(alone, ["after-note-no-hooks"]);
        assert!(Case::ALL[11].member_again("note").is_alone());
        assert!(!Case::ALL[2].member_again("note").is_alone());
    }

    #[test]
    fn the_task_a_window_starts_with_is_a_file_with_its_shell_and_where_it_is_cut_in_its_first_turn_the_long_command(
    ) {
        let rest = Case::ALL[0].first_message("ab12");
        assert_eq!(
            rest,
            "[ConsensFlow m-1 · T-1 · task from @chief]\nRun this exact command with your shell tool: echo first-ab12 > first-ab12.txt\nThen reply with exactly one line: FIRST_DONE"
        );
        // A window cut in its second turn has a first that ends, as one cut at rest has.
        let second_turn = Case::ALL[7];
        assert_eq!(second_turn.first_message("ab12"), rest);
        for case in Case::ALL.iter().filter(|case| case.cut == Cut::InFirstTurn) {
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

    #[test]
    fn the_second_task_is_the_long_command_under_a_number_of_its_own() {
        let second = Case::second_message();
        assert!(
            second
                .starts_with("[ConsensFlow m-5 · T-1 · note from @chief]\nRun this exact command"),
            "{second}"
        );
        assert!(second.ends_with("cf-devin-resume-long-command"), "{second}");
        assert_eq!((NOTE, SECOND), (2, 5));
    }
}
