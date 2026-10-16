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
    let read = junit(NEXTEST, false).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("tests::passes", false),
            ("tests::the_meant_one_passes_and_prints_a_footer", false),
            ("tests::another_fails_quoting_a_build", true),
        ]
    );
    assert_eq!(
        names(&read.failed()),
        ["tests::another_fails_quoting_a_build"]
    );
    assert!(read.outside.is_empty());
    // Libtest's words, a failures list and a footer, in what a passing test printed.
    let kept = junit(NEXTEST_KEPT_OUTPUT, false).unwrap();
    assert_eq!(
        names(&kept.failed()),
        ["tests::another_fails_quoting_a_build"]
    );
    assert_eq!(kept.tests.len(), 2);
}

/// What Node 26.8.1 wrote (2026-10-16) in its JUnit report for a file of four tests:
/// two in a group whose name holds `&`, `<` and `"`, one of them failing with an error
/// that quotes a build that failed, named with a `#` that Node's TAP escapes; one at the
/// top that fails; one at the top that passes. The test that fails printed a forged
/// `<testcase>`, which Node keeps out of the report. The checkout's path cut to
/// `/checkout/`.
const NODE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites>
	<testsuite name="a group &amp; &lt;its> &amp;quot;kind&amp;quot;" time="0.000999" disabled="0" errors="0" tests="2" failures="1" skipped="0" timestamp="2026-10-16T05:15:12.449Z" hostname="host">
		<testcase name="passes inside" time="0.000270" classname="a group &amp; &lt;its> &amp;quot;kind&amp;quot;" file="/checkout/tests/mixed.test.mjs"/>
		<testcase name="the meant one # marked fails inside" time="0.000301" classname="a group &amp; &lt;its> &amp;quot;kind&amp;quot;" file="/checkout/tests/mixed.test.mjs" failure="could not compile the response, error[E0308]">
			<failure type="testCodeFailure" message="could not compile the response, error[E0308]">
