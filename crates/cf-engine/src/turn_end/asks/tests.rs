//! The rule on the words of the two field reports of T-1119 (a worker that ended its turn on a
//! question, the board showed the task done) and on the shapes it must leave alone. Real
//! results were counted against it on a copy of a ledger (T-1119): its shapes are those
//! results', with synthetic words. The review of T-1125 found the cases of the negative
//! tests below: each fired on the first version.

use super::asks_the_chief;

/// The first field report: a question about going on, the work half done.
const SHOULD_I_CONTINUE: &str = "Should I continue with the live A/B runs and the packet/socket capture, or stop here so you can review the findings first?";

/// The second field report: a question and a word that the worker waits.
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
fn a_worker_that_waits_for_the_chiefs_answer_asks() {
    for words in [
        "I'm waiting for your go on the full gate.",
        "I\u{2019}m waiting for your answer.",
        "I'm waiting\nfor your answer.",
        "**Now waiting for your go** for the full gate: `cargo xtask check` once load is under 25.",
        "Waiting for your gate go.",
        "The exclusive timing addendum still waits for your word on the accepted version.",
        "I'm still waiting on your answer to m-6196 before restarting the gate.",
        "I'm waiting for the chief's go. After it, the tree needs a fresh build.",
        "Nothing runs. I await your decision to go on.",
        "I'm ending my turn and will wait for your go.",
        "I'm waiting for your reopen with the sealed digest.",
        "The composition is unchanged and awaits your approval after the review.",
        "**Still waiting on you:** the fix for the two Windows reds is not pushed.",
        "I'm waiting for you to land the fix.",
        "I did not run the gate and am waiting for your go.",
        "Nothing blocks me, so I'm waiting for your go.",
    ] {
        asks(words);
    }
}

#[test]
fn a_worker_that_goes_on_when_the_chief_answers_asks() {
    asks("When you reply, I'll restart from step 1.");
    asks("I'll rebuild when you answer.");
    // This one says it will not run it: the negation is read as a denial.
    does_not_ask("I will not run it until you reply, and then I'll start.");
}

#[test]
fn a_worker_that_asks_whether_to_go_on_stop_or_wait_asks() {
    for words in [
        "May I rerun the gate now at 105 GiB, or should I wait for a landing to free more?",
        "How should I proceed: keep waiting for space, run the gate at the current free space, or something else?",
        "**Should I continue?**",
        "Should I continue?\"",
        "Should I stop here?",
        "Should we proceed with the merge?",
        "Shall we go on?",
        "Done with the parser. Should I continue with the printer?",
        "- Should I stop?\n- Should we proceed?",
        "Should I continue with the A/B runs... or stop here?",
    ] {
        asks(words);
    }
}

