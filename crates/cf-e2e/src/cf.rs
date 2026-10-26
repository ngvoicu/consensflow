//! The programs under test: the `cf` and the pane host (`consensflow-bridge`),
//! the release builds of this checkout, each made once for a run of the tests;
//! and the test binaries `harness-ask` and `engine-ask`, which the live tests ask
//! what they need of the harness code and of the engine's texts (this crate
//! depends on no crate of ours: each is a program, started as the others are). They are built, not found: `bin/cf`
//! is whatever the last `cargo xtask build-cf` left, and a suite that ran that
//! could pass against sources it never saw (a change in hand not built yet, a
//! plant). `cargo build` knows when the build is up to date, and then it takes
//! a moment.
//!
//! A run that built `cf` and the pane host before it, from the sources as they
//! are, says where (`BUILT`), and they are taken from there and not built: under
//! nextest each test is a process of its own, which would build them for itself,
//! and cargo links a binary again as it builds, taking it from under a test of
//! another process that is starting it (the plants' runner, 2026-10-16).
//!
//! A run that did not (`cargo nextest run -p cf-e2e`) is told apart by the id
//! nextest gives it (`RUN`), the same in each of its processes, and builds each
//! program once: the first process to ask builds it, under a lock the others wait
//! on, and writes down which run it built it for; each later process of the run
//! finds that and runs no cargo. Cargo makes the program's file again whenever it
//! is run, even where it finds nothing to build (a new file each time, and for a
//! moment none: 8 loops of builds with nothing to do, watched from beside, left
//! `release/cf` missing 2,553 times in 20 s, 2026-10-25, T-544). A process that
//! had been told where `cf` was found none there ("could not start …/release/cf:
//! No such file or directory": 5 runs of 12 of the `cli` suite, every case that
//! failed in them), and one that looked just after its own cargo ended found the
//! same ("cargo built cf, but there is none").

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::process::{own_var, Run};
use crate::{checkout, files, Error, Result};

/// The variable that names the folder `cf` and the pane host were built in for
/// the run, before it, from the sources as they are.
pub const BUILT: &str = "CF_E2E_BUILT";

/// The variable that names the run: nextest gives each run an id of its own, the
/// same to every test's process of it. A run that is not nextest's has none.
pub const RUN: &str = "NEXTEST_RUN_ID";

/// The folder the workspace builds in, from the checkout's root:
/// `.cargo/config.toml` puts the build folder at `app/src-tauri/target`.
const TARGET: [&str; 3] = ["app", "src-tauri", "target"];

/// The folder in the build folder where the builds of a run are kept in order:
/// the lock to build under, and for each program the run it was built for.
const BUILDS: &str = "cf-e2e";

/// What a build leaves, or why it could not be made.
type Built = OnceLock<std::result::Result<PathBuf, String>>;

/// The `cf` under test: built, or found up to date, when this is first asked
/// for, and the same one for every case of the run; or the one built before the
/// run (`BUILT`).
pub fn binary() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf", "cf", &[], given())
}

/// The pane host under test, `consensflow-bridge`: the program that gives each
/// window its terminal, which the app starts beside the daemon. Built as `cf`
/// is, once for a run, or the one built before it.
pub fn pane_host() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(&BUILT, "cf-panes", "consensflow-bridge", &[], given())
}

/// The test binary `harness-ask`, which answers what a live test asks of the
/// harness code of the product: the harness crate's own, built with the feature
/// that makes it (it ships with nothing). Built as `cf` is, once for a run, and
/// the first time a test asks; never before the run.
pub fn harness_ask() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(
        &BUILT,
        "cf-harness",
        "harness-ask",
        &["--features", "test-support"],
        None,
    )
}

/// The test binary `engine-ask`, which answers what a live test asks of the
/// engine's texts (the note a window the human's Resume brings back is given): the
/// engine crate's own, built as `harness-ask` is, once for a run and never before it.
pub fn engine_ask() -> Result<&'static Path> {
    static BUILT: Built = OnceLock::new();
    built(
        &BUILT,
        "cf-engine",
        "engine-ask",
        &["--features", "test-support"],
        None,
    )
}

