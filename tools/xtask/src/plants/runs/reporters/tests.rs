use cf_base::env::Env;

use super::*;

fn words(command: &[&str]) -> Vec<String> {
    command.iter().map(|word| (*word).to_owned()).collect()
}

fn of(command: &[&str]) -> Reporter {
    Reporter::of(&words(command))
}

/// A checkout at `root`, which nothing is read from.
fn checkout(root: &Path) -> Context {
    Context {
        root: root.to_path_buf(),
        env: Env::default(),
    }
}

/// The words of `invocation` after its program.
fn args(invocation: &Invocation) -> Vec<String> {
    invocation
        .args
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_command_is_reported_by_the_runner_of_its_tests() {
    assert_eq!(of(&["cargo", "test", "-p", "x"]), Reporter::Nextest);
    // A suite of xtask's that is a `cargo test` is that test's, under nextest.
    for suite in [
        &["cargo", "xtask", "app", "test", "file_drop"][..],
        &["cargo", "xtask", "app", "test"],
        &["cargo", "xtask", "test", "clis", "--offline"],
        &["cargo", "xtask", "test", "agents", "--offline"],
    ] {
        assert_eq!(of(suite), Reporter::Nextest, "{suite:?}");
    }
    let smoke = if cfg!(target_os = "macos") {
        Reporter::Nextest
    } else {
        Reporter::Status
    };
    assert_eq!(of(&["cargo", "xtask", "smoke"]), smoke);
    assert_eq!(of(&["cargo", "xtask", "smoke", "--app", "a.app"]), smoke);
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
        &["cargo", "xtask", "build-cf", "--offline"],
        &["cargo", "xtask", "smoke-updater", "--export-bridge"],
        &["cargo", "xtask", "test", "no-such-suite"],
        &["cargo", "xtask", "smoke", "--no-such-option"],
        &["cargo", "xtask", "app", "clippy"],
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
    let context = checkout(Path::new("/checkout"));
    let invocation = Reporter::Nextest
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
            &context,
            "/tools/cargo-nextest",
        )
        .unwrap();
    let config = dir.path().join("nextest.toml");
    assert_eq!(invocation.program, "/tools/cargo-nextest");
    assert_eq!(
        args(&invocation),
        [
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
    assert_eq!(invocation.cwd, Path::new("/checkout"));
    assert!(invocation.vars.is_empty());
    let written = fs::read_to_string(&config).unwrap();
    let store = dir.path().display().to_string().replace('\\', "/");
    assert_eq!(
        written,
        format!(
            "[store]\ndir = \"{store}\"\n\n[profile.plants]\nretries = 0\nfail-fast = false\n\n\
             [profile.plants.junit]\npath = \"junit.xml\"\nreport-skipped = \"all\"\n"
        )
    );
}

/// What nextest is asked to run for `command`, a suite of xtask's, in `context`: its
/// words after the config file, its folder and its variables.
fn under_nextest_for(command: &[&str], context: &Context) -> (Vec<String>, PathBuf, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let invocation = Reporter::of(&words(command))
        .setup(&words(command), dir.path(), context, "/tools/cargo-nextest")
        .unwrap();
    assert_eq!(invocation.program, "/tools/cargo-nextest");
    let all = args(&invocation);
    assert_eq!(
        all[..5],
        ["nextest", "run", "--profile", "plants", "--config-file"]
    );
    let vars = invocation
        .vars
        .iter()
        .map(|(name, value)| format!("{}={:?}", name.to_string_lossy(), value))
        .collect();
    (all[6..].to_vec(), invocation.cwd, vars)
}

#[test]
fn a_suite_of_xtask_s_is_the_cargo_test_it_runs_under_nextest_in_its_folder_with_its_variables() {
    let context = checkout(Path::new("/checkout"));
    let (words, cwd, vars) =
        under_nextest_for(&["cargo", "xtask", "app", "test", "file_drop"], &context);
    assert_eq!(words, ["--offline", "--", "file_drop"]);
    assert_eq!(cwd, Path::new("/checkout").join("app/src-tauri"));
    assert_eq!(
        vars,
        [r#"TAURI_CONFIG=Some("{\"bundle\":{\"resources\":null}}")"#]
    );
    let (words, cwd, vars) =
        under_nextest_for(&["cargo", "xtask", "test", "agents", "--offline"], &context);
    assert_eq!(
        words,
        [
            "-p",
            "cf-e2e",
            "--test",
            "daemon",
            "agents_proof::",
            "--offline"
        ]
    );
    assert_eq!(cwd, Path::new("/checkout"));
    assert!(vars.is_empty());
    if cfg!(target_os = "macos") {
        let (words, cwd, vars) =
            under_nextest_for(&["cargo", "xtask", "smoke", "--app", "a.app"], &context);
        assert_eq!(
            words,
            [
                "-p",
                "cf-e2e",
                "--test",
                "smoke",
                "--",
                "--ignored",
                "--nocapture"
            ]
        );
        assert_eq!(cwd, Path::new("/checkout"));
        let app = Path::new("/checkout").join("a.app");
        assert_eq!(vars, [format!("CONSENSFLOW_SMOKE_APP=Some({:?})", app)]);
    }
}

#[test]
fn node_writes_the_plants_report_to_the_run_beside_the_report_it_prints() {
    let reporter = "./app/src-tauri/target/plants-runs/run-7/plants.mjs";
    let report = Path::new("/runs/7/node.jsonl");
    assert_eq!(
        node_words(
            &words(&["node", "--test", "tests/a.test.mjs"]),
            reporter,
            report
        ),
        [
            "--test-isolation=none",
            "--test-reporter=spec",
            "--test-reporter-destination=stdout",
            &format!("--test-reporter={reporter}"),
            &format!("--test-reporter-destination={}", report.display()),
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
            reporter,
            report
        ),
        [
            "--test-isolation=none",
            "--test-reporter=tap",
            "--test-reporter-destination=stdout",
            &format!("--test-reporter={reporter}"),
            &format!("--test-reporter-destination={}", report.display()),
            "--test",
            "tests/a.test.mjs",
        ]
    );
    // The tests run in Node's own process whatever isolation the command asks for, in
    // either way of giving an option its value; the report it names first is printed.
    for asked in [
        &["--test-isolation=process", "--test-reporter", "dot"][..],
        &["--test-isolation", "process", "--test-reporter=dot"],
        &[
            "--experimental-test-isolation=process",
            "--test-reporter=dot",
        ],
        &[
            "--test-reporter=dot",
            "--test-reporter-destination",
            "out.txt",
            "--test-reporter=tap",
        ],
    ] {
        let command: Vec<&str> = ["node", "--test"]
            .into_iter()
            .chain(asked.iter().copied())
            .chain(["tests/a.test.mjs"])
            .collect();
        assert_eq!(
            node_words(&words(&command), reporter, report),
            [
                "--test-isolation=none",
                "--test-reporter=dot",
                "--test-reporter-destination=stdout",
                &format!("--test-reporter={reporter}"),
                &format!("--test-reporter-destination={}", report.display()),
                "--test",
                "tests/a.test.mjs",
            ],
            "{command:?}"
        );
    }
    // The reporter is written into the run's folder, and named from the checkout's root.
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("app/src-tauri/target/plants-runs/run-7");
    fs::create_dir_all(&dir).unwrap();
    let invocation = Reporter::Node
        .setup(
            &words(&["node", "--test", "a.test.mjs"]),
            &dir,
            &checkout(root.path()),
            "",
        )
        .unwrap();
    assert_eq!(invocation.program, "node");
    assert_eq!(invocation.cwd, root.path());
    assert!(invocation.vars.is_empty());
    assert_eq!(
        fs::read_to_string(dir.join("plants.mjs")).unwrap(),
        NODE_REPORTER
    );
    let said = args(&invocation);
    assert!(
        said.contains(&format!("--test-reporter={reporter}")),
        "{said:?}"
    );
    assert!(said.contains(&format!(
        "--test-reporter-destination={}",
        dir.join("node.jsonl").display()
    )));
}

/// Drives the plants' reporter of Playwright's tests by hand: a test whose error is said
/// by a step two below its hook's step alone, and one whose error is its body's.
const PLAYWRIGHT_DRIVEN: &str = r"import Plants from './plants-playwright.mjs'

const plants = new Plants()
const test = (title) => {
  const result = { status: 'failed', errors: [{ message: 'Error: inside' }] }
  const test = {
    results: [result],
    titlePath: () => ['', 'p', 'a.spec.mjs', title],
    location: { file: 'a.spec.mjs', line: 1, column: 1 },
    parent: { project: () => ({ name: 'p' }) },
    outcome: () => 'unexpected',
  }
  return [test, result]
}
const [hooked, inHook] = test('hooked')
const [own, ofItsOwn] = test('own')
plants.onBegin({}, { allTests: () => [hooked, own] })
const hook = { category: 'hook', title: 'Before Hooks' }
const fixture = { category: 'fixture', title: 'a fixture', parent: hook }
plants.onStepEnd(hooked, inHook, { category: 'expect', title: 'soft', parent: fixture, error: { message: 'Error: inside' } })
plants.onStepEnd(hooked, inHook, fixture)
plants.onStepEnd(hooked, inHook, hook)
plants.onStepEnd(own, ofItsOwn, hook)
plants.onStepEnd(own, ofItsOwn, { category: 'expect', title: 'toBe', error: { message: 'Error: inside' } })
plants.onEnd()
";

#[test]
fn playwright_s_reporter_takes_an_error_of_any_step_in_a_hook_for_the_hook_s() {
    // Round 7, decision 2: a hook's error, wherever in the hook's steps it is said, is the
    // hook's (Playwright 1.63 says it again on each step it is in, which another version
    // need not); a body's own step is no hook's.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("plants-playwright.mjs"),
        PLAYWRIGHT_REPORTER,
    )
    .unwrap();
    fs::write(dir.path().join("driven.mjs"), PLAYWRIGHT_DRIVEN).unwrap();
    let report = dir.path().join("report.json");
    let driven = crate::process::capture(
        &Invocation::new("node", dir.path())
            .arg("driven.mjs")
            .var(PLAYWRIGHT_REPORT_VAR, &report),
        &Env::from_process(),
    )
    .unwrap();
    assert_eq!(driven.code, 0, "{driven:?}");
    let said = fs::read_to_string(&report).unwrap();
    assert!(
        said.contains(r#""path":["hooked"],"#)
            && said.contains(r#""own":false,"why":"a hook failed: Error: inside"}"#),
        "{said}"
    );
    assert!(
        said.contains(r#""path":["own"],"#) && said.contains(r#""own":true,"why":null}"#),
        "{said}"
    );
}

#[test]
fn playwright_writes_the_plants_report_to_the_run_beside_the_list_it_prints() {
    let command = words(&[
        "node",
        "app/node_modules/@playwright/test/cli.js",
        "test",
        "--reporter=dot",
        "app/tests/core-page.spec.mjs",
        "-g",
        "file drop",
    ]);
    // The reporter is written into the run's folder, and named from the checkout's root;
    // the report goes where the variable it reads says.
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("app/src-tauri/target/plants-runs/run-7");
    fs::create_dir_all(&dir).unwrap();
    let invocation = Reporter::Playwright
        .setup(&command, &dir, &checkout(root.path()), "")
        .unwrap();
    assert_eq!(invocation.program, "node");
    assert_eq!(invocation.cwd, root.path());
    assert_eq!(
        fs::read_to_string(dir.join("plants-playwright.mjs")).unwrap(),
        PLAYWRIGHT_REPORTER
    );
    assert_eq!(
        args(&invocation),
        [
            "app/node_modules/@playwright/test/cli.js",
            "test",
            "--reporter=list,./app/src-tauri/target/plants-runs/run-7/plants-playwright.mjs",
            "--retries=0",
            "app/tests/core-page.spec.mjs",
            "-g",
            "file drop",
        ]
    );
    // One attempt of each test, whatever the config says: a command's own retries of
    // none are given once; any others refuse the run (the first reviewer's look at
    // round 6: an attempt after the one that caught the plant could unsay it).
    let reporter = "./plants-playwright.mjs";
    for asked in [&["--retries=0"][..], &["--retries", "0"]] {
        let command: Vec<String> = ["node", "cli.js", "test"]
            .iter()
            .chain(asked)
            .chain(&["a.spec.mjs"])
            .map(|word| (*word).to_owned())
            .collect();
        assert_eq!(retried(&command), None, "{asked:?}");
        assert_eq!(
            playwright_words(&command, reporter),
            [
                "node",
                "cli.js",
                "test",
                "--reporter=list,./plants-playwright.mjs",
                "--retries=0",
                "a.spec.mjs"
            ],
            "{asked:?}"
        );
    }
    for asked in [&["--retries=2"][..], &["--retries", "1"], &["--retries"]] {
        let command: Vec<String> = ["node", "cli.js", "test"]
            .iter()
            .chain(asked)
            .map(|word| (*word).to_owned())
            .collect();
        let why = retried(&command).unwrap_or_default();
        assert!(
            why.starts_with("Playwright's --retries "),
            "{asked:?}: {why}"
        );
        assert!(why.ends_with(": a plant's run takes one attempt"), "{why}");
        assert_eq!(
            Reporter::Playwright.refused(&command, &checkout(Path::new("/checkout"))),
            Some(why)
        );
    }
    assert_eq!(
        invocation.vars,
        [(
            OsString::from("PLANTS_PLAYWRIGHT_REPORT"),
            Some(dir.join("playwright.json").into_os_string()),
        )]
    );
}

#[test]
fn a_command_that_says_no_more_than_its_status_is_run_as_it_is_and_reports_nothing() {
    let command = words(&["cargo", "xtask", "smoke-updater", "--export-bridge"]);
    let dir = tempfile::tempdir().unwrap();
    let invocation = Reporter::Status
        .setup(&command, dir.path(), &checkout(Path::new("/checkout")), "")
        .unwrap();
    assert_eq!(invocation.program, "cargo");
    assert_eq!(args(&invocation), command[1..]);
    assert_eq!(invocation.cwd, Path::new("/checkout"));
    assert!(invocation.vars.is_empty());
    fs::write(dir.path().join("junit.xml"), "<testsuites/>").unwrap();
    assert_eq!(Reporter::Status.results(dir.path()), None);
}

#[test]
fn each_report_is_read_from_where_its_runner_wrote_it_and_one_that_is_not_there_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    for reporter in [Reporter::Nextest, Reporter::Node, Reporter::Playwright] {
        let said = reporter.results(dir.path()).unwrap().unwrap_err();
        assert!(said.contains("could not be read"), "{reporter:?}: {said}");
    }
    fs::create_dir_all(dir.path().join("plants")).unwrap();
    fs::write(
        dir.path().join("plants/junit.xml"),
        "<testsuites><testcase name=\"a::nextest\"><failure/></testcase></testsuites>",
    )
    .unwrap();
    fs::write(
        dir.path().join("node.jsonl"),
        "{\"path\":[\"node\"],\"file\":\"/a.test.mjs\",\"line\":1,\"column\":1,\"whole\":false,\"ended\":\"failed\"}\n{\"end\":true}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("playwright.json"),
        r#"{"tests":[{"path":["page"],"file":"/a.spec.mjs","line":1,"column":1,"project":"","outcome":"unexpected","own":true,"why":null}],"errors":[],"end":true}"#,
    )
    .unwrap();
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
    // A report that is empty, cut short or not one says why, with its path, as the
    // system writes it (with `\` on Windows: zeewin, 2026-10-17).
    for (file, text, reporter) in [
        (&["plants", "junit.xml"][..], "", Reporter::Nextest),
        (
            &["plants", "junit.xml"],
            "<testsuites><testcase name=\"a\"/>",
            Reporter::Nextest,
        ),
        (&["node.jsonl"], "", Reporter::Node),
        (&["playwright.json"], "{", Reporter::Playwright),
    ] {
        let path = file
            .iter()
            .fold(dir.path().to_path_buf(), |path, part| path.join(part));
        fs::write(&path, text).unwrap();
        let said = reporter.results(dir.path()).unwrap().unwrap_err();
        assert!(said.starts_with(&path.display().to_string()), "{said}");
    }
}

#[test]
fn a_run_of_cf_e2e_s_tests_is_told_from_another_and_every_run_s_tests_run_beside_each_other() {
    let names = |line: &[&str]| names_cf_e2e(&line.iter().map(OsString::from).collect::<Vec<_>>());
    assert!(names(&[
        "test",
        "--offline",
        "-p",
        "cf-e2e",
        "--test",
        "rig"
    ]));
    assert!(names(&["test", "--package", "cf-e2e"]));
    assert!(names(&["test", "--package=cf-e2e"]));
    assert!(names(&["test", "-pcf-e2e"]));
    assert!(!names(&["test", "-p", "cf-engine", "--test", "cf-e2e"]));
    assert!(!names(&["test", "-p", "cf", "--", "-p", "cf-e2e"]));
    assert!(!names(&["test", "--offline", "--", "file_drop"]));
    // The suites of xtask's that are tests of cf-e2e name it; none is run one test at a
    // time.
    let dir = tempfile::tempdir().unwrap();
    let context = checkout(Path::new("/checkout"));
    for (suite, cf_e2e) in [
        (&["cargo", "xtask", "test", "clis", "--offline"][..], true),
        (&["cargo", "xtask", "app", "test", "file_drop"], false),
        (
            &[
                "cargo",
                "test",
                "--offline",
                "-p",
                "cf-e2e",
                "--test",
                "regressions",
            ],
            true,
        ),
        (&["cargo", "test", "--offline", "-p", "cf", "--lib"], false),
        (&["node", "--test", "-p", "cf-e2e"], false),
    ] {
        let reporter = Reporter::of(&words(suite));
        assert_eq!(
            reporter.runs_cf_e2e(&words(suite), &context),
            cf_e2e,
            "{suite:?}"
        );
        if reporter == Reporter::Nextest {
            reporter
                .setup(&words(suite), dir.path(), &context, "")
                .unwrap();
            let config = fs::read_to_string(dir.path().join("nextest.toml")).unwrap();
            assert!(!config.contains("test-threads"), "{suite:?}: {config}");
        }
    }
}

#[test]
fn each_option_of_cargo_test_is_given_to_nextest_as_it_names_what_cargo_means_or_refused() {
    let given = |line: &[&str]| -> Result<Vec<String>, String> {
        let args: Vec<OsString> = ["test"].iter().chain(line).map(OsString::from).collect();
        nextest_words(&args).map(|words| {
            words
                .iter()
                .map(|word| word.to_string_lossy().into_owned())
                .collect()
        })
    };
    // As they are: nextest's options of these names are cargo's.
    let same = [
        "--offline",
        "-p",
        "x",
        "--package=y",
        "-pz",
        "--lib",
        "--test",
        "rig",
        "--features",
        "local",
        "-Flocal",
        "--target-dir",
        "t",
        "--no-fail-fast",
        "--release",
        "a::",
        "--",
        "--skip",
        "b",
        "--profile",
        "-j",
    ];
    assert_eq!(given(&same).unwrap(), same);
    // As nextest names what cargo means: a profile of cargo's, jobs that build, cargo's
    // own output; and a short name nextest has not.
    for (cargo, nextest) in [
        (
            &["--profile", "release"][..],
            &["--cargo-profile", "release"][..],
        ),
        (&["--profile=release"], &["--cargo-profile", "release"]),
        (&["-j", "8"], &["--build-jobs", "8"]),
        (&["-j8"], &["--build-jobs", "8"]),
        (&["--jobs=8"], &["--build-jobs", "8"]),
        (&["-v"], &["--cargo-verbose"]),
        (&["-vv"], &["--cargo-verbose"]),
        (&["--verbose"], &["--cargo-verbose"]),
        (&["-q"], &["--cargo-quiet"]),
        (
            &["--message-format", "json"],
            &["--cargo-message-format", "json"],
        ),
        (
            &["-m", "a/Cargo.toml"],
            &["--manifest-path", "a/Cargo.toml"],
        ),
    ] {
        assert_eq!(given(cargo).unwrap(), nextest, "{cargo:?}");
    }
    // Refused, and why.
    for (cargo, why) in [
        (&["--doc"][..], "cargo test --doc: nextest runs no doctest"),
        (
            &["--no-run"],
            "cargo test --no-run: it runs no test, which no plant can be caught by",
        ),
        (
            &["-Z", "unstable-options"],
            "cargo test -Z: nextest takes no unstable flag of cargo's",
        ),
        (&["--help"], "cargo test --help: it runs no test"),
        (
            &["--frobnicate"],
            "cargo test --frobnicate: no option of cargo test",
        ),
        (&["-x"], "cargo test -x: no option of cargo test"),
        (&["-qv"], "cargo test -qv: write each short option apart"),
        (&["--lib=x"], "cargo test --lib=x: --lib takes no value"),
        (
            &["--features"],
            "cargo test --features: --features is given no value",
        ),
        (
            &["-p", "--lib"],
            "cargo test -p: --package is given no value",
        ),
    ] {
        assert_eq!(given(cargo).unwrap_err(), why, "{cargo:?}");
    }
    // A command so refused is no run at all, and a suite of xtask's is read for it too.
    let context = checkout(Path::new("/checkout"));
    assert_eq!(
        Reporter::Nextest
            .refused(&words(&["cargo", "test", "--doc"]), &context)
            .as_deref(),
        Some("cargo test --doc: nextest runs no doctest")
    );
    assert!(Reporter::Nextest
        .refused(
            &words(&["cargo", "xtask", "test", "clis", "--no-run"]),
            &context
        )
        .is_some());
    assert!(Reporter::Nextest
        .refused(
            &words(&["cargo", "xtask", "test", "clis", "--profile", "release"]),
            &context
        )
        .is_none());
    assert_eq!(
        Reporter::Node.refused(&words(&["node", "--test", "--doc"]), &context),
        None
    );
}

#[test]
fn nextest_is_given_none_of_the_variables_of_its_own_that_this_run_was_given() {
    // As a run of the plants under a nextest of its own (the tooling table's) is given
    // them, or a shell that sets them: each would win over the run's config.
    let context = Context {
        root: PathBuf::from("/checkout"),
        env: Env::from_vars([
            ("NEXTEST_TEST_THREADS", "8"),
            ("NEXTEST_RETRIES", "2"),
            ("PATH", "/bin"),
        ]),
    };
    let dir = tempfile::tempdir().unwrap();
    let invocation = Reporter::Nextest
        .setup(
            &words(&["cargo", "test", "-p", "cf-e2e"]),
            dir.path(),
            &context,
            "",
        )
        .unwrap();
    let mut taken: Vec<(String, Option<OsString>)> = invocation
        .vars
        .iter()
        .map(|(name, value)| (name.to_string_lossy().into_owned(), value.clone()))
        .collect();
    taken.sort();
    assert_eq!(
        taken,
        [
            ("NEXTEST_RETRIES".to_owned(), None),
            ("NEXTEST_TEST_THREADS".to_owned(), None)
        ]
    );
}

#[test]
fn a_cargo_test_whose_suite_turns_capture_off_is_refused_by_its_variable() {
    // A suite of xtask's that set nextest's or libtest's variable for it would run its
    // tests with capture off, whatever the words say.
    let tests = |vars: &[(&str, Option<&str>)]| Invocation {
        program: OsString::from("cargo"),
        args: ["test", "-p", "x"].map(OsString::from).to_vec(),
        cwd: PathBuf::from("/checkout"),
        vars: vars
            .iter()
            .map(|(name, value)| (OsString::from(name), value.map(OsString::from)))
            .collect(),
    };
    for name in ["NEXTEST_NO_CAPTURE", "RUST_TEST_NOCAPTURE"] {
        let why = uncaptured(&tests(&[(name, Some("1"))])).unwrap_or_default();
        assert!(
            why.starts_with(&format!("{name}: nextest keeps nothing its tests wrote")),
            "{why}"
        );
        for off in [None, Some(""), Some("0")] {
            assert_eq!(uncaptured(&tests(&[(name, off)])), None, "{name} {off:?}");
        }
    }
    assert_eq!(uncaptured(&tests(&[])), None);
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
