//! What the candidate keeps of the live app's, and what it proves of the live app:
//! the roster it starts with, the modes of what it makes, and that the app, its
//! roster and its processes are as they were when the build is done.

use std::fs;

use super::fake::{bundle, write};
use super::*;

#[test]
fn the_roster_is_the_live_apps_once_and_the_candidates_own_after() {
    let world = World::new();
    let plan = world.plan();
    let roster = plan.paths.state.join("agents.json");
    let live = fs::read_to_string(&plan.paths.live_roster).unwrap();

    // The first run starts the candidate with the agents the live app has saved.
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    assert_eq!(fs::read_to_string(&roster).unwrap(), live);

    // The candidate's own edits are not given back, nor are the live app's later ones.
    write(&roster, "{\"agents\":[\"mine\"]}");
    write(&plan.paths.live_roster, "{\"agents\":[\"the live app's\"]}");
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    assert_eq!(
        fs::read_to_string(&roster).unwrap(),
        "{\"agents\":[\"mine\"]}"
    );
}

#[test]
fn a_run_with_no_live_roster_makes_none_and_a_roster_it_kept_is_not_taken_away() {
    let world = World::new();
    let plan = world.plan();
    fs::remove_file(&plan.paths.live_roster).unwrap();
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    assert!(plan.paths.state.is_dir());
    assert!(!plan.paths.state.join("agents.json").exists());

    write(
        &plan.paths.state.join("agents.json"),
        "{\"agents\":[\"kept\"]}",
    );
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    assert_eq!(
        fs::read_to_string(plan.paths.state.join("agents.json")).unwrap(),
        "{\"agents\":[\"kept\"]}"
    );
}

#[cfg(unix)]
#[test]
fn the_candidates_home_and_its_roster_are_the_users_alone() {
    use std::os::unix::fs::PermissionsExt;

    let world = World::new();
    let plan = world.plan();
    // The live roster is the world's: its mode is whatever it was made with.
    fs::set_permissions(&plan.paths.live_roster, fs::Permissions::from_mode(0o644)).unwrap();
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&plan.paths.state), 0o700);
    assert_eq!(mode(&plan.paths.state.join("agents.json")), 0o600);
}

#[test]
fn a_build_that_touched_the_live_app_is_found_whatever_it_touched() {
    for (what, touch) in [
        (
            "a byte of its program",
            Box::new(|paths: &Paths| {
                write(
                    &paths.live_app.join("Contents").join("MacOS").join("app"),
                    "changed",
                );
            }) as Box<dyn Fn(&Paths)>,
        ),
        (
            "a file more",
            Box::new(|paths: &Paths| write(&paths.live_app.join("Contents").join("new"), "")),
        ),
        (
            "a file less",
            Box::new(|paths: &Paths| {
                fs::remove_file(paths.live_app.join("Contents").join("Info.plist")).unwrap();
            }),
        ),
        (
            "the app gone",
            Box::new(|paths: &Paths| fs::remove_dir_all(&paths.live_app).unwrap()),
        ),
    ] {
        let world = World::new();
        let plan = world.plan();
        let mut system = running_live_app(&world, 4242);
        let paths = plan.paths.clone();
        system.on("npm", move || touch(&paths));
        let said = said(install_on(&world, &mut system));
        assert_eq!(
            said,
            format!(
                "the live app at {} changed during the build — investigate",
                plan.paths.live_app.display()
            ),
            "{what}"
        );
    }
}

#[test]
fn an_app_installed_while_the_build_ran_is_a_change_too() {
    let world = World::new();
    let plan = world.plan();
    fs::remove_dir_all(&plan.paths.live_app).unwrap();
    let mut system = Fake::new(&plan);
    let paths = plan.paths.clone();
    system.on("npm", move || {
        bundle(&paths.live_app, "dev.ngvoicu.consensflow", "1")
    });
    assert!(said(install_on(&world, &mut system)).starts_with("the live app at "));
}

#[test]
fn a_build_that_touched_the_live_roster_is_found_and_said_with_what_may_explain_it() {
    for touch in [
        Box::new(|paths: &Paths| write(&paths.live_roster, "{\"agents\":[1]}"))
            as Box<dyn Fn(&Paths)>,
        Box::new(|paths: &Paths| fs::remove_file(&paths.live_roster).unwrap()),
    ] {
        let world = World::new();
        let plan = world.plan();
        let mut system = running_live_app(&world, 4242);
        let paths = plan.paths.clone();
        system.on("npm", move || touch(&paths));
        assert_eq!(
            said(install_on(&world, &mut system)),
            format!(
                "{} changed during the build — expected only if you edited agents in the live app",
                plan.paths.live_roster.display()
            )
        );
    }
    // A roster made where there was none is a change too.
    let world = World::new();
    let plan = world.plan();
    fs::remove_file(&plan.paths.live_roster).unwrap();
    let mut system = running_live_app(&world, 4242);
    let paths = plan.paths.clone();
    system.on("npm", move || write(&paths.live_roster, "{}"));
    assert!(said(install_on(&world, &mut system)).ends_with("edited agents in the live app"));
}

#[test]
fn a_live_app_that_stopped_running_during_the_build_is_found_by_the_process_that_is_gone() {
    let world = World::new();
    let mut system = running_live_app(&world, 4242);
    system.alive.clear();
    assert_eq!(
        said(install_on(&world, &mut system)),
        "the live app (PID 4242) is no longer running"
    );
    // Only after what was installed was written, which is the order the script had.
    assert!(world.plan().paths.target.is_dir());
}

#[test]
fn the_live_processes_are_those_that_run_the_live_apps_program_whole_and_alone() {
    let world = World::new();
    let plan = world.plan();
    let live = plan.paths.live_program();
    let helpers = program_in(&plan.paths.live_app, "consensflow-bridge");
    let mut system = Fake::new(&plan);
    system.table = vec![
        World::row(11, &live),
        World::row(12, &format!("{live} --flag")),
        World::row(13, &helpers),
        World::row(14, &live),
        World::row(15, "/usr/bin/ssh"),
    ];
    system.alive = vec![11, 14];
    let installed = install_on(&world, &mut system).unwrap();
    assert_eq!(installed.live, [11, 14]);
    assert!(installed
        .said()
        .ends_with("live app and roster unchanged; live PID 11, 14 running\n"));

    // None running is none said.
    let mut system = Fake::new(&plan);
    let installed = install_on(&world, &mut system).unwrap();
    assert_eq!(installed.live, Vec::<u32>::new());
    assert!(installed
        .said()
        .ends_with("live app and roster unchanged\n"));
}