/// The folder the run built `cf` and the pane host in before it, where it says one.
fn given() -> Option<PathBuf> {
    own_var(BUILT)
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// The build of the binary `bin` of the package `package`, with the cargo words
/// `more` after the rest, made the first time it is asked for and kept in `cell`;
/// or, where the run built it before in the folder `before`, that one.
fn built(
    cell: &'static Built,
    package: &str,
    bin: &str,
    more: &[&str],
    before: Option<PathBuf>,
) -> Result<&'static Path> {
    built_by(
        cell,
        Run::new(env!("CARGO")),
        &checkout::root(),
        (package, bin, more),
        before,
    )
}

/// [`built`] with the cargo that builds and the checkout it builds in given, and
/// the program as its package, its name and the cargo words after the rest.
fn built_by(
    cell: &'static Built,
    cargo: Run,
    root: &Path,
    (package, bin, more): (&str, &str, &[&str]),
    before: Option<PathBuf>,
) -> Result<&'static Path> {
    cell.get_or_init(|| match before {
        Some(folder) => taken(&folder, bin),
        None => made(cargo, root, package, bin, more, own_var(RUN).as_deref()),
    })
    .as_deref()
    .map_err(|message| Error::Build(message.clone()))
}

/// The binary `bin` the run built in `folder` before it: an error where it is not
/// there, which no build is made for.
fn taken(folder: &Path, bin: &str) -> std::result::Result<PathBuf, String> {
    let built = folder.join(exe(bin));
    if built.is_file() {
        Ok(built)
    } else {
        Err(format!(
            "{BUILT} says {bin} was built in {}, and there is none there",
            folder.display()
        ))
    }
}

/// The binary `bin` of the package `package` in the checkout at `root`, built for
/// the run `run` once, by whichever process of it asks first, and taken from there
/// by the rest; a process that is no run's builds its own. The processes of one
/// machine take turns on a lock in the build folder, so a process that arrives
/// while another builds waits for it, and finds the program made.
fn made(
    cargo: Run,
    root: &Path,
    package: &str,
    bin: &str,
    more: &[&str],
    run: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    let folder = target(root).join(BUILDS);
    files::make_dir(&folder).map_err(|cause| cause.to_string())?;
    let lock = folder.join("build.lock");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .and_then(|file| file.lock().map(|()| file))
        .map_err(|cause| format!("could not take the lock {}: {cause}", lock.display()))?;
    let built_for = folder.join(format!("{bin}.run"));
    let built = release(root).join(exe(bin));
    let made = run.is_some_and(|run| {
        built.is_file() && files::read_string(&built_for).is_ok_and(|said| said == run)
    });
    if !made {
        build(cargo, root, package, bin, more)?;
        if let Some(run) = run {
            files::write(&built_for, run).map_err(|cause| cause.to_string())?;
        }
    }
    drop(lock);
    Ok(built)
}

/// The folder the workspace builds in, from the checkout's root.
fn target(root: &Path) -> PathBuf {
    TARGET
        .iter()
        .fold(root.to_path_buf(), |folder, part| folder.join(part))
}

/// The folder the workspace builds a release binary in, from the checkout's root.
fn release(root: &Path) -> PathBuf {
    target(root).join("release")
}

/// The file name of the program `bin` on this platform.
fn exe(bin: &str) -> String {
    format!("{bin}{}", std::env::consts::EXE_SUFFIX)
}

