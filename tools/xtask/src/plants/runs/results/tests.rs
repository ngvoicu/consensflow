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

/// What the plants' reporter of Node's tests wrote under Node 26.8.1 (2026-10-16) for
/// three test files, the checkout's path cut to `/checkout/`:
/// - `cases.test.mjs`: a test to do that fails; a test skipped; a parent test that
///   fails after its subtest passed; a parent whose subtest fails; a group; a test
///   named `says &quot;hi&quot;` that fails and one named `says "hi"` that passes; a
///   name of two lines; a title in two groups, the second inside another and failing,
///   printing a line of this report that names `says "hi"` failed;
/// - `empty.test.mjs`, which loads and holds no test;
/// - `broken.test.mjs`, whose test passed before the file threw after an `await`.
const NODE: &str = r#"{"path":["before it throws"],"file":"/checkout/tests/broken.test.mjs","line":3,"column":1,"failed":false,"skipped":false}
{"path":["tests/broken.test.mjs"],"file":"/checkout/tests/broken.test.mjs","line":1,"column":1,"failed":true,"skipped":false}
{"path":["a todo that fails"],"file":"/checkout/tests/cases.test.mjs","line":3,"column":1,"failed":true,"skipped":true}
{"path":["a skip"],"file":"/checkout/tests/cases.test.mjs","line":7,"column":1,"failed":false,"skipped":true}
{"path":["a parent that fails itself","its child passes"],"file":"/checkout/tests/cases.test.mjs","line":10,"column":11,"failed":false,"skipped":false}
{"path":["a parent that fails itself"],"file":"/checkout/tests/cases.test.mjs","line":9,"column":1,"failed":true,"skipped":false}
{"path":["a parent whose child fails","its failing child"],"file":"/checkout/tests/cases.test.mjs","line":15,"column":11,"failed":true,"skipped":false}
{"path":["a parent whose child fails"],"file":"/checkout/tests/cases.test.mjs","line":14,"column":1,"failed":false,"skipped":false}
{"path":["a group","passes inside"],"file":"/checkout/tests/cases.test.mjs","line":21,"column":3,"failed":false,"skipped":false}
{"path":["a group"],"file":"/checkout/tests/cases.test.mjs","line":20,"column":1,"failed":false,"skipped":false}
{"path":["says &quot;hi&quot;"],"file":"/checkout/tests/cases.test.mjs","line":24,"column":1,"failed":true,"skipped":false}
{"path":["says \"hi\""],"file":"/checkout/tests/cases.test.mjs","line":28,"column":1,"failed":false,"skipped":false}
{"path":["two\nlines"],"file":"/checkout/tests/cases.test.mjs","line":30,"column":1,"failed":false,"skipped":false}
{"path":["first group","same title"],"file":"/checkout/tests/cases.test.mjs","line":33,"column":3,"failed":false,"skipped":false}
{"path":["first group"],"file":"/checkout/tests/cases.test.mjs","line":32,"column":1,"failed":false,"skipped":false}
{"path":["second group","inner","same title"],"file":"/checkout/tests/cases.test.mjs","line":38,"column":5,"failed":true,"skipped":false}
{"path":["second group","inner"],"file":"/checkout/tests/cases.test.mjs","line":37,"column":3,"failed":false,"skipped":false}
{"path":["second group"],"file":"/checkout/tests/cases.test.mjs","line":36,"column":1,"failed":false,"skipped":false}
{"path":["tests/empty.test.mjs"],"file":"/checkout/tests/empty.test.mjs","line":1,"column":1,"failed":false,"skipped":false}
{"end":true}
"#;

#[test]
fn node_names_a_test_by_the_titles_it_is_in_and_its_own_exactly_and_a_failure_by_its_own_code() {
    let read = node(NODE).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("before it throws", false),
            ("tests/broken.test.mjs", true),
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
            ("tests/empty.test.mjs", false),
        ]
    );
    assert_eq!(
        names(&read.failed()),
        [
            "tests/broken.test.mjs",
            "a parent that fails itself",
            "a parent whose child fails › its failing child",
            "says &quot;hi&quot;",
            "second group › inner › same title",
        ]
    );
    // Each test is told from another of its name by its place.
    assert_eq!(read.tests[11].at, "/checkout/tests/cases.test.mjs:33:3");
    assert_eq!(read.tests[13].at, "/checkout/tests/cases.test.mjs:38:5");
    assert_eq!(
        read.tests[0].file.as_deref(),
        Some("/checkout/tests/broken.test.mjs")
    );
    assert!(read.outside.is_empty());
}

