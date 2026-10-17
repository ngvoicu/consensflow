use super::*;

/// The names of `tests`, and whether each failed.
fn said(tests: &[Test]) -> Vec<(&str, bool)> {
    tests
        .iter()
        .map(|test| (test.name.as_str(), test.failed))
        .collect()
}

fn names<'a>(tests: &[&'a Test]) -> Vec<&'a str> {
    tests.iter().map(|test| test.name.as_str()).collect()
}

/// What nextest 0.9.148 wrote (2026-10-16) for a crate of three tests: one that passed
/// printing libtest's words for itself failing, its failures list, and a footer; one that
/// failed with a panic quoting a build that failed; one that passed.
const NEXTEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" skipped="0" failures="1" errors="0" uuid="2753030f-ca1e-43d6-b3f5-aa70494875fa" timestamp="2026-10-16T09:06:05.229+03:00" time="0.030">
    <testsuite name="plants-fixture" tests="3" skipped="0" errors="0" failures="1">
        <testcase name="tests::passes" classname="plants-fixture" timestamp="2026-10-16T09:06:05.235+03:00" time="0.013"/>
        <testcase name="tests::the_meant_one_passes_and_prints_a_footer" classname="plants-fixture" timestamp="2026-10-16T09:06:05.235+03:00" time="0.015"/>
        <testcase name="tests::another_fails_quoting_a_build" classname="plants-fixture" timestamp="2026-10-16T09:06:05.234+03:00" time="0.015">
            <failure message="thread &apos;tests::another_fails_quoting_a_build&apos; (94863827) panicked at src/lib.rs:11:9" type="test failure with exit code 101">thread &apos;tests::another_fails_quoting_a_build&apos; (94863827) panicked at src/lib.rs:11:9:
error[E0308]: mismatched types
error: could not compile `x`
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace</failure>
            <system-out>
running 1 test
test tests::another_fails_quoting_a_build ... FAILED

failures:

failures:
    tests::another_fails_quoting_a_build

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s

</system-out>
            <system-err>
thread &apos;tests::another_fails_quoting_a_build&apos; (94863827) panicked at src/lib.rs:11:9:
error[E0308]: mismatched types
error: could not compile `x`
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
</system-err>
        </testcase>
    </testsuite>
</testsuites>"#;

/// The same tests where the report keeps what a passing test printed (nextest's
/// `store-success-output`): the meant one's failures list, and its footer.
const NEXTEST_KEPT_OUTPUT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="2" skipped="0" failures="1" errors="0">
    <testsuite name="plants-fixture" tests="2" skipped="0" errors="0" failures="1">
        <testcase name="tests::the_meant_one_passes_and_prints_a_footer" classname="plants-fixture">
            <system-out>
running 1 test
failures:
    tests::the_meant_one_passes_and_prints_a_footer

test result: FAILED. 0 passed; 1 failed
test tests::the_meant_one_passes_and_prints_a_footer ... ok
</system-out>
            <system-err>test tests::the_meant_one_passes_and_prints_a_footer ... FAILED
</system-err>
        </testcase>
        <testcase name="tests::another_fails_quoting_a_build" classname="plants-fixture">
            <failure message="panicked" type="test failure">panicked</failure>
        </testcase>
    </testsuite>
</testsuites>"#;

