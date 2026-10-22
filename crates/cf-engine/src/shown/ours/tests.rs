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
