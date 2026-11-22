//! The reading on the five field sentences of the advice T-1919 (synthetic where a result's
//! own words are not kept) and on the shapes it must leave alone.

use super::ends_to_wait;

#[track_caller]
fn waits(words: &str) {
    assert!(ends_to_wait(words), "taken for a result: {words:?}");
}

#[track_caller]
fn is_a_result(words: &str) {
    assert!(!ends_to_wait(words), "taken for a wait: {words:?}");
}

#[test]
fn the_five_field_sentences_end_to_wait() {
    // T-1887 (m-12797) and T-1910 (m-12911), and the three before the hold rule.
    waits("Final-head workspace, whole rig and Windows harness runs are going (head `c61c13a60`); I'll check them at the next wakeup.");
    waits("Windows dispatcher passes: 1791 tests. The rig is still waiting for its build turn, so I'm ending this turn until it runs.");
    waits("Three locked steps are queued behind T-87: (A)'s parity, (A)'s suite, and (B)'s first compile. I'll continue when they report.");
    waits("The plant runner is still working through the nine plants in my worktree. I'll resume when it reports, then run the two crates' full suites once.");
    waits("The folder-trust table is still running. I'll pick up again when it moves to the other tables or a plant misses.");
}

#[test]
fn other_first_person_ways_to_say_it() {
    waits("I am ending my turn here until the build finishes.");
    waits("I will end my turn to wait for the run.");
    waits("Work is running.\n\nOnce the gate ends, I will continue with the review.");
    waits("After the suite finishes, I'll report the counts.");
    waits("I’m ending this turn until it runs.");
    waits("- the rig runs\n- I'll check it as soon as it lands");
}

#[test]
fn the_wordings_a_review_found_missing_end_to_wait() {
    // T-1974's finding 4, and T-50's m-2283.
    waits("The landing runs in the background with a 2-hour timeout. I'll wait for it to finish.");
    waits("I'm going to check them at the next wakeup.");
    waits("I'll stop here and continue when the run ends.");
    waits("Waiting for the build turn; will report when it finishes.");
    waits("I will wait until the gate ends.");
    waits("I am going to resume when it reports.");
}

#[test]
fn a_wait_for_something_that_is_no_work_of_the_members_is_a_result() {
    is_a_result("I'll wait for the review.");
    is_a_result("I'll wait for your go.");
    is_a_result("The runner will report when it finishes.");
    is_a_result("It is going to resume when the rig ends.");
    is_a_result("I'm not editing anything in the tree while it runs.");
    is_a_result("I won't wait for it to finish.");
}

#[test]
fn waits_only_the_last_paragraph_decides() {
    let wait = "I'll check them at the next wakeup.";
    waits(&format!("All of it is written.\n\n{wait}"));
    is_a_result(&format!("{wait}\n\nNo: it all ran and the head is pushed."));
}

#[test]
fn a_result_is_left_alone() {
    is_a_result("Nothing of mine is still running.");
    is_a_result("Done: the head is abc123, pushed. The suites passed.");
    is_a_result("The reader checks them at the next wakeup.");
    is_a_result("The runner will resume when it reports.");
    is_a_result("It continues when the rig ends, in the third person.");
    is_a_result("I did not end my turn until it ran; it ran inside the turn.");
    is_a_result("I won't resume when it reports; the work is done.");
    is_a_result("I never check them at the next wakeup.");
    // A negation before the phrase, in its clause, takes it back.
    is_a_result("I don't think I'll resume when it reports; it is done.");
    is_a_result("There is no reason I will check them at the next wakeup.");
    waits("It did not fail, so I'll resume when it reports.");
    // No first person in the clause: a limit of the reading, as for any other words.
    is_a_result("Ending my turn to wait for the run.");
    is_a_result("The test asserts `I'm ending this turn until it runs.` is read.");
    is_a_result("The note says \"I'll resume when it reports.\"");
    is_a_result("> I'll resume when it reports.");
    is_a_result("```\nI'm ending this turn until it runs.\n```");
    is_a_result("");
}
