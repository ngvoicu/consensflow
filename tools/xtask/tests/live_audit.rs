//! A say-only live run (`CF_LIVE_AUDIT`) through `cargo xtask live`, started as a developer starts
//! it, against a stand-in for `cargo` (the fake child): whatever the run came to, it is never a pass,
//! its file is the run's own and new, and an older run's file is never told as this run's (T-1592).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::fs;

use common::{err, out, programs, xtask};

/// The words after `live` of a regular live run of a test the checkout has.
const RUN: [&str; 2] = ["live", "paste"];

#[test]
fn the_fake_cargo_makes_only_the_binaries_a_build_names() {
    let folder = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        common::command(common::FAKE)
            .env("FAKE_CHILD_BUILD_DIR", folder.path())
            .env_remove("FAKE_CHILD_EXIT")
            .args(args)
            .output()
            .unwrap()
    };
    let built = run(&[
        "build",
        "--release",
        "--bin",
        "cf",
        "--bin",
        "cf-shim",
        "--bin=engine-ask",
    ]);
    assert!(built.status.success(), "{}", err(&built));
    let suffix = std::env::consts::EXE_SUFFIX;
    for bin in ["cf", "cf-shim", "engine-ask"] {
        let path = folder.path().join(format!("{bin}{suffix}"));
        assert!(path.is_file(), "fake cargo did not make {}", path.display());
        assert_eq!(fs::read(&path).unwrap(), b"", "{}", path.display());
    }
    let tested = run(&["test", "--bin", "not-a-build"]);
    assert!(tested.status.success(), "{}", err(&tested));
    let mut made: Vec<_> = fs::read_dir(folder.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    made.sort();
    let mut expected = [
        format!("cf{suffix}"),
        format!("cf-shim{suffix}"),
        format!("engine-ask{suffix}"),
    ];
    expected.sort();
    assert_eq!(made, expected);

    let failed = common::command(common::FAKE)
        .env("FAKE_CHILD_BUILD_DIR", folder.path())
        .env("FAKE_CHILD_EXIT", "101")
        .args(["build", "--bin", "failed-build"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(101));
    assert!(!folder.path().join(format!("failed-build{suffix}")).exists());
}

/// xtask already accepts the manifest directory Cargo gives it. Give it a miniature checkout of
/// the test's own, so its real build-folder rule and every fake build stay inside that checkout.
struct Fixture {
    root: tempfile::TempDir,
    tools: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), "").unwrap();
        fs::create_dir_all(root.path().join("tools/xtask")).unwrap();
        let tests = root.path().join("crates/cf-e2e/tests");
        fs::create_dir_all(&tests).unwrap();
        for name in ["paste", "parity", "own_cf", "paste_enter"] {
            fs::write(tests.join(format!("live_{name}.rs")), "").unwrap();
        }
        Self {
            root,
            tools: programs(&["cargo"]),
        }
    }

    fn build_folder(&self) -> std::path::PathBuf {
        self.root
            .path()
            .join("app")
            .join("src-tauri")
            .join("target")
            .join("release")
    }

    fn run(&self, builds: bool, extra: &[(&str, &str)], args: &[&str]) -> std::process::Output {
        let manifest = self.root.path().join("tools/xtask");
        let folder = self.build_folder();
        let mut vars = vec![
            ("CARGO_MANIFEST_DIR", manifest.to_str().unwrap()),
            ("FAKE_CHILD_EXIT", "0"),
            (
                "FAKE_CHILD_BUILD_DIR",
                if builds { folder.to_str().unwrap() } else { "" },
            ),
        ];
        vars.extend_from_slice(extra);
        xtask(self.root.path(), self.tools.path(), &vars, args)
    }

    fn assert_build_products(&self) {
        let suffix = std::env::consts::EXE_SUFFIX;
        let mut expected: Vec<_> = ["cf", "consensflow-bridge", "harness-ask", "engine-ask"]
            .into_iter()
            .map(|name| format!("{name}{suffix}"))
            .collect();
        if cfg!(windows) {
            expected.push(format!("cf-shim{suffix}"));
        }
        expected.sort();
        let mut made: Vec<_> = fs::read_dir(self.build_folder())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        made.sort();
        assert_eq!(made, expected);
        for name in expected {
            assert!(fs::read(self.build_folder().join(name)).unwrap().is_empty());
        }
    }
}

#[test]
fn a_say_only_run_that_made_no_file_is_no_pass_and_is_told_to_have_stopped_before_it_acted() {
    let fixture = Fixture::new();
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("said.txt");
    let ran = fixture.run(
        true,
        &[
            ("CF_LIVE_AUDIT", "1"),
            ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        ],
        &RUN,
    );
    // The stand-in cargo came to 0 and made no file: the run is no pass, and says why.
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    let said = err(&ran);
    assert!(said.contains("made no audit file"), "{said}");
    assert!(said.contains("no pass"), "{said}");
    fixture.assert_build_products();
}

