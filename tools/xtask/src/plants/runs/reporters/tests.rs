use super::*;

fn words(command: &[&str]) -> Vec<String> {
    command.iter().map(|word| (*word).to_owned()).collect()
}

fn of(command: &[&str]) -> Reporter {
    Reporter::of(&words(command))
}

#[test]
fn a_command_is_reported_by_the_runner_of_its_tests() {
    assert_eq!(of(&["cargo", "test", "-p", "x"]), Reporter::Nextest);
    assert_eq!(
        of(&["cargo", "xtask", "app", "test", "file_drop"]),
        Reporter::Libtest
    );
    assert_eq!(of(&["node", "--test", "tests/a.test.mjs"]), Reporter::Node);
    assert_eq!(
        of(&[
            "node",
            "app/node_modules/@playwright/test/cli.js",
            "test",
            "a.spec.mjs"
        ]),
        Reporter::Playwright
    );
    for status in [
        &["node", "app/scripts/bundle-ui.mjs"][..],
        &["node", "node.exe", "cf.mjs"],
        &["cargo", "build", "-p", "x"],
        &["git", "status"],
        &[],
    ] {
        assert_eq!(of(status), Reporter::Status, "{status:?}");
    }
}

#[test]
fn cargo_test_is_run_by_nextest_with_its_own_words_and_a_config_that_writes_the_report_to_the_run()
{
    let dir = tempfile::tempdir().unwrap();
    let setup = Reporter::Nextest
        .setup(
            &words(&[
                "cargo",
                "test",
                "--offline",
                "-p",
                "x",
                "--lib",
                "a::",
                "--",
                "--skip",
                "b",
            ]),
            dir.path(),
            "/tools/cargo-nextest",
            None,
        )
        .unwrap();
    let config = dir.path().join("nextest.toml");
    assert_eq!(
        setup.words,
        [
            "/tools/cargo-nextest",
            "nextest",
            "run",
            "--profile",
            "plants",
            "--config-file",
            &config.display().to_string(),
            "--offline",
            "-p",
            "x",
            "--lib",
            "a::",
            "--",
            "--skip",
            "b",
        ]
    );
    assert!(setup.env.is_empty());
    let written = fs::read_to_string(&config).unwrap();
    let store = dir.path().display().to_string().replace('\\', "/");
    assert_eq!(
        written,
        format!(
            "[store]\ndir = \"{store}\"\n\n[profile.plants]\nretries = 0\nfail-fast = false\n\n\
             [profile.plants.junit]\npath = \"junit.xml\"\n"
        )
    );
}

#[test]
fn a_suite_of_xtask_s_has_cargo_start_each_test_binary_through_plants_log() {
    let dir = Path::new("/runs/7");
    let command = words(&["cargo", "xtask", "app", "test", "file_drop"]);
    let setup = Reporter::Libtest
        .setup(
            &command,
            dir,
            "",
            Some(("CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER", "/target/xtask")),
        )
        .unwrap();
    assert_eq!(setup.words, command);
    assert_eq!(
        setup.env,
        [(
            OsString::from("CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER"),
            OsString::from(format!("/target/xtask plants-log {}", dir.display())),
        )]
    );
}

#[test]
fn node_writes_its_junit_report_to_the_run_beside_the_report_it_prints() {
    let junit = Path::new("/runs/7/junit.xml");
    assert_eq!(
        node_words(&words(&["node", "--test", "tests/a.test.mjs"]), junit),
        [
            "node",
            "--test-reporter=spec",
            "--test-reporter-destination=stdout",
            "--test-reporter=junit",
            &format!("--test-reporter-destination={}", junit.display()),
            "--test",
            "tests/a.test.mjs",
        ]
    );
    // The report the command asked for is the one printed; where it went is the run's.
    assert_eq!(
        node_words(
            &words(&[
                "node",
                "--test",
                "--test-reporter=tap",
                "--test-reporter-destination=out.txt",
                "tests/a.test.mjs"
            ]),
            junit
        ),
        [
            "node",
            "--test-reporter=tap",
            "--test-reporter-destination=stdout",
            "--test-reporter=junit",
            &format!("--test-reporter-destination={}", junit.display()),
            "--test",
            "tests/a.test.mjs",
        ]
    );
    let setup = Reporter::Node
        .setup(
            &words(&["node", "--test", "a.test.mjs"]),
            Path::new("/runs/7"),
            "",
            None,
        )
        .unwrap();
    assert!(setup.env.is_empty());
    assert!(setup.words.contains(&format!(
        "--test-reporter-destination={}",
        Path::new("/runs/7").join("junit.xml").display()
    )));
}