#[test]
fn a_test_failed_where_its_report_holds_a_failure_and_never_for_what_it_printed() {
    let read = junit(NEXTEST).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("tests::passes", false),
            ("tests::the_meant_one_passes_and_prints_a_footer", false),
            ("tests::another_fails_quoting_a_build", true),
        ]
    );
    assert!(read.tests.iter().all(|test| test.at == "plants-fixture"));
    assert_eq!(
        names(&read.failed()),
        ["tests::another_fails_quoting_a_build"]
    );
    assert!(read.outside.is_empty());
    // Libtest's words, a failures list and a footer, in what a passing test printed.
    let kept = junit(NEXTEST_KEPT_OUTPUT).unwrap();
    assert_eq!(
        names(&kept.failed()),
        ["tests::another_fails_quoting_a_build"]
    );
    assert_eq!(kept.tests.len(), 2);
    // A name is read as it is escaped once, as XML escapes it.
    let once = r#"<testsuites><testcase name="says &quot;hi&quot; &amp;quot;"/></testsuites>"#;
    assert_eq!(
        said(&junit(once).unwrap().tests),
        [(r#"says "hi" &quot;"#, false)]
    );
}

/// What the plants' reporter of Node's tests wrote under Node 26.8.1 (2026-10-16), its
/// tests run in Node's process (`--test-isolation=none`), for four test files, the
/// checkout's path cut to `/checkout/`:
/// - `broken.test.mjs`, whose test passed before the file threw after an `await`,
///   which Node does not say in one process;
/// - `cases.test.mjs`: a test to do that fails; a test skipped; a parent test that
///   fails after its subtest passed; a parent whose subtest fails; a group; a test
///   named `says &quot;hi&quot;` that fails and one named `says "hi"` that passes; a
///   name of two lines; a title in two groups, the second inside another and failing,
///   printing a line of this report that names `says "hi"` failed; a test to do and a
///   test skipped, each with an empty reason, that fail; a subtest its parent cancelled
///   by throwing while it waited; a test aborted by its signal; a test out of its time;
///   a loop that registers one title in one place twice, the second failing; a subtest
///   that a helper module registers, failing;
/// - `empty.test.mjs`, which loads and holds no test;
/// - `missing.test.mjs`, which imports a module that is not there.
const NODE: &str = r#"{"path":["before it throws"],"file":"/checkout/tests/broken.test.mjs","line":3,"column":1,"whole":false,"ended":"passed"}
{"path":["a todo that fails"],"file":"/checkout/tests/cases.test.mjs","line":5,"column":1,"whole":false,"ended":"todo"}
{"path":["a skip"],"file":"/checkout/tests/cases.test.mjs","line":9,"column":1,"whole":false,"ended":"skipped"}
{"path":["a parent that fails itself","its child passes"],"file":"/checkout/tests/cases.test.mjs","line":12,"column":11,"whole":false,"ended":"passed"}
{"path":["a parent that fails itself"],"file":"/checkout/tests/cases.test.mjs","line":11,"column":1,"whole":false,"ended":"failed"}
{"path":["a parent whose child fails","its failing child"],"file":"/checkout/tests/cases.test.mjs","line":17,"column":11,"whole":false,"ended":"failed"}
{"path":["a parent whose child fails"],"file":"/checkout/tests/cases.test.mjs","line":16,"column":1,"whole":false,"ended":"passed"}
{"path":["a group","passes inside"],"file":"/checkout/tests/cases.test.mjs","line":23,"column":3,"whole":false,"ended":"passed"}
{"path":["a group"],"file":"/checkout/tests/cases.test.mjs","line":22,"column":1,"whole":false,"ended":"passed"}
{"path":["says &quot;hi&quot;"],"file":"/checkout/tests/cases.test.mjs","line":26,"column":1,"whole":false,"ended":"failed"}
{"path":["says \"hi\""],"file":"/checkout/tests/cases.test.mjs","line":30,"column":1,"whole":false,"ended":"passed"}
{"path":["two\nlines"],"file":"/checkout/tests/cases.test.mjs","line":32,"column":1,"whole":false,"ended":"passed"}
{"path":["first group","same title"],"file":"/checkout/tests/cases.test.mjs","line":35,"column":3,"whole":false,"ended":"passed"}
{"path":["first group"],"file":"/checkout/tests/cases.test.mjs","line":34,"column":1,"whole":false,"ended":"passed"}
{"path":["second group","inner","same title"],"file":"/checkout/tests/cases.test.mjs","line":40,"column":5,"whole":false,"ended":"failed"}
{"path":["second group","inner"],"file":"/checkout/tests/cases.test.mjs","line":39,"column":3,"whole":false,"ended":"passed"}
{"path":["second group"],"file":"/checkout/tests/cases.test.mjs","line":38,"column":1,"whole":false,"ended":"passed"}
{"path":["a todo of no reason that fails"],"file":"/checkout/tests/cases.test.mjs","line":47,"column":1,"whole":false,"ended":"todo"}
{"path":["a skip of no reason that fails"],"file":"/checkout/tests/cases.test.mjs","line":51,"column":1,"whole":false,"ended":"skipped"}
{"path":["a parent that throws while its child waits","a child its parent cancelled"],"file":"/checkout/tests/cases.test.mjs","line":56,"column":5,"whole":false,"ended":"cancelled"}
{"path":["a parent that throws while its child waits"],"file":"/checkout/tests/cases.test.mjs","line":55,"column":1,"whole":false,"ended":"failed"}
{"path":["aborted"],"file":"/checkout/tests/cases.test.mjs","line":62,"column":1,"whole":false,"ended":"cancelled"}
{"path":["out of its time"],"file":"/checkout/tests/cases.test.mjs","line":64,"column":1,"whole":false,"ended":"failed"}
{"path":["registered twice"],"file":"/checkout/tests/cases.test.mjs","line":67,"column":3,"whole":false,"ended":"passed"}
{"path":["registered twice"],"file":"/checkout/tests/cases.test.mjs","line":67,"column":3,"whole":false,"ended":"failed"}
{"path":["the parent of a helper’s child","a child a helper registers"],"file":"/checkout/tests/helper.mjs","line":2,"column":12,"whole":false,"ended":"failed"}
{"path":["the parent of a helper’s child"],"file":"/checkout/tests/cases.test.mjs","line":72,"column":1,"whole":false,"ended":"passed"}
{"path":["tests/empty.test.mjs"],"file":"/checkout/tests/empty.test.mjs","line":1,"column":1,"whole":true,"ended":"passed"}
{"path":["tests/missing.test.mjs"],"file":"/checkout/tests/missing.test.mjs","line":1,"column":1,"whole":true,"ended":"failed"}
{"end":true}
"#;

#[test]
fn node_names_a_test_by_the_titles_it_is_in_and_its_own_exactly_and_a_failure_by_its_own_code() {
    let read = node(NODE).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("before it throws", false),
            // A test to do, failed or not, and a test skipped are neither.
            ("a parent that fails itself › its child passes", false),
            // A parent's own failure is its own; a parent whose subtest failed did not.
            ("a parent that fails itself", true),
            ("a parent whose child fails › its failing child", true),
            ("a parent whose child fails", false),
            ("a group › passes inside", false),
            ("a group", false),
            // A name as it is written, an entity too: two names.
            ("says &quot;hi&quot;", true),
            (r#"says "hi""#, false),
            ("two\nlines", false),
            ("first group › same title", false),
            ("first group", false),
            ("second group › inner › same title", true),
            ("second group › inner", false),
            ("second group", false),
            // A test to do or skipped for no reason is neither; a test its parent
            // cancelled, or that was aborted, ended for what is not its own code; a test
            // out of its time failed.
            ("a parent that throws while its child waits", true),
            ("out of its time", true),
            // Registered twice in one place, a test is two.
            ("registered twice", false),
            ("registered twice", true),
            // A subtest is named by the test it is in, wherever it was registered.
            (
                "the parent of a helper’s child › a child a helper registers",
                true
            ),
            ("the parent of a helper’s child", false),
            ("tests/empty.test.mjs", false),
            ("tests/missing.test.mjs", true),
        ]
    );
    assert_eq!(
        names(&read.failed()),
        [
            "a parent that fails itself",
            "a parent whose child fails › its failing child",
            "says &quot;hi&quot;",
            "second group › inner › same title",
            "a parent that throws while its child waits",
            "out of its time",
            "registered twice",
            "the parent of a helper’s child › a child a helper registers",
            "tests/missing.test.mjs",
        ]
    );
    // Each test is told from another of its name by its place, which a test registered
    // twice shares.
    assert_eq!(read.tests[10].at, "/checkout/tests/cases.test.mjs:35:3");
    assert_eq!(read.tests[12].at, "/checkout/tests/cases.test.mjs:40:5");
    assert_eq!(read.tests[17].at, "/checkout/tests/cases.test.mjs:67:3");
    assert_eq!(read.tests[18].at, read.tests[17].at);
    assert_eq!(read.tests[19].at, "/checkout/tests/helper.mjs:2:12");
    assert_eq!(
        read.tests[0].file.as_deref(),
        Some("/checkout/tests/broken.test.mjs")
    );
    assert!(read.outside.is_empty());
    // Those that neither failed nor passed are registrations all the same, kept apart,
    // none of them failed.
    assert_eq!(
        said(&read.others),
        [
            ("a todo that fails", false),
            ("a skip", false),
            ("a todo of no reason that fails", false),
            ("a skip of no reason that fails", false),
            (
                "a parent that throws while its child waits › a child its parent cancelled",
                false
            ),
            ("aborted", false),
        ]
    );
    assert_eq!(read.others[1].at, "/checkout/tests/cases.test.mjs:9:1");
}