/// An offer is a result, and so is a question about the work: the worker is not stopped on
/// either, and the chief decides the work.
#[test]
fn an_offer_and_a_question_about_the_work_are_results() {
    for words in [
        "Want me to also add the Windows case?",
        "Do you want me to land it?",
        "Would you like me to split the commit?",
        "Shall I land it?",
        "Should I rebase onto main first?",
        "Can I delete the scratch folder?",
        "Should I start on the printer?",
    ] {
        does_not_ask(words);
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

/// The cue and the question mark are in one sentence: an earlier sentence that says "you want
/// me to" is none.
#[test]
fn the_cue_is_in_the_sentence_that_ends_with_the_question_mark() {
    does_not_ask(
        "The old flag stays, as you want me to keep it. Should #7 also cover absolute paths?",
    );
    does_not_ask("I can continue on the next task. Should #7 also cover absolute paths?");
    asks("The old flag stays. Should I continue with the next one?");
    // A bullet is a sentence of its own.
    does_not_ask("- Should I continue was asked of me earlier\n- Should #7 cover absolute paths?");
}

/// The cases the review of T-1125 found for the first-person half: a question of the work
/// that holds "should we", a quoted question, a question in a list of what was added, and a
/// prompt in a code block.
#[test]
fn a_question_that_is_not_the_workers_to_ask_is_none() {
    for words in [
        "Open question: should we make the guard opt-in per project?",
        "**Decision for you:** should we land T-1119 before I-2, or after?",
        "The customer's last line was: \"How should I configure the proxy?\"",
        "Added two FAQ entries: 'How do I reset my password?' and 'Can I change my plan?'",
        "The prompt reads `Should I continue?` and then waits.",
        "The script ends with:\n\n```python\nanswer = input('Should I continue? ')\n```",
        "The script ends with:\n```\nShould I continue?\n```",
        "> Should I continue?",
        "| Should I continue? | the prompt |",
        // The same with a go-on verb in the earlier words and the question about the work.
        "I stopped the server and went on. Should the lane start folded?",
    ] {
        does_not_ask(words);
    }
}

/// What a worker says that finished work awaits is a result: it awaits a review, not an
/// answer, and the chief reads it as it reads any result.
#[test]
fn finished_work_that_awaits_review_or_feedback_is_a_result() {
    for words in [
        "Done. Awaiting your review.",
        "Results are above; awaiting your decision.",
        "Waiting for your feedback!",
        "The branch awaits your review.",
        "Everything is committed and pushed, and awaits your inspection.",
        "Noted, no more changes. The branch awaits your review.",
    ] {
        does_not_ask(words);
    }
}

/// A report on this feature, and the text of an interface, say the words without waiting.
#[test]
fn a_description_of_the_feature_or_of_an_interface_is_a_result() {
    for words in [
        "A misfired task waits for the chief's `cf answer`.",
        "The answer reaches it when you answer with `cf answer`.",
        "Nothing is typed into the window until you answer.",
        "The result that says \"I'm waiting for your answer\" is now a question.",
        "The test hangs on 'Waiting for your device\u{2026}'.",
        "Hero copy: 'The future is waiting for you.'",
        "Hero copy: \"The future is waiting for you.\"",
        "Program output:\n\n```\nWaiting for your input...\n\nWaiting for your go\n```",
        "The phrases it takes:\n\n| phrase | fires |\n|---|---|\n| waiting for your go | yes |",
        "What the task prints:\n\n> I'm waiting for your go.",
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
fn a_phrase_inside_another_word_is_none() {
    // The end of a phrase is checked as its start is: "you" is not "youtube", nor "your" "yours".
    does_not_ask("The page is waiting for youtube's processing.");
    does_not_ask("I'm waiting for yours truly to wake up.");
    does_not_ask("As you said when you answered m-6196, the gate is red.");
    does_not_ask("I stopped when you replied.");
    does_not_ask("I'm waiting for your goal to load.");
    does_not_ask("Should it continue? Shall it stop?");
    does_not_ask("I stopped; should I have continued earlier?");
    does_not_ask("The runner stopped and continued, so should I?");
}

#[test]
fn a_negation_takes_back_a_wait() {
    for words in [
        "No question for you.",
        "Nothing is waiting for your answer.",
        "I am not waiting for your answer: the work is complete.",
        "There is nothing to wait for from you.",
        "Never mind: I no longer await your decision to go on.",
        "Nothing of mine is left waiting for your go.",
        "Nobody is waiting for your word.",
        "I'm not waiting for you.",
    ] {
        does_not_ask(words);
    }
    // But, and and so end a clause for it: the wait is a new one.
    asks("It is not a failure, but I'm waiting for your go.");
}

/// A negator inside the quoted words is not the worker's: the quote is out of the sentence.
#[test]
fn a_negator_in_quoted_words_does_not_count_and_a_wait_in_them_neither() {
    does_not_ask("The screen reads: 'nothing here yet \u{2014} waiting for your first upload.'");
    does_not_ask("The screen reads: \"nothing here yet \u{2014} waiting for your first upload.\"");
    asks("The log says 'ok'. I'm waiting for your go.");
    asks("I read \"nothing to do\" in the log, and I'm waiting for your go.");
}

/// "When you reply" and "until you answer" are a worker's only in its own sentence, and a
/// negation there takes them back as it does the other waits.
#[test]
fn going_on_when_the_chief_answers_is_the_workers_own_and_a_negation_takes_it_back() {
    does_not_ask("There is nothing to do when you reply.");
    does_not_ask("I do nothing when you reply.");
    does_not_ask("No step waits, and there is nothing to do when you reply.");
    does_not_ask("I am not going to continue until you answer.");
    does_not_ask("Nothing is typed into the window until you answer.");
    does_not_ask("The answer reaches the window when you answer with a label.");
    // Not the writer's own sentence, though it ends there.
    does_not_ask("The answer is typed into the window when you reply.");
    // The writer's own, but it does not end there: what follows is not a wait.
    does_not_ask("I send the label when you answer with one.");
    // "I won't touch it until you answer" is a wait that a negation reads as a denial: the rule
    // is on words, and a worker that says so asks with `cf ask`, which holds its task.
    does_not_ask("I won't touch the folder until you answer.");
    asks("When you reply, I'll restart from step 1.");
}

#[test]
fn a_question_about_going_on_that_is_no_first_person_ask_is_none() {
    // A go-on verb and a question mark, and nobody asks what to do.
    does_not_ask("Did the runner stop?");
    does_not_ask("Is it safe to continue?");
    does_not_ask("Will the build proceed without the lock?");
    does_not_ask("Why did it wait?");
}

#[test]
fn a_wait_phrase_that_stands_in_another_word_is_none() {
    // "wait for" stands inside "await for", which the rule does not read as a wait.
    does_not_ask("I await for your go.");
}

#[test]
fn a_question_mark_alone_is_not_a_question() {
    // A first-person cue without a question mark at the end of its sentence asks nothing.
    does_not_ask("I asked myself should I continue? Yes, and I did; it is all done.");
    does_not_ask("The brief says what I should do next: shall I stop? It says no.");
    does_not_ask("I will continue, and I will not stop.");
}

#[test]
fn no_words_and_the_words_of_no_answer_are_no_question() {
    does_not_ask("");
    does_not_ask("   \n\n  ");
    does_not_ask("(the agent ended its turn without a written answer)");
    does_not_ask("```\nonly code\n```");
}
