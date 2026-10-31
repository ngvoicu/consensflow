use super::*;

/// The tests a run said failed, by their names.
fn caught(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
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
    assert_eq!(
        line(Verdict::NotRun, "a", 2, ""),
        "NOT RUN          a (2 s)"
    );
    assert_eq!(
        line(Verdict::Unstable, "a", 3, ""),
        "UNSTABLE         a (3 s)"
    );
    assert_eq!(
        line(Verdict::Broken, "a", 1, ""),
        "BROKEN           a (1 s)"
    );
    assert_eq!(
        line(Verdict::MissedByMeant, "a", 4, " by m::b, not by c"),
        "MISSED-BY-MEANT  a (4 s) by m::b, not by c"
    );
    assert_eq!(
        line(Verdict::NotBuilt, "a", 5, ""),
        "NOT BUILT        a (5 s)"
    );
}

#[test]
fn what_a_playwright_run_ran_and_its_first_failure_are_its_summary_and_its_first_error() {
    let output = "Running 13 tests using 1 worker\n\n  ✓  1 a.spec.mjs:1:1 › one (5ms)\n\n  1) a.spec.mjs:9:3 › g › two \n\n    Error: expect(received).toBe(expected) // Object.is equality\n\n    Expected: 2\n\n  1 failed\n    a.spec.mjs:9:3 › g › two \n  12 passed (3.1s)\n";
    assert_eq!(
        what_ran(output),
        [
            "Running 13 tests using 1 worker",
            "  1 failed",
            "  12 passed (3.1s)"
        ]
    );
    assert_eq!(
        first_failure(output).as_deref(),
        Some("Error: expect(received).toBe(expected) // Object.is equality")
    );
    assert_eq!(
        first_failure("  1) a.spec.mjs:9:3 › g › two \n\n  no error said\n"),
        None
    );
}

#[test]
fn a_plant_is_said_to_be_caught_by_the_first_test_that_failed_and_how_many_more_did() {
    let one = caught(&["a::b::first"]);
    assert_eq!(caught_by(&one, "first"), " by a::b::first");
    let three = caught(&["a::b::first", "a::b::second", "c::third"]);
    assert_eq!(caught_by(&three, "second"), " by a::b::first (+2)");
    assert_eq!(caught_by(&[], "second"), "");
}

#[test]
fn a_plant_caught_by_another_test_than_the_one_meant_says_so() {
    let found = caught(&["a::other"]);
    assert_eq!(
        caught_by(&found, "the_one_meant"),
        " by a::other, not by the_one_meant"
    );
    // A test that was meant is among those that failed, not only the first of them.
    let several = caught(&["a::other", "b::the_one_meant"]);
    assert_eq!(caught_by(&several, "the_one_meant"), " by a::other (+1)");
}

#[test]
fn the_test_meant_is_one_by_its_whole_name_or_the_end_of_its_path() {
    // A Rust test's name is a path that ends with the function; a Playwright test's, its
    // groups and its title; a Node test's, its own words.
    for (name, meant) in [
        ("x::tests::the_one_meant", "the_one_meant"),
        ("x::tests::the_one_meant", "tests::the_one_meant"),
        ("x::tests::the_one_meant", "x::tests::the_one_meant"),
        ("file drop on a Mac › types the paths", "types the paths"),
        (
            "it takes a word that is no option",
            "it takes a word that is no option",
        ),
    ] {
        assert!(is_meant(name, meant), "{name} is {meant}");
    }
    // Words that a name holds, at its start, its end or in it, are not enough.
    for (name, meant) in [
        ("x::not_the_one_meant", "the_one_meant"),
        ("x::the_one_meant_and_more", "the_one_meant"),
        ("x::the_one_meant::inner", "the_one_meant"),
        ("x::tests::the_one_meant", "s::the_one_meant"),
        ("file drop on a Mac › types the paths", "the paths"),
        ("it takes a word that is no option", "takes a word"),
        ("anything", ""),
    ] {
        assert!(!is_meant(name, meant), "{name} is not {meant}");
    }
    let found = caught(&["it takes a word that is no option"]);
    assert_eq!(
        caught_by(&found, "takes a word"),
        " by it takes a word that is no option, not by takes a word"
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
fn the_count_says_how_many_of_those_chosen_were_caught_and_how_many_were_unstable_or_skipped() {
    assert_eq!(totals(14, 15, 0, 0, 0), "14 of 15 plants caught");
    assert_eq!(
        totals(13, 15, 2, 0, 0),
        "13 of 15 plants caught, 2 unstable"
    );
    assert_eq!(
        totals(71, 71, 0, 2, 0),
        "71 of 71 plants caught, 2 skipped (platform)"
    );
    assert_eq!(
        totals(70, 70, 0, 1, 2),
        "70 of 70 plants caught, 1 skipped (platform), 2 not run (a harness this machine lacks)"
    );
    assert_eq!(
        absent("muse rig: a note", "Muse isn't installed"),
        "SKIPPED (absent)   muse rig: a note: not run: Muse isn't installed"
    );
    assert_eq!(
        skipped("a Mac says when", &["macos".into()], "windows"),
        "SKIPPED (platform) a Mac says when: for macos, and this is windows"
    );
}