#[test]
fn a_node_test_file_ran_no_test_where_it_failed_whole_and_its_own_tests_are_none() {
    let read = node(NODE).unwrap();
    // The file that threw after its test is not said at all; the empty file did not fail.
    let whole: Vec<&str> = read
        .tests
        .iter()
        .filter(|test| test.whole)
        .map(|test| test.name.as_str())
        .collect();
    assert_eq!(whole, ["tests/empty.test.mjs", "tests/missing.test.mjs"]);
    assert_eq!(read.files_that_ran_none(), ["tests/missing.test.mjs"]);
    let alone = |lines: &[&str]| {
        let mut report = lines.join("\n");
        report.push_str("\n{\"end\":true}\n");
        node(&report).unwrap()
    };
    // What the reporter wrote under Node 26.8.1 (2026-10-16) for two files, one whose test
    // passed and one that threw after a top-level `await`, before it had a test; the
    // checkout's path cut to `/checkout/`. The other file's test is none of its own.
    let two = alone(&[
        r#"{"path":["passes in another file"],"file":"/checkout/tests/passes.test.mjs","line":3,"column":1,"whole":false,"ended":"passed"}"#,
        r#"{"path":["tests/tla.test.mjs"],"file":"/checkout/tests/tla.test.mjs","line":1,"column":1,"whole":true,"ended":"failed"}"#,
    ]);
    assert_eq!(two.files_that_ran_none(), ["tests/tla.test.mjs"]);
    // A file that failed whole after a test of its own ran ran a test.
    let after = alone(&[
        r#"{"path":["ran first"],"file":"/checkout/tests/a.test.mjs","line":3,"column":1,"whole":false,"ended":"passed"}"#,
        r#"{"path":["tests/a.test.mjs"],"file":"/checkout/tests/a.test.mjs","line":1,"column":1,"whole":true,"ended":"failed"}"#,
    ]);
    assert!(after.files_that_ran_none().is_empty());
    // A test of its own named as a file is, which the reporter does not say is one, is a test.
    let named = r#"{"path":["tests/a.test.mjs"],"file":"/checkout/tests/a.test.mjs","line":1,"column":1,"whole":false,"ended":"failed"}"#;
    let named = alone(&[named]);
    assert!(!named.tests[0].whole);
    assert!(named.files_that_ran_none().is_empty());
}

