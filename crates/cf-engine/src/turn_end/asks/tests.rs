//! The rule on the words of the two field reports of T-1119 (a worker that ended its turn on a
//! question, the board showed the task done) and on the shapes it must leave alone. Real
//! results were counted against it on a copy of a ledger (T-1119): its shapes are those
//! results', with synthetic words.

use super::asks_the_chief;

/// The field report's m-383: a question about going on, the work half done.
const SHOULD_I_CONTINUE: &str = "Should I continue with the live A/B runs and the packet/socket capture, or stop here so you can review the findings first?";

/// The field report's m-387: a question and a word that the worker waits.
const WAITING_FOR_THE_INSTALL: &str =
    "Question for you: has the Build Tools 2022 install finished? I'm waiting for your answer.";

#[track_caller]
fn asks(words: &str) {
    assert!(asks_the_chief(words), "taken for a result: {words:?}");
}

#[track_caller]
fn does_not_ask(words: &str) {
    assert!(!asks_the_chief(words), "taken for a question: {words:?}");
}

#[test]
fn the_two_field_reports_are_questions() {
    asks(SHOULD_I_CONTINUE);
    asks(WAITING_FOR_THE_INSTALL);
}

#[test]
fn only_the_last_paragraph_decides() {
    let report = "Measured the capture on both hosts.\n\nThe A/B runs are half done.";
    asks(&format!("{report}\n\n{SHOULD_I_CONTINUE}"));
    asks(&format!("{report}\n\n\n  {WAITING_FOR_THE_INSTALL}\n\n"));
    // A question or a wait that a paragraph later takes back is none.
    does_not_ask(&format!(
        "{SHOULD_I_CONTINUE}\n\nNo: it is all done and committed."
    ));
    does_not_ask(&format!(
        "{WAITING_FOR_THE_INSTALL}\n\nThe install finished; the build passed."
    ));
}

#[test]
fn a_worker_that_waits_for_the_chief_asks() {
    for words in [
        "I'm waiting for your go on the full gate.",
        "I\u{2019}m waiting for your answer.",
        "I'm waiting\nfor your answer.",
        "**Now waiting for your go** for the full gate: `cargo xtask check` once load is under 25.",
        "Waiting for your gate go.",
        "The exclusive timing addendum still waits for your word on the accepted version.",
        "I'm still waiting on your answer to m-6196 before restarting the gate.",
        "I'm waiting for the chief's go. After it, the tree needs a fresh build.",
        "Nothing runs. I await your decision.",
        "When you reply, I'll restart from step 1.",
        "I won't touch the folder until you answer.",
        "The composition is unchanged and awaits your approval after the review.",
    ] {
        asks(words);
    }
}

#[test]
fn a_worker_that_asks_what_it_should_do_next_asks() {
    for words in [
        "Should I rebase onto main first?",
        "May I rerun the gate now at 105 GiB, or should I wait for a landing to free more?",
        "How should I proceed: keep waiting for space, run the gate at the current free space, or something else?",
        "Shall I land it?",
        "Do you want me to land it?",
        "Want me to also add the Windows case?",
        "Would you like me to split the commit?",
        "Should we take main's version of the file?",
        "Can I delete the scratch folder?",
        "**Should I continue?**",
        "Should I continue?\"",
        "Done with the parser. Should I start on the printer?",
        "- Should I rebase?\n- Should I land it?",
    ] {
        asks(words);
    }
}

#[test]
fn a_question_about_the_work_itself_leaves_a_finished_result_alone() {
    // The reports that end with an open question about their product are results: their
    // work is done, and the chief accepted every one in the ledger the rule was counted on.
    for words in [
        "## Open question\n\nShould the v3 default include antigravity?",
        "Should #7 also cover absolute paths like this one?",
        "Should it be run once as a gate?",
        "Should that test be superseded so a local timeout is refused too?",
        "A hook that is slow or noisy on stderr will show that in the service log. Fine as it is?",
        "## Open questions\n\n- Who fixes T-106's 5 Windows `skill_refresh` failures?",
        "Can you confirm, or point me to the right row?",
        "Do you want a further guard there, or is the limitation as documented acceptable?",
        "Which test should it run?",
        "Can it be run later?",
        "Is that wanted?",
    ] {
        does_not_ask(words);
    }
}

#[test]
fn a_wait_that_is_not_for_the_chief_is_none() {
    for words in [
        "Waiting for the lock; the monitor will report each stage.",
        "I'll wait for it to finish.",
        "Waiting on the landscape agent. Sections 1, 3 and 4 are drafted.",
        "The battery is next; waiting for the next event.",
        "The tests wait for a free slot, and nothing of mine holds a lock now.",
        "The scratch folders can be deleted by exact path on your word.",
        "`~/r35-lab` stays until you say otherwise; 377 GB is free.",
        "Free-tree-target.sh frees it when you say.",
        "I'll go on when the build ends.",
    ] {
        does_not_ask(words);
    }
}

#[test]
fn a_word_that_nothing_waits_is_none() {
    for words in [
        "No question for you.",
        "Nothing is waiting for your answer.",
        "I am not waiting for your answer: the work is complete.",
        "There is nothing to wait for. I won't wait for your reply.",
        "Never mind: I no longer await your decision.",
    ] {
        does_not_ask(words);
    }
}

#[test]
fn a_first_person_question_that_the_paragraph_goes_on_to_answer_is_none() {
    // The cue is in the paragraph, and the paragraph does not end on a question.
    does_not_ask("I asked myself should I continue? Yes, and I did; it is all done.");
    does_not_ask("The brief says what I should do next: shall I add a test? It says yes.");
}

#[test]
fn no_words_and_the_words_of_no_answer_are_no_question() {
    does_not_ask("");
    does_not_ask("   \n\n  ");
    does_not_ask("(the agent ended its turn without a written answer)");
}
