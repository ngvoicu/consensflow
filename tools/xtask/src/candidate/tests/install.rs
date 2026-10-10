//! What a run builds, refuses, puts in place and writes down.

use std::ffi::OsString;
use std::fs;

use serde_json::Value;

use super::fake::write;
use super::*;

#[test]
fn a_good_run_builds_proves_the_bundle_installs_it_and_writes_down_what_it_was_in_that_order() {
    let world = World::new();
    let plan = world.plan();
    let paths = &plan.paths;
    let mut system = running_live_app(&world, 4242);
    system.porcelain = " M app/src-tauri/src/lib.rs\n?? notes.txt\n".to_owned();

    let installed = install_on(&world, &mut system).unwrap();

    let [build, smoke, copy, seal, head, status] = &system.ran[..] else {
        panic!("{:?}", system.lines());
    };
    assert_eq!((build, smoke), (&plan.build, &plan.smoke));
    // The bundle is copied beside the place it is to be put in, and the seal is of the place.
    assert_eq!(copy.program, OsString::from("/usr/bin/ditto"));
    assert_eq!(
        copy.args,
        [
            paths.built.clone().into_os_string(),
            beside(&paths.target, ".next").into_os_string()
        ]
    );
    assert_eq!(seal.program, OsString::from("/usr/bin/codesign"));
    assert_eq!(
        seal.args,
        [
            OsString::from("--verify"),
            OsString::from("--deep"),
            OsString::from("--strict"),
            paths.target.clone().into_os_string()
        ]
    );
    assert_eq!(head.display(), "git rev-parse HEAD");
    assert_eq!(status.display(), "git status --porcelain");
    // What was installed is what was built.
    assert!(paths
        .target
        .join("Contents")
        .join("MacOS")
        .join("app")
        .is_file());
    assert_eq!(
        plist_value(&paths.target, "CFBundleIdentifier").unwrap(),
        "dev.ngvoicu.consensflow.candidate"
    );
    assert!(!beside(&paths.target, ".next").exists());
    assert!(!beside(&paths.target, ".previous").exists());
    assert_eq!(
        installed,
        Installed {
            target: paths.target.clone(),
            version: "3.0.0-alpha.83".to_owned(),
            state: paths.state.clone(),
            live: vec![4242],
        }
    );
    assert_eq!(
        installed.said(),
        format!(
            "installed {} (3.0.0-alpha.83)\nstate: {}\nlive app and roster unchanged; live PID 4242 running\n",
            paths.target.display(),
            paths.state.display()
        )
    );

    // The record of the build, written as the script wrote it.
    let text = fs::read_to_string(paths.state.join("candidate-build.json")).unwrap();
    assert!(
        text.starts_with("{\n  \"version\": \"3.0.0-alpha.83\",\n  \"app\": "),
        "{text}"
    );
    assert!(text.ends_with("\"\n}\n"), "{text}");
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        record,
        serde_json::json!({
            "version": "3.0.0-alpha.83",
            "app": paths.target.to_string_lossy(),
            "source": paths.repo.to_string_lossy(),
            "head": "0123456789abcdef0123456789abcdef01234567",
            "uncommittedFiles": 2,
            "builtAt": "2026-10-10T09:08:07.654Z",
        })
    );
    // In the order the script wrote it, which is the order a person reads it in.
    let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "version",
            "app",
            "source",
            "head",
            "uncommittedFiles",
            "builtAt"
        ]
    );
}

#[test]
fn git_is_asked_in_the_checkout_and_what_it_counts_as_uncommitted_is_the_lines_it_says() {
    let world = World::new();
    for (porcelain, files) in [("", 0), (" M a\n", 1), (" M a\n?? b\n\n?? c\n", 3)] {
        let mut system = running_live_app(&world, 7);
        system.porcelain = porcelain.to_owned();
        install_on(&world, &mut system).unwrap();
        let text =
            fs::read_to_string(world.plan().paths.state.join("candidate-build.json")).unwrap();
        let record: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(record["uncommittedFiles"], files, "{porcelain:?}");
        let asked: Vec<_> = system
            .ran
            .iter()
            .filter(|run| run.program == "git")
            .collect();
        assert_eq!(asked.len(), 2);
        assert!(asked.iter().all(|run| run.cwd == world.context.root));
    }
}

#[test]
fn a_git_that_does_not_answer_is_said_with_what_it_said() {
    let world = World::new();
    let mut system = running_live_app(&world, 7);
    system.git_status = 128;
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        "git rev-parse HEAD did not answer: fatal: not a git repository"
    );
}

#[test]
fn a_candidate_that_is_running_is_refused_before_anything_is_run_and_nothing_else_is_taken_for_it()
{
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system
        .table
        .push(World::row(9, &program_in(&plan.paths.target, "app")));
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        format!(
            "quit ConsensFlow Candidate first — it is running from {}",
            plan.paths.target.display()
        )
    );
    assert!(system.ran.is_empty());
    assert!(!plan.paths.state.exists());

    // What only looks like it, a copy being put beside it and the live app, is not it.
    let mut system = running_live_app(&world, 4242);
    system.table.extend([
        World::row(10, &program_in(&beside(&plan.paths.target, ".next"), "app")),
        World::row(
            11,
            &format!(
                "{}/Contents/Resources/cli/bin/cf",
                plan.paths.target.display()
            ),
        ),
        World::row(12, &plan.paths.target.to_string_lossy()),
    ]);
    install_on(&world, &mut system).unwrap();
}