#[test]
fn a_report_of_node_s_that_is_cut_short_or_is_not_one_is_an_error() {
    let ended = "{\"end\":true}\n";
    assert_eq!(node(ended).unwrap(), Results::default());
    // No end: the run did not get to it, and the tests after are not known.
    let cut = NODE.strip_suffix(ended).unwrap();
    assert!(node(cut).unwrap_err().contains("ends before the run did"));
    assert!(node("").is_err());
    // A line after the end, a line that is not JSON, one that is neither a test nor the end.
    for wrong in [
        format!("{NODE}{ended}"),
        format!("not json\n{ended}"),
        format!("{{}}\n{ended}"),
        "{\"path\":[\"a\"],\"end\":true}\n".to_owned(),
        "{\"ended\":\"passed\",\"end\":true}\n".to_owned(),
        format!("{{\"path\":[\"a\"]}}\n{ended}"),
        format!("{{\"path\":[],\"ended\":\"failed\"}}\n{ended}"),
        format!("{{\"path\":[\"a\"],\"ended\":\"lost\"}}\n{ended}"),
        format!("{{\"path\":[\"a\"],\"ended\":\"passed\",\"more\":1}}\n{ended}"),
        // A line of the reporter of round 3, which said no more than failed and skipped.
        format!("{{\"path\":[\"a\"],\"failed\":true,\"skipped\":false}}\n{ended}"),
    ] {
        assert!(node(&wrong).is_err(), "{wrong}");
    }
}