#[test]
fn playwright_writes_its_json_report_to_the_run_beside_the_list_it_prints() {
    let command = words(&[
        "node",
        "app/node_modules/@playwright/test/cli.js",
        "test",
        "--reporter=dot",
        "app/tests/core-page.spec.mjs",
        "-g",
        "file drop",
    ]);
    let dir = Path::new("/runs/7");
    let setup = Reporter::Playwright.setup(&command, dir, "", None).unwrap();
    assert_eq!(
        setup.words,
        [
            "node",
            "app/node_modules/@playwright/test/cli.js",
            "test",
            "--reporter=list,json",
            "app/tests/core-page.spec.mjs",
            "-g",
            "file drop",
        ]
    );
    assert_eq!(
        setup.env,
        [(
            OsString::from("PLAYWRIGHT_JSON_OUTPUT_FILE"),
            dir.join("report.json").into_os_string(),
        )]
    );
}

#[test]
fn a_command_that_says_no_more_than_its_status_is_run_as_it_is_and_reports_nothing() {
    let command = words(&["node", "app/scripts/bundle-ui.mjs"]);
    let dir = tempfile::tempdir().unwrap();
    let setup = Reporter::Status
        .setup(&command, dir.path(), "", None)
        .unwrap();
    assert_eq!(setup.words, command);
    assert!(setup.env.is_empty());
    fs::write(dir.path().join("junit.xml"), "<testsuites/>").unwrap();
    assert_eq!(Reporter::Status.results(dir.path()), None);
}

#[test]
fn each_report_is_read_from_where_its_runner_wrote_it_and_none_is_none() {
    let dir = tempfile::tempdir().unwrap();
    for reporter in [
        Reporter::Nextest,
        Reporter::Libtest,
        Reporter::Node,
        Reporter::Playwright,
    ] {
        assert_eq!(reporter.results(dir.path()), None, "{reporter:?}");
    }
    let failed = |name: &str| {
        format!("<testsuites><testcase name=\"{name}\"><failure/></testcase></testsuites>")
    };
    fs::create_dir_all(dir.path().join("plants")).unwrap();
    fs::write(dir.path().join("plants/junit.xml"), failed("a::nextest")).unwrap();
    fs::write(dir.path().join("junit.xml"), failed("node")).unwrap();
    fs::write(
        dir.path().join("report.json"),
        r#"{"suites": [{"title": "a.spec.mjs", "specs": [{"title": "page", "tests": [{"status": "unexpected"}]}]}], "errors": []}"#,
    )
    .unwrap();
    // A log for each test binary, in the order cargo ran them.
    fs::write(dir.path().join("0001-b.log"), "failed b::second\n").unwrap();
    fs::write(dir.path().join("0000-a.log"), "ok a::first\n").unwrap();
    let failed_of = |reporter: Reporter| -> Vec<String> {
        let results = reporter.results(dir.path()).unwrap().unwrap();
        results
            .tests
            .iter()
            .map(|test| format!("{} {}", test.name, test.failed))
            .collect()
    };
    assert_eq!(failed_of(Reporter::Nextest), ["a::nextest true"]);
    assert_eq!(failed_of(Reporter::Node), ["node true"]);
    assert_eq!(failed_of(Reporter::Playwright), ["page true"]);
    assert_eq!(
        failed_of(Reporter::Libtest),
        ["a::first false", "b::second true"]
    );
    // A report that cannot be read says why.
    fs::write(dir.path().join("report.json"), "{").unwrap();
    assert!(matches!(
        Reporter::Playwright.results(dir.path()),
        Some(Err(_))
    ));
}

#[test]
fn the_folder_of_nextest_s_store_is_written_with_forward_slashes() {
    let config = nextest_config(Path::new(
        r"C:\checkout\app\src-tauri\target\plants-runs\run-1",
    ));
    assert!(
        config
            .starts_with("[store]\ndir = \"C:/checkout/app/src-tauri/target/plants-runs/run-1\"\n"),
        "{config}"
    );
}
