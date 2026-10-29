//! `cargo xtask check`: what a developer runs before a commit, the one full
//! local gate (`npm run check:all` is this). It says each step before it runs
//! it and stops at the first that fails, with that step's exit status as its
//! own.
//!
//! The steps are rustfmt; clippy for the workspace, for the app and for
//! Windows (the workspace's, then the app's); the tests of the workspace, the engine's kit
//! again with the gate's pacing on in every case (`CF_KIT_PACED`), the tests of xtask and of
//! the app; the plants of
//! every family, each checked to still apply to the sources (`cargo xtask
//! plants <family> --check`); then the linter, the build of the native `cf` (the
//! `bin/cf` the page's Playwright suite runs, `npm run test:ui`, which is no part
//! of this gate: it is left as
//! the sources build it) and the Node tests (`npm run lint` and `npm test`: what
//! `npm run check` runs). The app crate is left out of the
//! workspace's lint and tests as the
//! gates leave it out: its build script wants the resources `cargo xtask stage`
//! puts in `app/src-tauri`, which a worktree has not, so it is linted and tested
//! by the steps `cargo xtask app clippy` and `app test` are, with them left out
//! of its configuration. Clippy for Windows is `cargo xtask clippy-windows`,
//! from a machine that is not Windows; on Windows the workspace's own clippy is
//! that, and there is no step of it.
//!
//! No step runs with the variables a window gives its member (`CONSENSFLOW_URL`,
//! `CONSENSFLOW_TOKEN`, `CONSENSFLOW_PARTICIPANT`, `CONSENSFLOW_PROJECT`): the
//! gate is run in member windows, and a test that reads the board from its
//! environment would reach the live board as that member and put its fixture
//! there (2026-10-21: a test question on two tasks' boards, which read
//! "waiting"). Every step is run outside a window, whichever window it is in.
//!
//! xtask's own tests run in a step of their own, in a target folder of their
//! own. The `xtask` this command is running from is `target/debug/xtask`, the
//! one `cargo xtask` built, and cargo puts there whichever build of xtask it
//! made last: a test build of it is not that one (its dev-dependencies change the
//! features of what is under `cf-release`, so the `cf-release` it links is
//! another, and so is the binary: seen on macOS, 1,441,144 bytes against
//! 1,440,936), and so is a test build of the whole workspace. A program that is
//! running was overwritten without a word where this was seen (macOS), and
//! Windows refuses to overwrite one. So the workspace's test step leaves xtask
//! out, and xtask is tested where nothing of it is running. The step that builds
//! the `cf` is `cargo xtask build-cf`, which starts xtask again through cargo: no
//! step before it builds xtask in the shared target folder (clippy only checks,
//! and the tests of xtask have a folder of their own), so cargo finds the one
//! that is running fresh and leaves it as it is.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};
use crate::process::{self, Invocation};
use crate::{app, clippy_windows, plants};

/// Where xtask's own tests are built, from the checkout's root: beside the
/// workspace's build folder, not in it. The plants' runs that test xtask whole
/// name it too (`tests/plants/release`), and a test of theirs holds them to it.
pub(crate) const XTASK_TARGET: &str = "app/src-tauri/target/xtask-check";

/// What a window gives its member to reach the board with. The tests of the
/// gate stand on their own boards and read none from the shell they run in.
const WINDOW_VARIABLES: [&str; 4] = [
    "CONSENSFLOW_URL",
    "CONSENSFLOW_TOKEN",
    "CONSENSFLOW_PARTICIPANT",
    "CONSENSFLOW_PROJECT",
];