#[test]
fn a_testcase_marked_skipped_neither_failed_nor_passed_though_it_holds_a_failure() {
    // nextest 0.9.148 writes no case for a test it did not run (an ignored one, 2026-10-16);
    // a JUnit report that marks one skipped, a test to do that failed among them, says
    // nothing of it.
    let xml = r#"<testsuites><testsuite name="x">
        <testcase name="a::to_do" classname="x"><skipped type="todo"/><failure message="fails"/></testcase>
        <testcase name="a::skipped" classname="x"><skipped/></testcase>
        <testcase name="a::runs" classname="x"/>
    </testsuite></testsuites>"#;
    let read = junit(xml).unwrap();
    assert_eq!(said(&read.tests), [("a::runs", false)]);
    // Registered all the same, and none of them failed.
    assert_eq!(
        said(&read.others),
        [("a::to_do", false), ("a::skipped", false)]
    );
}

#[test]
fn a_test_nextest_could_not_start_never_ran_and_is_an_error_outside_every_test() {
    // As nextest 0.9.148 says a test whose binary was taken away after it listed its
    // tests (2026-10-17): its own error where it starts the test itself (Windows); on
    // Unix its double-spawn's status and words. A test that exits with 70 itself, or
    // writes those words and fails otherwise, failed.
    let xml = r#"<testsuites><testsuite name="x">
        <testcase name="a::unstarted" classname="x"><error message="error spawning child process" type="execution failure">error spawning child process
  - No such file or directory (os error 2)</error></testcase>
        <testcase name="a::double_spawned" classname="x"><failure type="test failure with exit code 70"/><system-out></system-out><system-err>error: [double-spawn] failed to exec `&quot;/x/target/debug/deps/x-1&quot; &quot;--exact&quot;`

Caused by:
  No such file or directory (os error 2)
</system-err></testcase>
        <testcase name="a::exits_with_70" classname="x"><failure type="test failure with exit code 70"/><system-err>it exits with 70 itself</system-err></testcase>
        <testcase name="a::says_the_words" classname="x"><failure type="test failure with exit code 101"/><system-err>error: [double-spawn] failed to exec, it says</system-err></testcase>
    </testsuite></testsuites>"#;
    let read = junit(xml).unwrap();
    assert_eq!(
        said(&read.tests),
        [("a::exits_with_70", true), ("a::says_the_words", true)]
    );
    assert_eq!(
        said(&read.others),
        [("a::unstarted", false), ("a::double_spawned", false)]
    );
    assert_eq!(
        read.outside,
        [
            "nextest could not start a::unstarted (x)",
            "nextest could not start a::double_spawned (x)"
        ]
    );
}