#[test]
fn a_node_test_file_ran_no_test_where_it_failed_whole_and_its_own_tests_are_none() {
    let read = node(NODE).unwrap();
    // The broken file ran a test before it threw; the empty file did not fail.
    assert!(read.tests[1].is_whole_file());
    assert!(read.tests[16].is_whole_file());
    assert!(read.files_that_ran_none().is_empty());
    let alone = |lines: &[&str]| {
        let mut report = lines.join("\n");
        report.push_str("\n{\"end\":true}\n");
        node(&report).unwrap()
    };
    // What the reporter wrote under Node 26.8.1 (2026-10-16) for two files, one whose test
    // passed and one that threw after a top-level `await`, before it had a test; the
    // checkout's path cut to `/checkout/`. The other file's test is none of its own.
    let two = alone(&[
        r#"{"path":["passes in another file"],"file":"/checkout/tests/passes.test.mjs","line":3,"column":1,"failed":false,"skipped":false}"#,
        r#"{"path":["tests/tla.test.mjs"],"file":"/checkout/tests/tla.test.mjs","line":1,"column":1,"failed":true,"skipped":false}"#,
    ]);
    assert_eq!(two.files_that_ran_none(), ["tests/tla.test.mjs"]);
    let broken = r#"{"path":["tests/broken.test.mjs"],"file":"/checkout/tests/broken.test.mjs","line":1,"column":1,"failed":true,"skipped":false}"#;
    let empty = r#"{"path":["tests/empty.test.mjs"],"file":"/checkout/tests/empty.test.mjs","line":1,"column":1,"failed":false,"skipped":false}"#;
    assert_eq!(
        alone(&[broken]).files_that_ran_none(),
        ["tests/broken.test.mjs"]
    );
    assert!(alone(&[empty]).files_that_ran_none().is_empty());
    // A test of its own named as a file is, in a file of another name, is a test.
    let named = r#"{"path":["config.mjs"],"file":"/checkout/tests/a.test.mjs","line":1,"column":1,"failed":true,"skipped":false}"#;
    let named = alone(&[named]);
    assert!(!named.tests[0].is_whole_file());
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
        format!("{{\"path\":[\"a\"],\"more\":1}}\n{ended}"),
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
    assert_eq!(said(&junit(xml).unwrap().tests), [("a::runs", false)]);
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

/// What Playwright 1.63 wrote (2026-10-16) in its JSON report for a spec whose test in a
/// group passed, having written Playwright's summary of a failure to its standard error,
/// and whose test at the top failed on an expectation that quotes Playwright's words for
/// an error outside every test; cut to what is read. The test before it was skipped.
const PLAYWRIGHT: &str = r#"{
  "config": { "version": "1.63.0" },
  "suites": [
    {
      "title": "prints.spec.mjs",
      "file": "prints.spec.mjs",
      "specs": [
        {
          "title": "is skipped",
          "ok": true,
          "tests": [{ "results": [], "status": "skipped" }],
          "file": "prints.spec.mjs"
        },
        {
          "title": "another fails",
          "ok": false,
          "tests": [
            {
              "results": [
                {
                  "status": "failed",
                  "error": {
                    "message": "Error: expect(received).toBe(expected)\n\nExpected: \"Error: No tests found.\"\nReceived: \"1 error was not a part of any test\""
                  },
                  "stdout": [],
                  "stderr": []
                }
              ],
              "status": "unexpected"
            }
          ],
          "file": "prints.spec.mjs",
          "line": 11,
          "column": 1
        }
      ],
      "suites": [
        {
          "title": "a group",
          "file": "prints.spec.mjs",
          "specs": [
            {
              "title": "the meant one",
              "ok": true,
              "tests": [
                {
                  "results": [
                    {
                      "status": "passed",
                      "errors": [],
                      "stdout": [],
                      "stderr": [{ "text": "  1 failed\n    prints.spec.mjs:4:3 › a group › the meant one \n" }]
                    }
                  ],
                  "status": "expected"
                },
                {
                  "results": [{ "status": "failed" }, { "status": "passed" }],
                  "status": "flaky"
                }
              ],
              "file": "prints.spec.mjs",
              "line": 4,
              "column": 3
            }
          ]
        }
      ]
    }
  ],
  "errors": [],
  "stats": { "expected": 1, "skipped": 1, "unexpected": 1, "flaky": 1 }
}"#;

/// Playwright 1.63's report for a spec that imports a module that is not there.
const PLAYWRIGHT_UNIMPORTED: &str = r#"{
  "config": { "version": "1.63.0" },
  "suites": [],
  "errors": [
    {
      "message": "Error: Cannot find module '/checkout/app/tests/not-there.mjs' imported from /checkout/app/tests/missing.spec.mjs",
      "stack": "Error: Cannot find module '/checkout/app/tests/not-there.mjs' imported from /checkout/app/tests/missing.spec.mjs\n    at eval (<anonymous>:1:1)"
    },
    {
      "message": "Error: No tests found.\nMake sure that arguments are regular expressions matching test files.\nYou may need to escape symbols like \"$\" or \"*\" and quote the arguments.",
      "stack": "Error: No tests found."
    }
  ],
  "stats": { "expected": 0, "skipped": 0, "unexpected": 0, "flaky": 0 }
}"#;

#[test]
fn playwright_says_each_test_named_from_its_first_group_on_and_whether_it_failed_and_nothing_it_printed(
) {
    let read = playwright(PLAYWRIGHT).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("another fails", true),
            ("a group › the meant one", false),
            // A test that failed and then passed, run again, is no failure.
            ("a group › the meant one", false),
        ]
    );
    assert_eq!(names(&read.failed()), ["another fails"]);
    assert!(read.outside.is_empty(), "{:?}", read.outside);
    // The same test, run in two projects, is one test in one place.
    assert_eq!(read.tests[0].at, "prints.spec.mjs:11:1");
    assert_eq!(read.tests[1].at, "prints.spec.mjs:4:3");
    assert_eq!(read.tests[2].at, read.tests[1].at);
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
    // A report that is JSON and not Playwright's is no report of its tests.
    assert!(playwright("{}").is_err());
    assert!(playwright(r#"{"suites": []}"#).is_err());
    assert!(playwright(r#"{"suites": [], "errors": []}"#).is_ok());
}