/// Builds the binary `bin` of the package `package` in the checkout at `root`
/// with `cargo` (the program that builds, given its first words if it has any),
/// and the cargo words `more`, and answers where it is.
fn build(
    cargo: Run,
    root: &Path,
    package: &str,
    bin: &str,
    more: &[&str],
) -> std::result::Result<PathBuf, String> {
    let ran = cargo
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            package,
            "--bin",
            bin,
        ])
        .args(more)
        .cwd(root)
        .inheriting_env()
        .unlimited()
        .run()
        .map_err(|cause| cause.to_string())?;
    if ran.code != Some(0) {
        return Err(format!("building {bin} did not succeed: {ran}"));
    }
    let built = release(root).join(exe(bin));
    if !built.is_file() {
        return Err(format!(
            "cargo built {bin}, but there is none at {}: is the build folder somewhere \
             else than .cargo/config.toml says?",
            built.display()
        ));
    }
    Ok(built)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    use crate::files;

    /// A shell script, in a folder of its own.
    fn script(text: &str) -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("cargo.sh");
        files::write(&file, text).unwrap();
        (folder, file)
    }

    /// A `cargo` that is a shell script: run by the shell, since a script that
    /// has just been written and is started while another thread starts a
    /// program of its own may be refused as a file still open for writing.
    fn cargo(text: &str) -> (tempfile::TempDir, Run) {
        let (folder, file) = script(text);
        (folder, Run::new("/bin/sh").arg(file))
    }

    #[test]
    fn the_build_is_asked_for_in_the_checkout_and_the_cf_it_leaves_is_the_answer() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "pwd > asked-in; echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/cf",
        );
        let built = build(cargo, root.path(), "cf", "cf", &[]).unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!("cf{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf --bin cf\n"
        );
        let asked_in = files::read_string(&root.path().join("asked-in")).unwrap();
        assert_eq!(
            asked_in.trim_end(),
            std::fs::canonicalize(root.path())
                .unwrap()
                .to_string_lossy()
        );
    }

    #[test]
    fn the_pane_host_is_asked_for_as_a_binary_of_its_own_package() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/consensflow-bridge",
        );
        let built = build(cargo, root.path(), "cf-panes", "consensflow-bridge", &[]).unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!(
                    "consensflow-bridge{}",
                    std::env::consts::EXE_SUFFIX
                ))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf-panes --bin consensflow-bridge\n"
        );
    }

    #[test]
    fn a_build_that_fails_says_what_cargo_said() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo("echo 'error: no such package' >&2; exit 101");
        let failed = build(cargo, root.path(), "cf", "cf", &[]).unwrap_err();
        assert!(
            failed.starts_with("building cf did not succeed: exit code 101\n"),
            "{failed}"
        );
        assert!(failed.contains("error: no such package"), "{failed}");
    }

    #[test]
    fn a_build_that_leaves_no_binary_where_it_should_says_where() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo("exit 0");
        let failed = build(cargo, root.path(), "cf", "cf", &[]).unwrap_err();
        assert!(
            failed.starts_with("cargo built cf, but there is none at "),
            "{failed}"
        );
        assert!(failed.contains("target"), "{failed}");
    }

    #[test]
    fn a_cargo_that_cannot_be_started_is_a_failed_build() {
        let root = tempfile::tempdir().unwrap();
        let failed = build(
            Run::new(root.path().join("no-cargo")),
            root.path(),
            "cf",
            "cf",
            &[],
        )
        .unwrap_err();
        assert!(failed.starts_with("could not start `"), "{failed}");
    }

    /// Run by the test below in a process of its own, given `BUILT`: says the `cf` and
    /// the pane host under test.
    #[test]
    #[ignore = "a program the tests of cf start"]
    fn helper_says_the_programs_under_test() {
        println!("cf: {}", binary().unwrap().display());
        println!("pane host: {}", pane_host().unwrap().display());
    }

    #[test]
    fn the_programs_built_before_the_run_are_those_under_test_and_none_is_built() {
        let folder = tempfile::tempdir().unwrap();
        let named = |bin: &str| {
            folder
                .path()
                .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX))
        };
        files::write(&named("cf"), "").unwrap();
        files::write(&named("consensflow-bridge"), "").unwrap();
        // A build would fail at once: no compiler is there.
        let ran = Run::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cf::tests::helper_says_the_programs_under_test",
                "--ignored",
                "--nocapture",
            ])
            .var(BUILT, folder.path())
            .var("RUSTC", folder.path().join("no-rustc"))
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        assert!(
            ran.stdout
                .contains(&format!("cf: {}\n", named("cf").display())),
            "{}",
            ran.output()
        );
        assert!(
            ran.stdout.contains(&format!(
                "pane host: {}\n",
                named("consensflow-bridge").display()
            )),
            "{}",
            ran.output()
        );
    }

    #[test]
    fn a_program_the_run_says_it_built_and_did_not_is_an_error_and_is_not_built() {
        let folder = tempfile::tempdir().unwrap();
        let failed = taken(folder.path(), "cf").unwrap_err();
        assert_eq!(
            failed,
            format!(
                "CF_E2E_BUILT says cf was built in {}, and there is none there",
                folder.path().display()
            )
        );
        static CELL: Built = OnceLock::new();
        let failed = built(&CELL, "cf", "cf", &[], Some(folder.path().to_path_buf()))
            .unwrap_err()
            .to_string();
        assert!(failed.contains("and there is none there"), "{failed}");
    }

    #[test]
    fn the_test_binary_the_live_tests_ask_is_built_with_the_feature_that_makes_it() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = cargo(
            "echo \"$@\" > asked; \
             mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/harness-ask",
        );
        let built = build(
            cargo,
            root.path(),
            "cf-harness",
            "harness-ask",
            &["--features", "test-support"],
        )
        .unwrap();
        assert_eq!(
            built,
            root.path()
                .join("app/src-tauri/target/release")
                .join(format!("harness-ask{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            files::read_string(&root.path().join("asked")).unwrap(),
            "build --release --locked -p cf-harness --bin harness-ask --features test-support\n"
        );
    }

    /// The cargo of the cases below, as cargo is: one build at a time in a build
    /// folder (its lock, `.cargo-lock`, is let go as it ends), and a program that
    /// is linked again even where there was nothing to build: removed, and made
    /// again a moment later. It writes down each time it is run.
    const REPLACING: &str = "until mkdir .cargo-lock 2>/dev/null; do sleep 0.01; done; \
         echo built >> cargo-calls; \
         rm -f app/src-tauri/target/release/cf; \
         sleep 0.2; \
         mkdir -p app/src-tauri/target/release && : > app/src-tauri/target/release/cf; \
         rmdir .cargo-lock";

    /// The variables that give `helper_asks_for_the_cf_of_a_run` the checkout and
    /// the cargo it asks in.
    const ROOT: &str = "CF_E2E_TEST_ROOT";
    const CARGO: &str = "CF_E2E_TEST_CARGO";

    /// Run by the cases below in processes of their own, as the tests of a nextest
    /// run are, given the checkout in `ROOT`, the script of the cargo in `CARGO`
    /// and the run in `RUN`: asks for the `cf` of the run, says where it is, and
    /// says whether it is still there a moment later.
    #[test]
    #[ignore = "a program the tests of cf start"]
    fn helper_asks_for_the_cf_of_a_run() {
        static CELL: Built = OnceLock::new();
        let root = PathBuf::from(own_var(ROOT).unwrap());
        let cargo = Run::new("/bin/sh").arg(own_var(CARGO).unwrap());
        match built_by(&CELL, cargo, &root, ("cf", "cf", &[]), None) {
            Ok(path) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                println!("cf: {}\nthere: {}", path.display(), path.is_file());
            }
            Err(failed) => println!("failed: {failed}"),
        }
    }

    /// What `count` processes that ask at once for the `cf` of the run `run` (the
    /// run of no one, where none is given) in `root` say, each with the cargo
    /// `script`.
    fn ask_for_the_cf(root: &Path, script: &Path, run: Option<&str>, count: usize) -> Vec<String> {
        std::thread::scope(|scope| {
            let asking: Vec<_> = (0..count)
                .map(|_| {
                    scope.spawn(|| {
                        let mut process = Run::new(std::env::current_exe().unwrap())
                            .args([
                                "--exact",
                                "cf::tests::helper_asks_for_the_cf_of_a_run",
                                "--ignored",
                                "--nocapture",
                            ])
                            .var("PATH", own_var("PATH").unwrap_or_default())
                            .var(ROOT, root)
                            .var(CARGO, script);
                        if let Some(run) = run {
                            process = process.var(RUN, run);
                        }
                        let ran = process.run().unwrap();
                        assert_eq!(ran.code, Some(0), "{}", ran.output());
                        // The test harness says its own words too.
                        ran.stdout
                            .lines()
                            .filter(|line| {
                                ["cf: ", "there: ", "failed: "]
                                    .iter()
                                    .any(|said| line.starts_with(said))
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                })
                .collect();
            asking
                .into_iter()
                .map(|said| said.join().unwrap())
                .collect()
        })
    }

    fn calls(root: &Path) -> usize {
        files::read_string(&root.join("cargo-calls"))
            .map(|calls| calls.lines().count())
            .unwrap_or(0)
    }

    /// T-544: under `cargo nextest run -p cf-e2e` each test is a process, and each
    /// ran its own cargo against the one build folder. Cargo takes `release/cf` from
    /// under a program that has just been told where it is, and that was a case that
    /// "could not start …/release/cf: No such file or directory" (2 of 12 runs).
    #[test]
    fn the_processes_of_a_run_build_a_program_once_and_none_has_it_taken_from_under_it() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = script(REPLACING);
        let said = ask_for_the_cf(root.path(), &cargo, Some("run-a"), 8);
        let there = release(root.path()).join(exe("cf"));
        for said in &said {
            assert_eq!(
                *said,
                format!("cf: {}\nthere: true", there.display()),
                "{said}"
            );
        }
        assert_eq!(calls(root.path()), 1, "cargo was run once for the run");
    }

    #[test]
    fn a_program_built_for_another_run_is_built_again_and_one_built_for_this_one_is_not() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = script(REPLACING);
        ask_for_the_cf(root.path(), &cargo, Some("run-a"), 2);
        assert_eq!(calls(root.path()), 1);
        ask_for_the_cf(root.path(), &cargo, Some("run-b"), 2);
        assert_eq!(calls(root.path()), 2, "the second run's own build");
        ask_for_the_cf(root.path(), &cargo, Some("run-b"), 2);
        assert_eq!(calls(root.path()), 2, "nothing again for the same run");
    }

    #[test]
    fn a_program_the_run_built_and_that_is_gone_is_built_again() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = script(REPLACING);
        ask_for_the_cf(root.path(), &cargo, Some("run-a"), 1);
        std::fs::remove_file(release(root.path()).join(exe("cf"))).unwrap();
        ask_for_the_cf(root.path(), &cargo, Some("run-a"), 1);
        assert_eq!(calls(root.path()), 2);
    }

    #[test]
    fn processes_that_belong_to_no_run_each_build_their_own_and_write_down_nothing() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, cargo) = script(REPLACING);
        let said = ask_for_the_cf(root.path(), &cargo, None, 3);
        for said in &said {
            assert!(said.contains("there: true"), "{said}");
        }
        assert_eq!(calls(root.path()), 3);
        assert!(!target(root.path()).join(BUILDS).join("cf.run").exists());
    }

    #[test]
    fn a_build_that_did_not_succeed_is_not_written_down_as_made_for_the_run() {
        let root = tempfile::tempdir().unwrap();
        let (_folder, failing) = script("echo built >> cargo-calls; exit 101");
        let said = ask_for_the_cf(root.path(), &failing, Some("run-a"), 1);
        assert!(
            said[0].starts_with("failed: building cf did not succeed"),
            "{}",
            said[0]
        );
        assert!(!target(root.path()).join(BUILDS).join("cf.run").exists());
        let (_folder, cargo) = script(REPLACING);
        let said = ask_for_the_cf(root.path(), &cargo, Some("run-a"), 1);
        assert!(said[0].contains("there: true"), "{}", said[0]);
        assert_eq!(calls(root.path()), 2, "the next process builds it again");
    }
}
