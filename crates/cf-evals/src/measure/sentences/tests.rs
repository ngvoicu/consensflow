//! What counts as a question put to the owner, and what does not.

use super::*;

#[test]
fn a_question_mark_in_code_or_in_a_link_is_no_question() {
    let terminal = [
        "Am citit fișierele. Păstrăm vechiul document din docs/? Sau îl ștergem?",
        "",
        "```sh",
        "grep -n \"ce?\" site/*.html",
        "```",
        "Vezi `a ? b : c` și https://example.com/page?lang=en pentru detalii.",
        "- Publicăm acum sau după ce vezi pagina?",
    ]
    .join("\n");
    assert_eq!(
        question_sentences(&terminal),
        [
            "Păstrăm vechiul document din docs/?",
            "Sau îl ștergem?",
            "- Publicăm acum sau după ce vezi pagina?",
        ]
    );
    assert_eq!(count_questions(&terminal), 3);
}

#[test]
fn the_marks_around_a_question_do_not_end_it_an_opus_chief_asked_in_bold() {
    let bold = [
        "Nu am pornit nimic pe board încă — două întrebări:",
        "",
        "1. **Cum se numește fișierul paginii?** (spune-mi exact numele, inclusiv extensia)",
        "2. **În ce limbă o facem întâi — română sau engleză?**",
        "",
        "_Pornesc imediat?_",
    ]
    .join("\n");
    // The numbers of the list end their own sentences, and the words in brackets after the
    // first question end in no question mark.
    assert_eq!(
        question_sentences(&bold),
        [
            "Cum se numește fișierul paginii?",
            "În ce limbă o facem întâi — română sau engleză?",
            "Pornesc imediat?",
        ]
    );
    assert_eq!(count_questions(&bold), 3);
    assert_eq!(count_questions("Gata. Pagina e făcută."), 0);
    assert_eq!(count_questions(""), 0);
}

#[test]
fn a_question_is_cut_where_a_mark_that_ends_a_sentence_is_followed_by_white_space() {
    assert_eq!(
        sentences("Done.  Next?  Yes!"),
        ["Done.", "Next?", "Yes!"],
        "a run of white space is one separator"
    );
    // No mark before the white space: no split.
    assert_eq!(sentences("a b  c"), ["a b  c"]);
    assert_eq!(sentences("what? no"), ["what?", "no"]);
    // A mark with nothing after it is not split from the line's end by a space that is not there.
    assert_eq!(sentences("end?"), ["end?"]);
    assert_eq!(sentences(""), [""]);
    // White space at the end of the line after a mark is a separator, with an empty piece after it, as split does.
    assert_eq!(sentences("end? "), ["end?", ""]);
    // A mark that is not at the end of a word does not split.
    assert_eq!(sentences("a.b c"), ["a.b c"]);
    assert_eq!(sentences("?  a"), ["?", "a"]);
}

#[test]
fn a_closing_quote_or_bracket_after_the_question_mark_is_not_part_of_the_sentence() {
    assert_eq!(count_questions("Did he say \"stop?\""), 1);
    assert_eq!(count_questions("(Is it done?)"), 1);
    assert_eq!(count_questions("Said «ce faci?»"), 1);
    // With a space after the mark the line is cut there, and the quote is a piece of its own.
    assert_eq!(question_sentences("Said « ce faci? »"), ["Said « ce faci?"]);
    // Text after the closing mark that is not one of them: no question.
    assert_eq!(count_questions("Is it done? Yes."), 1);
}

#[test]
fn a_fence_that_is_never_closed_is_text_and_a_pair_of_fences_is_gone_with_what_they_hold() {
    assert_eq!(count_questions("```\nwhat?\n```\nand this?"), 1);
    // Never closed: nothing is code, and the line is a question.
    assert_eq!(count_questions("```\nwhat?"), 1);
}

#[test]
fn a_link_is_cut_out_where_javascripts_word_boundary_finds_one() {
    assert_eq!(count_questions("see (http://a.b/c?d=1) now?"), 1);
    assert_eq!(
        count_questions("http://a.b/c?"),
        0,
        "a link, and no question"
    );
    // A scheme that begins inside a word has no boundary before it, and is no link.
    assert_eq!(count_questions("1http://a.b/c?"), 1);
    // After a letter that is not ASCII there is a boundary, as JavaScript has it.
    assert_eq!(count_questions("éhttp://a.b/c? ok?"), 1);
    assert_eq!(count_questions("éhttp://a.b/c?"), 0);
}

#[test]
fn the_questions_of_each_turn_end_are_counted_with_the_pickers_the_owner_answered() {
    let counted = owner_questions(
        &[
            "Two pages are done.".to_owned(),
            "Should I also translate the FAQ? And the footer?".to_owned(),
        ],
        2,
    );
    assert_eq!(
        (counted.questions, counted.turns_asking, counted.pickers),
        (2, 1, 2)
    );
    // What was counted, for a human to check.
    assert_eq!(
        counted.texts,
        ["Should I also translate the FAQ? And the footer?"]
    );
    let none = owner_questions(&[], 0);
    assert_eq!((none.questions, none.turns_asking), (0, 0));
    assert!(none.texts.is_empty());
}
