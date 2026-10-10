//! The candidate's steps, on a machine of the test's own in a temporary tree and
//! a system that is a script: nothing here touches the real `/Applications`, the
//! real homes, a real build or a real app. This file is what the tests share and
//! what they say of where a run is and what it is asked to do; `install` is what
//! a run builds, refuses and puts in place, and `proof` what it keeps of the live
//! app's and proves of it.

mod fake;
mod install;
mod proof;

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;
use fake::{Fake, Fixed, World};

/// The time the tests say it is, which the record of a build says back.
const NOW: &str = "2026-10-10T09:08:07.654Z";

/// What a good run of `world` has the system say: the live app is running as `pid`.
fn running_live_app(world: &World, pid: u32) -> Fake {
    let plan = world.plan();
    let mut system = Fake::new(&plan);
    system.table = vec![
        World::row(1, "/sbin/launchd"),
        World::row(pid, &plan.paths.live_program()),
    ];
    system.alive = vec![pid];
    system
}

/// The run of `world` on `system`.
fn install_on(world: &World, system: &mut Fake) -> Result<Installed, Error> {
    let now = cf_base::time::parse(NOW).unwrap();
    install(&world.plan(), system, &mut Fixed(now))
}

fn said(result: Result<Installed, Error>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn a_run_is_where_the_checkout_the_home_and_the_applications_put_it() {
    let world = World::new();
    let plan = world.plan();
    let root = &world.context.root;
    let home = world.temp.path().join("home");
    let paths = &plan.paths;
    assert_eq!(paths.repo, *root);
    assert_eq!(paths.app, root.join("app"));
    assert_eq!(
        paths.built,
        root.join("app")
            .join("src-tauri")
            .join("target")
            .join("release")
            .join("bundle")
            .join("macos")
            .join("ConsensFlow Candidate.app")
    );
    // The candidate goes in the user's own applications, never the system's.
    assert_eq!(
        paths.target,
        home.join("Applications").join("ConsensFlow Candidate.app")
    );
    assert_eq!(paths.state, home.join(".consensflow-candidate"));
    assert_eq!(paths.live_app, world.applications.join("ConsensFlow.app"));
    assert_eq!(
        paths.live_roster,
        home.join(".consensflow").join("agents.json")
    );
    assert!(!paths.target.starts_with(&world.applications));
    assert_eq!(
        paths.live_program(),
        format!(
            "{}/app",
            world
                .applications
                .join("ConsensFlow.app")
                .join("Contents")
                .join("MacOS")
                .display()
        )
    );
}

#[test]
fn the_build_is_the_tauri_build_with_the_candidates_configuration_and_the_smoke_is_the_one_on_the_bundle_it_leaves(
) {
    let world = World::new();
    let plan = world.plan();
    let config = plan
        .paths
        .app
        .join("src-tauri")
        .join("tauri.candidate.conf.json");
    assert_eq!(
        plan.build.display(),
        format!("npm run build -- --config {}", config.display())
    );
    assert_eq!(plan.build.cwd, plan.paths.app);
    assert_eq!(
        plan.smoke,
        smoke::invocation(&world.context, Some(&plan.paths.built))
    );
    assert_eq!(
        plan.smoke.vars,
        [(
            OsString::from("CONSENSFLOW_SMOKE_APP"),
            Some(plan.paths.built.clone().into_os_string())
        )]
    );
}

#[test]
fn a_run_with_no_home_has_nowhere_to_install() {
    let context = Context {
        root: PathBuf::from("checkout"),
        env: cf_base::env::Env::default(),
    };
    let error = plan(&context, Path::new("/Applications")).err().unwrap();
    assert!(matches!(error, Error::NoHome));
    assert_eq!(
        error.to_string(),
        "HOME is not set: the candidate is installed under it and keeps its state in it"
    );
}

#[test]
fn the_processes_of_a_bundle_are_those_started_from_inside_its_programs_folder() {
    let bundle = Path::new("/Users/someone/Applications/ConsensFlow Candidate.app");
    let table = vec![
        World::row(1, &program_in(bundle, "app")),
        World::row(2, &program_in(bundle, "consensflow-bridge --serve")),
        World::row(3, &format!("{}/Contents/MacOS", bundle.display())),
        World::row(4, &format!("{}.next/Contents/MacOS/app", bundle.display())),
        World::row(5, "/Applications/ConsensFlow.app/Contents/MacOS/app"),
    ];
    let found: Vec<u32> = running_from(&table, bundle)
        .iter()
        .map(|row| row.pid)
        .collect();
    assert_eq!(found, [1, 2]);
}

#[cfg(unix)]
#[test]
fn a_program_of_a_bundle_is_the_command_ps_says_of_it() {
    let live = Path::new("/Applications/ConsensFlow.app");
    assert_eq!(
        program_in(live, "app"),
        "/Applications/ConsensFlow.app/Contents/MacOS/app"
    );
    assert_eq!(
        program_in(live, ""),
        "/Applications/ConsensFlow.app/Contents/MacOS/"
    );
}

#[test]
fn a_file_is_put_beside_a_path_by_a_suffix_on_its_name() {
    assert_eq!(
        beside(Path::new("/a/b/ConsensFlow Candidate.app"), ".next"),
        PathBuf::from("/a/b/ConsensFlow Candidate.app.next")
    );
}

#[test]
fn the_command_takes_no_arguments_and_runs_in_rust() {
    assert!(matches!(COMMANDS[0].run, Run::Native(_)));
    assert_eq!(COMMANDS[0].words, ["candidate"]);
    let context = World::new().context;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let refused = run(&context, &[OsString::from("--fast")], &mut console).unwrap_err();
    assert_eq!(refused.to_string(), "candidate takes no arguments");
    assert!(matches!(refused, Failure::Usage(_)));
    assert!(out.is_empty() && err.is_empty());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn where_there_is_no_mac_the_command_says_so_and_runs_nothing() {
    let context = World::new().context;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    assert_eq!(run(&context, &[], &mut console).unwrap(), 1);
    assert!(out.is_empty());
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "candidate: the candidate build is macOS-only for now\n"
    );
}