#[test]
fn a_build_that_fails_installs_nothing_and_no_smoke_is_run() {
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system.statuses.insert("npm", 3);
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        "`npm run build` ended with status 3: nothing was installed"
    );
    assert_eq!(system.lines(), [plan.build.display()]);
    assert!(!plan.paths.target.exists());
    assert!(!plan.paths.state.exists());
}

#[test]
fn a_bundle_that_kept_the_release_identity_is_refused_before_it_is_smoked_or_installed() {
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system.identity = "dev.ngvoicu.consensflow".to_owned();
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        format!(
            "{} does not carry dev.ngvoicu.consensflow.candidate",
            plan.paths.built.display()
        )
    );
    assert_eq!(system.lines(), [plan.build.display()]);
    assert!(!plan.paths.target.exists());
    assert!(!plan.paths.state.exists());
}

#[test]
fn a_build_that_ends_well_and_leaves_no_bundle_to_read_is_an_error_not_a_pass() {
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system.leaves_a_bundle = false;
    let said = said(install_on(&world, &mut system));
    let plist = plan.paths.built.join("Contents").join("Info.plist");
    assert!(
        said.starts_with(&format!("could not read {}: ", plist.display())),
        "{said}"
    );
    assert_eq!(system.lines(), [plan.build.display()]);
    assert!(!plan.paths.target.exists());
}

#[test]
fn a_candidate_that_fails_the_smoke_is_not_installed_and_the_one_there_stays() {
    let world = World::new();
    let plan = world.plan();
    // An older candidate is installed, with what is the user's in its folder.
    write(
        &plan.paths.target.join("Contents").join("old"),
        "the installed one",
    );
    let mut system = running_live_app(&world, 4242);
    system.statuses.insert("cargo", 101);
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        "the packaged smoke failed; the candidate was not installed"
    );
    assert_eq!(system.lines(), [plan.build.display(), plan.smoke.display()]);
    assert_eq!(
        fs::read_to_string(plan.paths.target.join("Contents").join("old")).unwrap(),
        "the installed one"
    );
    assert!(!beside(&plan.paths.target, ".next").exists());
    assert!(!plan.paths.state.exists());
}

#[test]
fn a_smoke_that_cannot_be_started_is_an_error_that_says_what_is_missing_and_installs_nothing() {
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system.missing.push("cargo");
    let said = said(install_on(&world, &mut system));
    assert_eq!(
        said,
        "`cargo` was not found: is it installed, and on the PATH?"
    );
    assert!(!plan.paths.target.exists());
}

#[test]
fn an_install_replaces_what_is_there_and_clears_what_an_interrupted_one_left() {
    let world = World::new();
    let plan = world.plan();
    let paths = &plan.paths;
    write(
        &paths.target.join("Contents").join("old"),
        "the installed one",
    );
    write(
        &beside(&paths.target, ".next").join("half"),
        "a copy cut short",
    );
    write(
        &beside(&paths.target, ".previous").join("older"),
        "a swap cut short",
    );
    let mut system = running_live_app(&world, 4242);
    install_on(&world, &mut system).unwrap();
    assert!(!paths.target.join("Contents").join("old").exists());
    assert!(!paths.target.join("half").exists());
    assert!(paths.target.join("Contents").join("Info.plist").is_file());
    assert!(!beside(&paths.target, ".next").exists());
    assert!(!beside(&paths.target, ".previous").exists());
}

#[test]
fn a_copy_that_fails_leaves_the_installed_one_where_it_was() {
    let world = World::new();
    let plan = world.plan();
    write(
        &plan.paths.target.join("Contents").join("old"),
        "the installed one",
    );
    let mut system = running_live_app(&world, 4242);
    system.statuses.insert("/usr/bin/ditto", 1);
    let said = said(install_on(&world, &mut system));
    assert!(
        said.starts_with("`/usr/bin/ditto ") && said.ends_with("` ended with status 1"),
        "{said}"
    );
    assert!(plan.paths.target.join("Contents").join("old").is_file());
    assert!(!plan.paths.state.exists());
}

#[test]
fn a_seal_that_does_not_verify_is_an_error_naming_what_was_asked_and_the_status() {
    let world = World::new();
    let plan = world.plan();
    let mut system = running_live_app(&world, 4242);
    system.statuses.insert("/usr/bin/codesign", 1);
    let said = said(install_on(&world, &mut system));
    assert!(
        said.starts_with("`/usr/bin/codesign --verify --deep --strict "),
        "{said}"
    );
    assert!(said.ends_with("` ended with status 1"), "{said}");
    // The script stopped there too: no roster was kept and nothing was written down.
    assert!(!plan.paths.state.join("candidate-build.json").exists());
}
