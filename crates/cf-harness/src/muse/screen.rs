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

/// Whether `line` is the rule Muse draws above its box: one with a label in it (the
/// voice key's), where the rule under the box has none.
fn is_labelled(line: &str) -> bool {
    is_rule(line) && line.trim().chars().any(|c| c != '─')
}

/// The place of the rule above the box: the last labelled rule in view, for the box is
/// the screen's last thing but its status line.
fn voice_rule(screen: &[String]) -> Option<usize> {
    screen.iter().rposition(|line| is_labelled(line))
}

/// The box `screen` (a window's last lines) shows: its first line's place and words
/// after the mark, and whether a line after it holds anything. The box is found by the
/// rule above it: its first line is the line after that rule, where it begins with the
/// mark, and a mark anywhere else is the box's words or the conversation's (a brief
/// that quotes a Muse screen holds such lines: Calliope's P3-2 of T-945). None where
/// there is none, or it is drawn halfway (no rule below it yet).
fn boxed(screen: &[String]) -> Option<(usize, &str, bool)> {
    let first = voice_rule(screen)? + 1;
    if !screen.get(first)?.trim_start().starts_with(MARK) {
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

/// The words Muse's input box shows on `screen`, its lines after the mark with a line
/// break between two: empty for an empty box, none where it shows no box.
pub(crate) fn words(screen: &[String]) -> Option<String> {
    let (first, typed, _) = boxed(screen)?;
    let end = first + screen[first..].iter().position(|line| is_rule(line))?;
    let words = std::iter::once(typed)
        .chain(screen[first + 1..end].iter().map(|line| line.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    Some(words.trim_end().to_owned())
}

/// Whether `screen` shows the tail of a box whose first line, with the mark, is out of
/// view: the rule above the box is in view, with words under it up to the rule below,
/// and no mark on the first line under it. A box holding more lines than the window keeps
/// them under its rule and scrolls its first line out (T-829's probe, Muse 1.4.14). A
/// screen drawn halfway, the rule above the box and nothing under it yet, is no cut box.
pub(crate) fn box_cut(screen: &[String]) -> bool {
    let Some(top) = voice_rule(screen) else {
        return false;
    };
    let under = &screen[top + 1..];
    let Some(end) = under.iter().position(|line| is_rule(line)) else {
        return false;
    };
    let inside = &under[..end];
    inside.iter().any(|line| !line.trim().is_empty()) && !inside[0].trim_start().starts_with(MARK)
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
    fn the_human_is_told_the_box_s_words_as_muse_draws_them_and_empty_for_an_empty_box() {
        use crate::shared::input_box::shown;
        use cf_proto::agents::Harness;
        let more = screen("❯ Say hello.", "❯ Line one of a draft\n  line two of it\n");
        assert_eq!(
            shown(Harness::Muse, &more, None).as_deref(),
            Some("Line one of a draft\nline two of it")
        );
        let empty = screen("❯ Say hello.\n◆ echo: Say hello.", "❯");
        assert_eq!(shown(Harness::Muse, &empty, None).as_deref(), Some(""));
        assert_eq!(
            shown(Harness::Muse, &lines("  Muse Code 1.4.13"), None),
            None
        );
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

    /// Calliope's P3-2 of T-945: a brief that quotes a Muse screen has lines that begin
    /// with the mark, which the last mark in view took for the box's first line.
    #[test]
    fn a_box_whose_words_begin_a_line_with_the_mark_is_read_whole_by_the_rule_above_it() {
        let quoting = screen(
            "❯ Say hello.",
            "❯ Read this screen:\n  ❯ Say hello.\n  ◆ echo: Say hello.",
        );
        assert_eq!(read(&quoting, None), Some(InputBox::Holds));
        assert!(shows_prompt(&quoting));
        assert!(!box_cut(&quoting));
        assert_eq!(
            words(&quoting).as_deref(),
            Some("Read this screen:\n❯ Say hello.\n◆ echo: Say hello.")
        );
    }

    #[test]
    fn a_cut_box_is_read_cut_with_a_mark_in_its_words_and_a_screen_drawn_halfway_is_not() {
        // The tail of a taller box: its first line out of view, a quoted mark in it.
        let tail = lines(&format!(
            "{VOICE}\n  l30 of the brief\n  ❯ Say hello.\n  l32 of the brief\n{RULE}\n{STATUS}"
        ));
        assert_eq!(read(&tail, None), None);
        assert!(box_cut(&tail));
        // The rule above the box drawn, and nothing under it yet: no box, cut or whole.
        let halfway = lines(&format!(
            "  Muse Code 1.4.14\n\n◆ echo: a turn taken back\n{VOICE}"
        ));
        assert!(!box_cut(&halfway));
        assert!(!shows_prompt(&halfway));
        let halfway_blank = lines(&format!("  Muse Code 1.4.14\n◆ echo: one\n{VOICE}\n\n"));
        assert!(!box_cut(&halfway_blank));
        // A whole box is never cut, empty or holding words.
        assert!(!box_cut(&screen("❯ Say hello.", "❯")));
        assert!(!box_cut(&screen("", "❯ Line one\n  line two")));
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