#[test]
fn a_report_of_nextest_s_that_is_cut_short_or_is_not_xml_is_an_error_and_a_testcase_with_no_name_too(
) {
    assert!(junit("<testsuites><testcase name=\"a\"/></testcase>").is_err());
    assert!(junit("<testsuites><testcase name=\"a &unknown; b\"/></testsuites>").is_err());
    assert!(junit("<testsuites><testcase classname=\"a\"/></testsuites>").is_err());
    assert!(junit("").is_err());
    let cut = NEXTEST.strip_suffix("</testsuites>").unwrap();
    assert!(junit(cut).unwrap_err().contains("ends before"));
    assert_eq!(
        junit("<testsuites><testsuite>").unwrap_err(),
        "the report ends before its <testsuites> does"
    );
    assert_eq!(junit("<testsuites/>").unwrap(), Results::default());
    assert_eq!(
        junit("<testsuites></testsuites>").unwrap(),
        Results::default()
    );
}

/// What the plants' reporter of Playwright's tests wrote (Playwright 1.63, 2026-10-17)
/// for a spec run in two projects, the second given one retry: a test skipped; a test
/// in a group that passed, having written Playwright's summary of a failure to its
/// standard error, and in the second project failed and then passed, run again; a test
/// that failed on an expectation that quotes Playwright's words for an error outside
/// every test; a test whose group's `beforeAll` threw, its body never run; and a test
/// whose body failed and whose `afterEach` threw.
const PLAYWRIGHT: &str = r#"{"tests":[
{"path":["is skipped"],"file":"/checkout/app/tests/prints.spec.mjs","line":3,"column":6,"project":"first","outcome":"skipped","own":false,"hook":null},
{"path":["a group","the meant one"],"file":"/checkout/app/tests/prints.spec.mjs","line":6,"column":3,"project":"first","outcome":"expected","own":false,"hook":null},
{"path":["another fails"],"file":"/checkout/app/tests/prints.spec.mjs","line":12,"column":1,"project":"first","outcome":"unexpected","own":true,"hook":null},
{"path":["set up","its body never runs"],"file":"/checkout/app/tests/prints.spec.mjs","line":20,"column":3,"project":"first","outcome":"unexpected","own":false,"hook":"Error: the beforeAll throws"},
{"path":["torn down","its body fails too"],"file":"/checkout/app/tests/prints.spec.mjs","line":27,"column":3,"project":"first","outcome":"unexpected","own":true,"hook":"Error: the afterEach throws"},
{"path":["is skipped"],"file":"/checkout/app/tests/prints.spec.mjs","line":3,"column":6,"project":"second","outcome":"skipped","own":false,"hook":null},
{"path":["a group","the meant one"],"file":"/checkout/app/tests/prints.spec.mjs","line":6,"column":3,"project":"second","outcome":"flaky","own":false,"hook":null},
{"path":["another fails"],"file":"/checkout/app/tests/prints.spec.mjs","line":12,"column":1,"project":"second","outcome":"unexpected","own":true,"hook":null},
{"path":["set up","its body never runs"],"file":"/checkout/app/tests/prints.spec.mjs","line":20,"column":3,"project":"second","outcome":"unexpected","own":false,"hook":"Error: the beforeAll throws"},
{"path":["torn down","its body fails too"],"file":"/checkout/app/tests/prints.spec.mjs","line":27,"column":3,"project":"second","outcome":"unexpected","own":true,"hook":"Error: the afterEach throws"}
],"errors":[],"end":true}
"#;

/// The plants' reporter's report (Playwright 1.63) for a spec that imports a module that
/// is not there, and for a run that found no test.
const PLAYWRIGHT_UNIMPORTED: &str = r#"{"tests":[],"errors":["Error: Cannot find module '/checkout/app/tests/not-there.mjs' imported from /checkout/app/tests/missing.spec.mjs","Error: No tests found."],"end":true}
"#;

