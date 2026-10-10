use super::*;
use crate::plants::runs::Ran;

/// What a run that wrote `output` caught.
fn caught(output: &str) -> Vec<String> {
    Ran {
        command: vec!["cargo".into(), "test".into()],
        code: Some(101),
        hung: false,
        output: output.to_owned(),
    }
    .caught()
}

#[test]
fn the_line_of_a_plant_pads_its_verdict_to_sixteen_and_says_the_seconds_and_what_caught_it() {
    assert_eq!(
        line(Verdict::Caught, "parser: a flag", 4, " by a_test"),
        "CAUGHT           parser: a flag (4 s) by a_test"
    );
    assert_eq!(
        line(Verdict::DoesNotCompile, "a", 0, ""),
        "DOES NOT COMPILE a (0 s)"
    );
    assert_eq!(
        line(Verdict::Missed, "a", 12, ""),
        "MISSED           a (12 s)"
    );
    assert_eq!(line(Verdict::Hung, "a", 1, ""), "HUNG             a (1 s)");
    assert_eq!(
        line(Verdict::DoesNotApply, "a", 0, ""),
        "DOES NOT APPLY   a (0 s)"
    );
}

#[test]
fn a_plant_is_said_to_be_caught_by_the_first_test_that_failed_and_how_many_more_did() {
    let one = caught("test a::b::first ... FAILED\n");
    assert_eq!(caught_by(&one, "first"), " by a::b::first");
    let three = caught(
        "test a::b::first ... FAILED\ntest a::b::second ... FAILED\ntest c::third ... FAILED\n",
    );
    assert_eq!(caught_by(&three, "second"), " by a::b::first (+2)");
    assert_eq!(caught_by(&[], "second"), "");
}

#[test]
fn a_plant_caught_by_another_test_than_the_one_meant_says_so() {
    let found = caught("test a::other ... FAILED\n");
    assert_eq!(
        caught_by(&found, "the_one_meant"),
        " by a::other, not by the_one_meant"
    );
    // A Rust test's name is a path that ends with the function, and what a plant says it
    // is meant for may be the whole function or only its start.
    for test in ["x::the_one_meant", "x::the_one_meant_and_more"] {
        let found = caught(&format!("test {test} ... FAILED\n"));
        assert_eq!(caught_by(&found, "the_one_meant"), format!(" by {test}"));
    }
    // A test that was meant is among those that failed, not only the first of them.
    let several = caught("test a::other ... FAILED\ntest b::the_one_meant ... FAILED\n");
    assert_eq!(caught_by(&several, "the_one_meant"), " by a::other (+1)");
}

#[test]
fn a_node_test_is_the_one_meant_where_its_name_holds_the_words_it_was_meant_by() {
    let found = caught("  ✖ it takes a word that is no option (1.2ms)\n");
    assert_eq!(
        caught_by(&found, "takes a word"),
        " by it takes a word that is no option"
    );
    assert_eq!(
        caught_by(&found, "takes two"),
        " by it takes a word that is no option, not by takes two"
    );
}

#[test]
fn what_a_run_that_passed_ran_is_the_lines_that_count_tests() {
    let output = "   Compiling x v1\nrunning 3 tests\ntest a ... ok\n\ntest result: ok. 3 passed; 0 failed\nℹ tests 5\nℹ pass 5\nℹ fail 0\nℹ cancelled 0\nrunning tests\n running 2 tests\n";
    assert_eq!(
        what_ran(output),
        [
            "running 3 tests",
            "test result: ok. 3 passed; 0 failed",
            "ℹ tests 5",
            "ℹ pass 5",
            "ℹ fail 0"
        ]
    );
    assert!(what_ran("nothing counted here").is_empty());
}

#[test]
fn the_first_error_the_report_of_node_ends_with_is_what_the_first_test_to_fail_said() {
    let output = "ℹ tests 2\n✖ failing tests:\n\n✖ it holds (3.1ms)\n  AssertionError [ERR_ASSERTION]: the version is its own\n    at foo\n✖ another (1ms)\n  TypeError: x is not a function\n";
    assert_eq!(
        first_failure(output).as_deref(),
        Some("AssertionError [ERR_ASSERTION]: the version is its own")
    );
}

#[test]
fn only_an_error_two_spaces_under_a_test_of_the_report_is_said() {
    // Before the report, or three spaces in, or no `Error` in the first word: nothing.
    assert_eq!(first_failure("✖ a (1ms)\n  Error: before\n"), None);
    assert_eq!(
        first_failure("failing tests:\n✖ a (1ms)\n   Error: too deep\n"),
        None
    );
    assert_eq!(
        first_failure("failing tests:\n✖ a (1ms)\n  Not an error\n"),
        None
    );
    // `Error` anywhere in the first word of the line counts, as `\w*Error` does.
    assert_eq!(
        first_failure("failing tests:\n✖ a (1ms)\n  RangeError: out\n").as_deref(),
        Some("RangeError: out")
    );
    assert_eq!(first_failure("no report"), None);
}

#[test]
fn what_the_error_said_is_cut_at_two_hundred_and_twenty_characters() {
    let output = format!("failing tests:\n✖ a (1ms)\n  Error: {}\n", "x".repeat(400));
    assert_eq!(
        first_failure(&output).map(|said| said.chars().count()),
        Some(220)
    );
}

#[test]
fn the_tail_of_a_text_is_its_last_characters_whole() {
    assert_eq!(tail("abcdef", 3), "def");
    assert_eq!(tail("abc", 10), "abc");
    assert_eq!(tail("", 3), "");
    // A character that is more than one byte is not cut.
    assert_eq!(tail("ab✖cd", 3), "✖cd");
}

#[test]
fn the_count_says_how_many_of_those_chosen_were_caught() {
    assert_eq!(totals(14, 15), "14 of 15 plants caught");
}
