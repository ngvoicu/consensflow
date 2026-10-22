//! The run rule, line by line. The texts are made up for the tests.

use super::*;

fn lines(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

fn paste(id: i64, text: &str) -> Paste {
    Paste {
        id,
        text: text.to_owned(),
    }
}

#[test]
fn a_paste_is_its_run_and_the_line_after_it_is_the_windows_whatever_it_repeats() {
    let pastes = [paste(
        7,
        "[ConsensFlow m-7 · note from ConsensFlow]\nzeus says: Error: provider unreachable",
    )];
    let screen = lines(&[
        "> [ConsensFlow m-7 · note from ConsensFlow]",
        "  zeus says: Error: provider",
        "  unreachable",
        "",
        "Error: provider unreachable",
    ]);
    assert_eq!(ours(&screen, &pastes), [true, true, true, false, false]);
}

#[test]
fn a_line_that_only_holds_a_header_is_the_windows() {
    let pastes = [paste(7, "[ConsensFlow m-7 · note from ConsensFlow]\nLook")];
    let screen = lines(&["Error: bad prompt [ConsensFlow m-7 · note from ConsensFlow]"]);
    assert_eq!(ours(&screen, &pastes), [false]);
}

#[test]
fn the_labels_of_the_rows_a_paste_carried_and_short_pieces_are_the_pastes() {
    let pastes = [paste(
        9,
        "[ConsensFlow m-9 · T-1 · task from @chief]\nGo on\n\n(kept for you: note m-12 from @zeus)\nOK",
    )];
    let screen = lines(&[
        "│ [ConsensFlow m-9 · T-1 · task from @chief] │",
        "│ Go on                                      │",
        "│ (kept for you: note m-12 from @zeus)       │",
        "│ OK                                         │",
        "Error: model not found",
    ]);
    assert_eq!(ours(&screen, &pastes), [true, true, true, true, false]);
}

#[test]
fn a_header_as_devin_on_windows_shows_it_starts_the_run() {
    let pastes = [paste(
        3,
        "[ConsensFlow m-3 · T-1 · result from @worker] — done",
    )];
    let screen = lines(&[
        "[ConsensFlow m-3 | T-1 | result from @worker] -- done",
        "Error: x",
    ]);
    assert_eq!(ours(&screen, &pastes), [true, false]);
}

#[test]
fn a_placeholder_for_a_paste_is_no_reason_and_a_header_of_a_paste_not_known_ends_its_line() {
    let screen = lines(&[
        "> [Pasted text #1 +42 lines]",
        "[ConsensFlow m-99 · note from ConsensFlow]",
        "Look at the board",
    ]);
    assert_eq!(ours(&screen, &[]), [true, true, false]);
}

#[test]
fn a_line_of_the_run_that_looks_like_a_header_is_the_runs() {
    // A result that quotes another message's header as its screen showed it.
    let pastes = [paste(
        5,
        "[ConsensFlow m-5 · T-2 · result from @zeus]\nIts window said:\n[ConsensFlow m-12 · T-3 · note from @hera]\nand went on.",
    )];
    let screen = lines(&[
        "[ConsensFlow m-5 · T-2 · result from @zeus]",
        "Its window said:",
        "[ConsensFlow m-12 · T-3 · note from @hera]",
        "and went on.",
        "Error: provider unreachable",
    ]);
    assert_eq!(ours(&screen, &pastes), [true, true, true, true, false]);
}

#[test]
fn the_newest_paste_whose_header_scrolled_off_runs_from_the_top_line_only() {
    let pastes = [
        paste(
            9,
            "[ConsensFlow m-9 · note from ConsensFlow]\nline one\nline two\nline three",
        ),
        paste(8, "[ConsensFlow m-8 · note from ConsensFlow]\nolder words"),
    ];
    // Its last two lines at the top, then the window's error.
    let screen = lines(&["line two", "line three", "Error: x"]);
    assert_eq!(ours(&screen, &pastes), [true, true, false]);
    // An older paste's words at the top are not the newest's: the window's.
    assert_eq!(
        ours(&lines(&["older words", "Error: x"]), &pastes),
        [false, false]
    );
    // Lower down, the newest's words after the window's own line are the window's.
    assert_eq!(
        ours(&lines(&["Error: x", "line three"]), &pastes),
        [false, false]
    );
}

#[test]
fn a_box_drawn_in_ascii_is_its_frame() {
    let pastes = [paste(
        4,
        "[ConsensFlow m-4 · note from ConsensFlow]\nGo on with it",
    )];
    let screen = lines(&[
        "| [ConsensFlow m-4 · note from ConsensFlow] |",
        "| Go on with it                             |",
        "Error: x",
    ]);
    assert_eq!(ours(&screen, &pastes), [true, true, false]);
}

#[test]
fn a_placeholder_ends_the_run_it_stands_for() {
    // The paste's words are not on the screen: a line after its placeholder that
    // they hold is the window's (Calliope's re-check T-300, P3-R2).
    let pastes = [paste(
        6,
        "[ConsensFlow m-6 · note from ConsensFlow]\nSay OK when done.",
    )];
    let screen = lines(&["> [Pasted text #1 +1 lines]", "OK", "Error: x"]);
    assert_eq!(ours(&screen, &pastes), [true, false, false]);
}