#[test]
fn playwright_says_each_test_named_from_its_first_group_on_and_whether_its_own_body_failed() {
    let read = playwright(PLAYWRIGHT).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            // The same test, run in two projects, is one test; in the second it failed and
            // then passed, run again, which is no failure.
            ("a group › the meant one", false),
            ("another fails", true),
            // Its body failed, whatever its `afterEach` did.
            ("torn down › its body fails too", true),
        ]
    );
    assert_eq!(
        names(&read.failed()),
        ["another fails", "torn down › its body fails too"]
    );
    assert_eq!(read.tests[0].at, "/checkout/app/tests/prints.spec.mjs:6:3");
    assert_eq!(read.tests[1].at, "/checkout/app/tests/prints.spec.mjs:12:1");
    // The test skipped in every project, and the one whose `beforeAll` threw, whose body
    // never ran, are registered, and neither failed nor passed.
    assert_eq!(
        said(&read.others),
        [
            ("is skipped", false),
            ("set up › its body never runs", false)
        ]
    );
    // The hook's failure is an error outside every test, in each project.
    assert_eq!(
        read.outside,
        [
            "a hook of set up › its body never runs in first failed: Error: the beforeAll throws",
            "a hook of set up › its body never runs in second failed: Error: the beforeAll throws",
        ]
    );
    // A test failed where its body failed in one project of those it was given to; one
    // skipped in every project, or failed by its hooks alone, is one of the others.
    let projects = |outcomes: &[(&str, bool)]| {
        let tests: Vec<String> = outcomes
            .iter()
            .map(|(outcome, own)| {
                format!(
                    r#"{{"path":["t"],"file":"/a.spec.mjs","line":1,"column":1,"project":"","outcome":"{outcome}","own":{own},"hook":null}}"#
                )
            })
            .collect();
        let json = format!(
            r#"{{"tests":[{}],"errors":[],"end":true}}"#,
            tests.join(",")
        );
        let read = playwright(&json).unwrap();
        let owned = |tests: &[Test]| {
            said(tests)
                .into_iter()
                .map(|(name, failed)| (name.to_owned(), failed))
                .collect::<Vec<_>>()
        };
        (owned(&read.tests), owned(&read.others))
    };
    let t = |failed| vec![("t".to_owned(), failed)];
    let failed = ("unexpected", true);
    let hooked = ("unexpected", false);
    assert_eq!(
        projects(&[("expected", false), failed]),
        (t(true), Vec::new())
    );
    assert_eq!(
        projects(&[failed, ("skipped", false)]),
        (t(true), Vec::new())
    );
    assert_eq!(
        projects(&[("skipped", false), ("expected", false)]),
        (t(false), Vec::new())
    );
    assert_eq!(
        projects(&[("skipped", false), ("skipped", false)]),
        (Vec::new(), t(false))
    );
    assert_eq!(projects(&[hooked]), (Vec::new(), t(false)));
    assert_eq!(
        projects(&[hooked, ("expected", false)]),
        (t(false), Vec::new())
    );
}

#[test]
fn playwright_says_the_errors_outside_every_test_each_by_its_first_line() {
    let read = playwright(PLAYWRIGHT_UNIMPORTED).unwrap();
    assert!(read.tests.is_empty());
    assert_eq!(
        read.outside,
        [
            "Error: Cannot find module '/checkout/app/tests/not-there.mjs' imported from /checkout/app/tests/missing.spec.mjs",
            "Error: No tests found.",
        ]
    );
    assert!(playwright("Running 2 tests").is_err());
    // A report that is JSON and not the reporter's is no report of its tests, nor one
    // that does not say the run ended.
    assert!(playwright("{}").is_err());
    assert!(playwright(r#"{"suites": [], "errors": []}"#).is_err());
    assert!(playwright(r#"{"tests":[],"errors":[],"end":false}"#).is_err());
    assert!(playwright(r#"{"tests":[],"errors":[],"end":true}"#).is_ok());
}