#[test]
fn an_audit_file_that_is_there_stops_the_run_before_cargo_starts_and_is_not_told_as_the_runs() {
    let fixture = Fixture::new();
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("older.txt");
    fs::write(&file, "an older run's audit\nkill\tprocess 1\tground\n").unwrap();
    let ran = fixture.run(
        true,
        &[
            ("CF_LIVE_AUDIT", "1"),
            ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        ],
        &RUN,
    );
    assert_ne!(ran.status.code(), Some(0), "{}", err(&ran));
    let said = err(&ran);
    assert!(said.contains("is there already"), "{said}");
    assert!(!said.contains("would have made"), "{said}");
    // Cargo was never started (the stand-in says where it was started), and the file is as it was.
    assert!(!out(&ran).contains("cwd:"), "{}", out(&ran));
    assert!(!fixture.build_folder().exists());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "an older run's audit\nkill\tprocess 1\tground\n"
    );
}

#[test]
fn a_set_under_audit_is_no_pass_with_a_file_for_each_run_and_stops_where_one_is_there() {
    let fixture = Fixture::new();
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("said.txt");
    let vars = [
        ("CF_LIVE_AUDIT", "1"),
        ("CF_LIVE_AUDIT_FILE", file.to_str().unwrap()),
        (
            "FAKE_CHILD_REPORT",
            "CF_LIVE_AUDIT_FILE,CF_E2E_BUILT,CF_E2E_HELPERS,CF_E2E_NO_BUILD",
        ),
    ];
    let ran = fixture.run(true, &vars, &["live", "--set", "windows"]);
    // Every run of the set came to 0, and the set is no pass all the same.
    assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
    assert!(err(&ran).contains("is no pass"), "{}", err(&ran));
    // The fake cargo also reports the builds before the cases. Count the file given to each
    // case, not the variable inherited by those builds.
    let output = out(&ran);
    let cases: Vec<&str> = output.split("== live ").skip(1).collect();
    assert_eq!(cases.len(), 3, "{output}");
    for (at, case) in cases.iter().enumerate() {
        let given: Vec<&str> = case
            .lines()
            .filter(|line| line.starts_with("var CF_LIVE_AUDIT_FILE:"))
            .collect();
        assert_eq!(given.len(), 1, "{case}");
        assert!(
            given[0].contains(&format!("said-{}.txt", at + 1)),
            "run {} was given {}",
            at + 1,
            given[0]
        );
        let folder = common::shown(fixture.build_folder().to_str().unwrap());
        for name in ["CF_E2E_BUILT", "CF_E2E_HELPERS"] {
            assert!(case.contains(&format!("var {name}: {folder}")), "{case}");
        }
        assert!(case.contains("var CF_E2E_NO_BUILD: \"1\""), "{case}");
    }
    fixture.assert_build_products();

    // A file of an earlier run of the set stops it before any run starts.
    fs::write(folder.path().join("said-2.txt"), "an older run's\n").unwrap();
    let stopped = fixture.run(true, &vars, &["live", "--set", "windows"]);
    assert_ne!(stopped.status.code(), Some(0));
    assert!(
        err(&stopped).contains("is there already"),
        "{}",
        err(&stopped)
    );
    assert!(!out(&stopped).contains("== live"), "{}", out(&stopped));
}

#[test]
fn a_successful_fake_build_that_leaves_a_program_missing_stops_before_the_case() {
    for missing in ["cf", "consensflow-bridge", "harness-ask", "engine-ask"] {
        let fixture = Fixture::new();
        let folder = fixture.build_folder();
        fs::create_dir_all(&folder).unwrap();
        for present in ["cf", "consensflow-bridge", "harness-ask", "engine-ask"] {
            if present != missing {
                fs::write(
                    folder.join(format!("{present}{}", std::env::consts::EXE_SUFFIX)),
                    "",
                )
                .unwrap();
            }
        }
        let audit = fixture.root.path().join("said.txt");
        // Fake cargo succeeds and makes nothing: the three other files and the missing one are
        // all in this test's checkout, never the checkout whose xtask was compiled.
        let ran = fixture.run(
            false,
            &[
                ("CF_LIVE_AUDIT", "1"),
                ("CF_LIVE_AUDIT_FILE", audit.to_str().unwrap()),
            ],
            &RUN,
        );
        assert_eq!(ran.status.code(), Some(1), "{}", err(&ran));
        let wanted = folder.join(format!("{missing}{}", std::env::consts::EXE_SUFFIX));
        assert!(
            err(&ran).contains(&format!(
                "{missing} was built, and there is none at {}",
                wanted.display()
            )),
            "{}",
            err(&ran)
        );
        assert!(!out(&ran).contains("arg: \"test\""), "{}", out(&ran));
        assert!(!audit.exists());
    }
}