pub const COMMANDS: &[Command] = &[Command {
    words: &["check"],
    about: "The full gate before a commit: rustfmt, clippy (workspace, app, Windows), the Rust tests (workspace, xtask, app), the linter and the Node tests, stopping at the first that fails",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("check takes no arguments".into()));
    }
    let windows = context.env.on_windows();
    if windows {
        writeln!(
            console.err,
            "xtask check: no clippy-windows step: on Windows the clippy of the workspace is clippy for Windows"
        )?;
    }
    let families = plants::families(&context.path("tests/plants"))?;
    run_steps(
        &steps(context, &clippy_windows::archiver()?, windows, &families),
        |step| process::run(step, &context.env),
        console.err,
    )
}

/// What `check` runs, in order. `archiver` is the program cc-rs runs as the
/// archiver in the lint for Windows, which `windows` leaves out.
/// The variable that turns the gate's pacing on in every case of the engine's kit
/// (`cf_engine::testing::PACED_VARIABLE`): the paced kit's step sets it.
const PACED_VARIABLE: &str = "CF_KIT_PACED";

fn steps(
    context: &Context,
    archiver: &Path,
    windows: bool,
    families: &[String],
) -> Vec<Invocation> {
    let at = |program: &str, words: &[&str]| {
        Invocation::new(program, &context.root).args(words.iter().copied())
    };
    let mut steps = vec![
        at("cargo", &["fmt", "--all", "--check"]),
        at(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--exclude",
                "app",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        app::clippy(context, &[]),
    ];
    if !windows {
        steps.push(clippy_windows::lint(context, &[], archiver));
        // The app for Windows too, its tests included: CI's Windows checks lint it there, and
        // the workspace's lint above leaves it out (an unused import in a Windows-only test of
        // the app reached main so, 3.0.6-alpha.3's integration B, T-836). Through
        // `cargo xtask`, as `build-cf` is below: the lint of the app wants the stand-in
        // resource compiler that command puts on its PATH.
        steps.push(at("cargo", &["xtask", "clippy-windows", "app"]));
    }
    steps.extend([
        at(
            "cargo",
            &[
                "test",
                "--workspace",
                "--exclude",
                "app",
                "--exclude",
                "xtask",
            ],
        ),
        // The engine's kit again, with the gate's pacing on in every case: what a case that
        // asks for no settle would hide (T-842: the P1 a withheld Enter left).
        at(
            "cargo",
            &["test", "--package", "cf-engine", "--test", "dispatcher"],
        )
        .var(PACED_VARIABLE, "1"),
        at("cargo", &["test", "--package", "xtask", "--target-dir"])
            .arg(context.path(XTASK_TARGET)),
        app::test(context, &[]),
    ]);
    // Each family's plants still apply to the sources: a plant whose text is gone, or whose
    // run is refused, fails the gate by its name (`plants::check`).
    steps.extend(
        families
            .iter()
            .map(|family| at("cargo", &["xtask", "plants", family.as_str(), "--check"])),
    );
    steps.extend([
        at("npm", &["run", "lint"]),
        // The page's Playwright suite runs the built cf (`npm run test:ui`, outside this
        // gate): built again from the sources as they are, as the `check:all` this
        // replaces did first of all.
        at("cargo", &["xtask", "build-cf", "--offline"]),
        at("npm", &["test"]),
    ]);
    steps
}

/// `step` as it is run: outside a window, without the variables
/// [`WINDOW_VARIABLES`] names, after the ones the step sets.
fn outside_a_window(step: &Invocation) -> Invocation {
    WINDOW_VARIABLES
        .into_iter()
        .fold(step.clone(), Invocation::without)
}

/// Runs `steps` with `run`, saying each on `err` first: the status of the
/// first that does not end with 0, else 0. Each is run outside a window.
fn run_steps(
    steps: &[Invocation],
    mut run: impl FnMut(&Invocation) -> Result<i32, process::Failure>,
    err: &mut dyn Write,
) -> Result<i32, Failure> {
    let total = steps.len();
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        writeln!(err, "xtask check [{number}/{total}]: {}", step.display())?;
        let status = run(&outside_a_window(step))?;
        if status != 0 {
            writeln!(
                err,
                "xtask check: step {number} of {total} ended with status {status}: {}",
                step.display()
            )?;
            return Ok(status);
        }
    }
    writeln!(err, "xtask check: all {total} steps passed")?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::PathBuf;

    use cf_base::env::Env;

    /// The families the data of this checkout has.
    fn families() -> Vec<String> {
        ["cli", "daemon", "release"].map(String::from).to_vec()
    }

    fn context() -> Context {
        Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        }
    }

    /// The names of the variables a step sets.
    fn variables(step: &Invocation) -> Vec<String> {
        step.vars
            .iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_steps_are_the_gates_in_order_and_each_runs_where_and_as_its_own_command_does() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), false, &families());
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        let own_target = context.path(XTASK_TARGET).display().to_string();
        assert_eq!(
            lines,
            [
                "cargo fmt --all --check".to_string(),
                "cargo clippy --workspace --exclude app --all-targets -- -D warnings".to_string(),
                "cargo clippy --offline --all-targets -- -D warnings".to_string(),
                "cargo clippy --offline --target x86_64-pc-windows-msvc --all-targets \
                 --workspace --exclude app -- -D warnings"
                    .to_string(),
                "cargo xtask clippy-windows app".to_string(),
                "cargo test --workspace --exclude app --exclude xtask".to_string(),
                "cargo test --package cf-engine --test dispatcher".to_string(),
                format!("cargo test --package xtask --target-dir {own_target}"),
                "cargo test --offline --".to_string(),
                "cargo xtask plants cli --check".to_string(),
                "cargo xtask plants daemon --check".to_string(),
                "cargo xtask plants release --check".to_string(),
                "npm run lint".to_string(),
                "cargo xtask build-cf --offline".to_string(),
                "npm test".to_string(),
            ]
        );
        let root = PathBuf::from("checkout");
        let app: PathBuf = ["checkout", "app", "src-tauri"].iter().collect();
        let cwds: Vec<_> = steps.iter().map(|step| step.cwd.clone()).collect();
        assert_eq!(
            cwds,
            [
                root.clone(),
                root.clone(),
                app.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                app,
                root.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                root.clone(),
                root
            ]
        );
        // The app's two steps leave its resources out; the lint for Windows is told
        // what it is told by its own command; the rest start from xtask's environment.
        assert_eq!(
            steps.iter().map(variables).collect::<Vec<_>>(),
            [
                vec![],
                vec![],
                vec!["TAURI_CONFIG".to_string()],
                vec![
                    "CC_x86_64_pc_windows_msvc".to_string(),
                    "AR_x86_64_pc_windows_msvc".to_string()
                ],
                vec![],
                vec![],
                vec![PACED_VARIABLE.to_string()],
                vec![],
                vec!["TAURI_CONFIG".to_string()],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![]
            ]
        );
    }

    #[test]
    fn the_cf_the_pages_suite_runs_is_built_again_from_the_sources_after_the_lint() {
        let steps = steps(&context(), Path::new("xtask"), false, &families());
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        let [.., lint, build, _] = &lines[..] else {
            panic!("{lines:?}");
        };
        assert_eq!(
            [lint.as_str(), build.as_str()],
            ["npm run lint", "cargo xtask build-cf --offline"]
        );
    }

    #[test]
    fn the_lint_for_windows_is_the_one_its_command_runs_with_xtask_as_its_archiver() {
        let context = context();
        let archiver = Path::new("target").join("debug").join("xtask");
        let steps = steps(&context, &archiver, false, &families());
        assert_eq!(steps[3], clippy_windows::lint(&context, &[], &archiver));
        assert_eq!(steps[2], app::clippy(&context, &[]));
        assert_eq!(steps[4].display(), "cargo xtask clippy-windows app");
        assert_eq!(steps[8], app::test(&context, &[]));
    }

    #[test]
    fn on_windows_the_workspaces_clippy_is_the_one_for_windows_and_there_is_no_step_of_it() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), true, &families());
        let lines: Vec<_> = steps.iter().map(Invocation::display).collect();
        assert_eq!(lines.len(), 13);
        assert!(lines.iter().all(
            |line| !line.contains("x86_64-pc-windows-msvc") && !line.contains("clippy-windows")
        ));
        // Everything else is where it was.
        assert_eq!(
            lines[..3],
            [
                "cargo fmt --all --check",
                "cargo clippy --workspace --exclude app --all-targets -- -D warnings",
                "cargo clippy --offline --all-targets -- -D warnings"
            ]
        );
        assert_eq!(
            lines[6..],
            [
                "cargo test --offline --",
                "cargo xtask plants cli --check",
                "cargo xtask plants daemon --check",
                "cargo xtask plants release --check",
                "npm run lint",
                "cargo xtask build-cf --offline",
                "npm test"
            ]
        );
    }

    /// The kit with the gate's pacing on every case (CF_KIT_PACED, T-842: it found the P1 the
    /// plain run hid) is a step of CI's `checks` job too, as the gate runs it, beside the
    /// plain kit of the `workspace` job (Calliope's re-check of `a0aecd192`, T-854).
    #[test]
    fn the_paced_kit_the_gate_runs_is_a_step_of_cis_checks_job_with_its_variable() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), false, &families());
        let paced: Vec<_> = steps
            .iter()
            .filter(|step| variables(step) == [PACED_VARIABLE])
            .collect();
        assert_eq!(paced.len(), 1, "one step of the gate runs the paced kit");
        let run = paced[0].display();
        assert_eq!(run, "cargo test --package cf-engine --test dispatcher");
        let workflow = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml"),
        )
        .unwrap();
        let checks: String = workflow
            .lines()
            .skip_while(|line| *line != "  checks:")
            .skip(1)
            .take_while(|line| !(line.starts_with("  ") && !line.starts_with("   ")))
            .map(|line| format!("{line}\n"))
            .collect();
        let step = format!(
            "      - run: {run}\n        env:\n          {PACED_VARIABLE}: 1\n        timeout-minutes: "
        );
        assert!(
            checks.contains(&step),
            "ci.yml's checks job runs the paced kit as the gate does:\n{step}"
        );
    }

    #[test]
    fn check_takes_no_arguments() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let refused = run(&context(), &[OsString::from("--fast")], &mut console).unwrap_err();
        assert_eq!(refused.to_string(), "check takes no arguments");
        assert!(matches!(refused, Failure::Usage(_)));
        assert!(err.is_empty());
    }

    fn steps_named(names: &[&str]) -> Vec<Invocation> {
        names
            .iter()
            .map(|name| Invocation::new(*name, "."))
            .collect()
    }

    #[test]
    fn every_step_is_said_before_it_runs_and_all_passing_is_said_last() {
        let steps = steps_named(&["one", "two", "three"]);
        let (mut ran, mut said) = (Vec::new(), Vec::new());
        let status = run_steps(
            &steps,
            |step| {
                ran.push(step.display());
                Ok(0)
            },
            &mut said,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(ran, ["one", "two", "three"]);
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "xtask check [1/3]: one\nxtask check [2/3]: two\nxtask check [3/3]: three\n\
             xtask check: all 3 steps passed\n"
        );
    }

    #[test]
    fn every_step_is_run_outside_a_window_without_the_variables_a_member_is_given() {
        let context = context();
        let steps = steps(&context, Path::new("xtask"), false, &families());
        let mut run = Vec::new();
        let status = run_steps(
            &steps,
            |step| {
                run.push(step.clone());
                Ok(0)
            },
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(run.len(), steps.len());
        let gone = [
            "CONSENSFLOW_URL",
            "CONSENSFLOW_TOKEN",
            "CONSENSFLOW_PARTICIPANT",
            "CONSENSFLOW_PROJECT",
        ]
        .map(|name| (OsString::from(name), None));
        for (step, was) in run.iter().zip(&steps) {
            // What the step sets comes first and is as it was; then the four are taken away.
            assert_eq!(
                step.vars[..was.vars.len()],
                was.vars[..],
                "{}",
                step.display()
            );
            assert_eq!(step.vars[was.vars.len()..], gone, "{}", step.display());
            assert_eq!(
                (&step.program, &step.args, &step.cwd),
                (&was.program, &was.args, &was.cwd)
            );
        }
    }

    #[test]
    fn the_first_step_that_fails_ends_it_with_its_status_and_the_rest_do_not_run() {
        let steps = steps_named(&["one", "two", "three"]);
        let (mut ran, mut said) = (Vec::new(), Vec::new());
        let status = run_steps(
            &steps,
            |step| {
                ran.push(step.display());
                Ok(if step.display() == "two" { 101 } else { 0 })
            },
            &mut said,
        )
        .unwrap();
        assert_eq!(status, 101);
        assert_eq!(ran, ["one", "two"]);
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "xtask check [1/3]: one\nxtask check [2/3]: two\n\
             xtask check: step 2 of 3 ended with status 101: two\n"
        );
    }

    #[test]
    fn a_step_that_cannot_be_started_is_an_error_not_a_status() {
        let steps = steps_named(&["npm"]);
        let mut said = Vec::new();
        let failed = run_steps(
            &steps,
            |step| {
                Err(process::Failure::NotFound {
                    program: step.display(),
                })
            },
            &mut said,
        )
        .unwrap_err();
        assert_eq!(
            failed.to_string(),
            "`npm` was not found: is it installed, and on the PATH?"
        );
        assert_eq!(String::from_utf8(said).unwrap(), "xtask check [1/1]: npm\n");
    }

    #[test]
    fn a_plant_whose_anchor_no_longer_matches_fails_the_gate_step_that_checks_its_family() {
        let folder = tempfile::tempdir().unwrap();
        let context = Context {
            root: folder.path().to_path_buf(),
            env: Env::default(),
        };
        let write = |relative: &str, text: &str| {
            let path = context.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("src/a.rs", "one\ntwo\n");
        write(
            "tests/plants/demo/kit.toml",
            "run_limit_minutes = 1\ntables = [\"all\"]\n",
        );
        write(
            "tests/plants/demo/all.toml",
            "[[plant]]\nname = \"demo: gone\"\nmeant = \"m\"\nruns = [[\"cargo\", \"test\"]]\n\n\
             [[plant.edit]]\nfile = \"src/a.rs\"\nfind = \"three\"\nreplace = \"3\"\n",
        );
        let families = plants::families(&context.path("tests/plants")).unwrap();
        let gate: Vec<Invocation> = steps(&context, Path::new("xtask"), false, &families)
            .into_iter()
            .filter(|step| step.args.get(1).and_then(|word| word.to_str()) == Some("plants"))
            .collect();
        assert_eq!(gate.len(), 1, "the gate checks the plants of demo");

        let (mut said, mut answered) = (Vec::new(), Vec::new());
        let status = run_steps(
            &gate,
            |step| -> Result<i32, process::Failure> {
                let mut err = Vec::new();
                let mut console = Console {
                    out: &mut answered,
                    err: &mut err,
                };
                Ok(plants::run(&context, &step.args[2..], &mut console).unwrap())
            },
            &mut said,
        )
        .unwrap();
        assert_eq!(status, 1);
        assert_eq!(
            String::from_utf8(answered).unwrap(),
            "demo: gone: src/a.rs holds \"three\" 0 times, not once\n0 of 1 plants apply\n"
        );
        assert_eq!(
            String::from_utf8(said).unwrap(),
            "xtask check [1/1]: cargo xtask plants demo --check\n\
             xtask check: step 1 of 1 ended with status 1: cargo xtask plants demo --check\n"
        );
    }
}
