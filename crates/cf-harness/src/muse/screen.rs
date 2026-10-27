//! What a Muse window's screen shows (Muse 1.4.13, T-787's probes on `--provider
//! echo`): under the header `Muse Code <version>` and the conversation, the input box,
//! a rule that names the voice key (`── Voice input (⌥ + v to start) ───…`), the box's
//! lines, the first after its `❯`, a rule again, and the status line (the model, the
//! folder, `YOLO`). An empty box shows nothing after its `❯`. The box stays drawn while
//! a turn works, its status above it ("◈ Thinking (3s · esc to interrupt)"). Words the
//! conversation shows of the user's are drawn after a `❯` too, with no rule above.
//!
//! One screen holds a window for good: a resume of a session another Muse holds opens
//! Muse's session picker, which says so ("that session is already open in another
//! window — pick another session (held by pid …)") and resumes nothing.

use crate::shared::input_box::{Cursor, InputBox};

/// The mark the box's first line begins with.
const MARK: char = '❯';

/// What the picker says of a session another window holds.
const HELD: &str = "that session is already open in another window";

/// Whether `line` is one of Muse's rules: `─` from its start to its end, a label in it
/// at most (the voice key's, above the box).
fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("──") && line.ends_with('─')
}

/// The box `screen` (a window's last lines) shows: its first line's place and words
/// after the mark, and whether a line after it holds anything. None where there is
/// none, or it is drawn halfway (no rule below it yet).
fn boxed(screen: &[String]) -> Option<(usize, &str, bool)> {
    let first = screen
        .iter()
        .rposition(|line| line.trim_start().starts_with(MARK))?;
    if first == 0 || !is_rule(&screen[first - 1]) {
        return None;
    }
    let end = first + screen[first..].iter().position(|line| is_rule(line))?;
    let typed = screen[first].trim_start().trim_start_matches(MARK).trim();
    let more = screen[first + 1..end]
        .iter()
        .any(|line| !line.trim().is_empty());
    Some((first, typed, more))
}

/// What Muse's input box holds, where `screen` shows one. Muse draws no hint in an
/// empty box, so the cursor says nothing more than the words do.
pub(crate) fn read(screen: &[String], _cursor: Option<Cursor>) -> Option<InputBox> {
    let (_, typed, more) = boxed(screen)?;
    Some(if typed.is_empty() && !more {
        InputBox::Empty
    } else {
        InputBox::Holds
    })
}

/// Whether `screen` shows Muse's prompt: its input box, which it draws once it is up,
/// at work or at rest.
pub(crate) fn shows_prompt(screen: &[String]) -> bool {
    boxed(screen).is_some()
}

/// What the screen says where it shows the picker of a session another window holds:
/// Muse's own words, the lines that say it joined. None where it does not.
pub(crate) fn held_elsewhere(screen: &[String]) -> Option<String> {
    let at = screen.iter().position(|line| line.contains(HELD))?;
    let said: Vec<&str> = screen[at..]
        .iter()
        .map(|line| line.trim())
        .take_while(|line| !line.is_empty() && !line.contains(" · "))
        .collect();
    Some(said.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    const VOICE: &str = "── Voice input (⌥ + v to start) ──────────────────────────────────────";
    const RULE: &str = "──────────────────────────────────────────────────────────────────────";
    const STATUS: &str = "  echo · /U/g/P/n/work · YOLO";

    fn screen(conversation: &str, inside: &str) -> Vec<String> {
        lines(&format!(
            "  Muse Code 1.4.13\n\n{conversation}\n{VOICE}\n{inside}\n{RULE}\n{STATUS}"
        ))
    }

    #[test]
    fn an_empty_box_at_rest_or_at_work_is_empty_and_shows_the_prompt() {
        let rest = screen("❯ Say hello.\n◆ echo: Say hello.", "❯");
        assert_eq!(read(&rest, None), Some(InputBox::Empty));
        assert!(shows_prompt(&rest));
        let working = screen(
            "❯ Turn one.\n◈ Thinking (3s · esc to interrupt)─────────",
            "❯ ",
        );
        assert_eq!(read(&working, None), Some(InputBox::Empty));
        assert!(shows_prompt(&working));
    }

    #[test]
    fn words_in_the_box_on_one_line_or_more_are_held_and_the_conversation_s_are_not() {
        let typed = screen("❯ Say hello.", "❯ probe words");
        assert_eq!(read(&typed, None), Some(InputBox::Holds));
        let more = screen("", "❯ Line one of a draft\n  line two of it\n");
        assert_eq!(read(&more, None), Some(InputBox::Holds));
        let second_empty = screen("", "❯\n  line two");
        assert_eq!(read(&second_empty, None), Some(InputBox::Holds));
    }

    #[test]
    fn a_screen_still_starting_or_drawn_halfway_shows_no_prompt() {
        let starting = lines("  Muse Code 1.4.13\n\n");
        assert!(!shows_prompt(&starting));
        assert_eq!(read(&starting, None), None);
        let halfway = lines(&format!("  Muse Code 1.4.13\n{VOICE}\n❯"));
        assert!(!shows_prompt(&halfway), "no rule below it yet");
        // The user's words the conversation shows have no rule above them.
        let conversation = lines("  Muse Code 1.4.13\n\n❯ Say hello.\n◆ echo: Say hello.");
        assert!(!shows_prompt(&conversation));
    }

    #[test]
    fn the_picker_of_a_session_another_window_holds_says_so_in_muse_s_words() {
        let picker = lines(concat!(
            "\n\n  that session is already open in another window — pick another session (held by pid 40913 on\n",
            "  muse-host.local for 12s; if that window is gone, run `kill 40913` on muse-host.local)\n",
            "  0 / 0 · 0%  enter resume  esc exit  ctrl+c exit  tab focus sort/filter  ←/→ change option\n",
            "  ctrl+o comfortable view  ctrl+t transcript  ctrl+e expand  ↑/↓ browse  pgup/pgdn page",
        ));
        let said = held_elsewhere(&picker).unwrap();
        assert!(
            said.starts_with("that session is already open in another window"),
            "{said}"
        );
        assert!(
            said.ends_with("run `kill 40913` on muse-host.local)"),
            "{said}"
        );
        assert!(!shows_prompt(&picker));
        assert_eq!(held_elsewhere(&screen("", "❯")), None);
    }
}