Error [ERR_TEST_FAILURE]: could not compile the response, error[E0308]
    at async Promise.all (index 0) {
  code: 'ERR_TEST_FAILURE',
  failureType: 'testCodeFailure',
  cause: Error: could not compile the response, error[E0308]
      at TestContext.&lt;anonymous> (file:///checkout/tests/mixed.test.mjs:7:11)
}
			</failure>
		</testcase>
	</testsuite>
	<testcase name="a top-level one fails" time="0.000044" classname="test" file="/checkout/tests/mixed.test.mjs" failure="top failure">
		<failure type="testCodeFailure" message="top failure">
[Error [ERR_TEST_FAILURE]: top failure] {
  code: 'ERR_TEST_FAILURE',
  failureType: 'testCodeFailure',
  cause: Error: top failure
}
		</failure>
	</testcase>
	<testcase name="says &amp;quot;hi&amp;quot; &amp; &lt;bye>" time="0.000454" classname="test" file="/checkout/tests/mixed.test.mjs"/>
	<!-- tests 4 -->
	<!-- suites 1 -->
	<!-- pass 2 -->
	<!-- fail 2 -->
</testsuites>
"#;

#[test]
fn node_names_a_test_by_its_own_title_as_written_however_its_reporters_escape_it() {
    let read = junit(NODE, true).unwrap();
    assert_eq!(
        said(&read.tests),
        [
            ("passes inside", false),
            ("the meant one # marked fails inside", true),
            ("a top-level one fails", true),
            (r#"says "hi" & <bye>"#, false),
        ]
    );
    assert!(read
        .tests
        .iter()
        .all(|test| test.file.as_deref() == Some("/checkout/tests/mixed.test.mjs")));
    assert!(read.files_that_ran_none().is_empty());
    // nextest's report escapes once.
    let once = r#"<testsuites><testcase name="says &quot;hi&quot;"/></testsuites>"#;
    assert_eq!(
        said(&junit(once, false).unwrap().tests),
        [(r#"says "hi""#, false)]
    );
}

/// What Node 26.8.1 wrote (2026-10-16) for two test files, one whose test passed and
/// one that threw after a top-level `await`, before it had a test: Node's own counts
/// are of the two files together.
const NODE_TWO_FILES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites>
	<testcase name="passes in another file" time="0.000288" classname="test" file="/checkout/tests/passes.test.mjs"/>
	<testcase name="tests/tla.test.mjs" time="0.029056" classname="test" file="/checkout/tests/tla.test.mjs" failure="test failed">
		<failure type="testCodeFailure" message="test failed">
[Error: test failed] { code: 'ERR_TEST_FAILURE', failureType: 'testCodeFailure', cause: 'test failed', exitCode: 1, signal: null }
		</failure>
	</testcase>
	<!-- tests 2 -->
	<!-- suites 0 -->
	<!-- pass 1 -->
	<!-- fail 1 -->
</testsuites>
"#;

/// A file whose first test passed and whose second ended the process after a wait.
const NODE_ENDED_LATE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites>
	<testcase name="passes first" time="0.000288" classname="test" file="/checkout/tests/late.test.mjs"/>
	<testcase name="tests/late.test.mjs" time="0.086890" classname="test" file="/checkout/tests/late.test.mjs" failure="test failed">
		<failure type="testCodeFailure" message="test failed">
[Error: test failed] { code: 'ERR_TEST_FAILURE', failureType: 'testCodeFailure', cause: 'test failed', exitCode: 3, signal: null }
		</failure>
	</testcase>
	<!-- tests 2 -->
</testsuites>
"#;

/// A file that does not parse, in a folder with a space in its name.
const NODE_UNPARSED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites>
	<testcase name="tests/with spaces/syntax.test.mjs" time="0.029564" classname="test" file="/checkout/tests/with spaces/syntax.test.mjs" failure="test failed">
		<failure type="testCodeFailure" message="test failed">
[Error: test failed] { code: 'ERR_TEST_FAILURE', failureType: 'testCodeFailure', cause: 'test failed', exitCode: 1, signal: null }
		</failure>
	</testcase>
	<!-- tests 1 -->
</testsuites>
"#;

#[test]
fn a_node_test_file_ran_no_test_where_its_own_tests_are_none_whatever_the_other_files_ran() {
    let two = junit(NODE_TWO_FILES, true).unwrap();
    assert_eq!(two.files_that_ran_none(), ["tests/tla.test.mjs"]);
    assert_eq!(names(&two.failed()), ["tests/tla.test.mjs"]);
    assert!(two.failed()[0].is_whole_file());
    let unparsed = junit(NODE_UNPARSED, true).unwrap();
    assert_eq!(
        unparsed.files_that_ran_none(),
        ["tests/with spaces/syntax.test.mjs"]
    );
    // A file that failed whole once tests of it ran ran them.
    let late = junit(NODE_ENDED_LATE, true).unwrap();
    assert!(late.files_that_ran_none().is_empty());
    assert!(late.failed()[0].is_whole_file());
    // A test of its own named as a file is, in a file of another name, is a test.
    let named = r#"<testsuites><testcase name="config.mjs" file="/checkout/tests/a.test.mjs"><failure/></testcase></testsuites>"#;
    let named = junit(named, true).unwrap();
    assert!(!named.tests[0].is_whole_file());
    assert!(named.files_that_ran_none().is_empty());
}

#[test]
fn a_report_that_is_not_xml_is_an_error_and_a_testcase_with_no_name_too() {
    assert!(junit("<testsuites><testcase name=\"a\"/></testcase>", false).is_err());
    assert!(junit(
        "<testsuites><testcase name=\"a &unknown; b\"/></testsuites>",
        false
    )
    .is_err());
    assert!(junit("<testsuites><testcase file=\"a\"/></testsuites>", true).is_err());
    assert_eq!(junit("", false).unwrap(), Results::default());
}

/// What libtest wrote to its log (`--logfile`, the stable toolchain of 2026-10-16) for a binary of
/// five tests: one ignored with a reason, two that passed, a `should_panic` test whose
/// panic was another (libtest says why on lines of their own, the panic quoted with a
/// line that is libtest's words for a test that passed), and one that failed.
const LIBTEST_LOG: &str = "ignored: asked for by name tests::ignored_with_a_reason
ok tests::passes
ok tests::the_meant_one_passes_and_prints_a_footer
failed: panic did not contain expected string
      panic message: \"another\\nok tests::passes\"
 expected substring: \"the one\" tests::panics_with_another
failed tests::another_fails_quoting_a_build
";

#[test]
fn libtest_s_log_says_each_test_that_ran_and_whether_it_failed_its_name_last() {
    let read = libtest_logs(&[
        LIBTEST_LOG.to_owned(),
        "ok b::one\nfailed b::two\n".to_owned(),
    ]);
    assert_eq!(
        said(&read.tests),
        [
            ("tests::passes", false),
            ("tests::the_meant_one_passes_and_prints_a_footer", false),
            ("tests::panics_with_another", true),
            ("tests::another_fails_quoting_a_build", true),
            ("b::one", false),
            ("b::two", true),
        ]
    );
    assert_eq!(libtest_logs(&[String::new()]), Results::default());
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
          "file": "prints.spec.mjs"
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
              "file": "prints.spec.mjs"
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
}
