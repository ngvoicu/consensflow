//! A live run beside another run's things (T-1592): the home of a real run is shared. Every
//! run's project folders are below one `~/.consensflow-candidate/live`, and its rig folders and
//! login copies are in one temp folder, so another run's `bench` or `finish-claude` folder and
//! its login copies are in the way of a run that names its own the same. A stand-in rig's home
//! is inside its own folder, where none of that is, so these cases lay a home out as the real
//! one is ([`cf_e2e::live::layout::Layout`]: another run's project folders with files in them, an
//! older rig folder, a kept one, two login copies, one of a run that is gone) and run a case of
//! the live kit on stand-in agents in it, as a program of its own (this file's, run again with
//! `HOME` and the temp folder of the layout).
//!
//! After each, every file of the other run is there with the bytes it had and the time it was
//! last written: a run changes, ends and removes nothing it did not make. A say-only run
//! (`CF_LIVE_AUDIT`) has changed nothing below another run's folders at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::layout::{Layout, OTHER_RUNS_PROJECTS};
use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::members::{self, find, Members, Plan};
use cf_e2e::live::Log;
use cf_e2e::process::{is_alive, own_var, Run};

use crate::{secs, Outcome, FAKE_AGENT};

/// The variable that tells this file's program which case of the layout it is run for.
const CASE: &str = "CF_LAYOUT_CASE";
/// The folder the cases and their programs talk in.
const SIGNALS: &str = "CF_LAYOUT_SIGNALS";
/// What a program says of a folder it made, one a line.
const FOLDER: &str = "LAYOUT-FOLDER ";

/// The program of a case: this test binary, run again for `case` in the layout's home, with
/// `more` set over it.
fn program(
    layout: &Layout,
    case: &str,
    more: &[(&str, &str)],
) -> Result<Run, Box<dyn std::error::Error>> {
    // `cf` and the pane host are built here, as the person's own, and the program is told where:
    // with the layout's home it would find no cargo of the person's to build them with.
    let (cf, host) = (cf_e2e::cf::binary()?, cf_e2e::cf::pane_host()?);
    let built = cf.parent().ok_or("cf has no folder")?;
    assert_eq!(
        Some(built),
        host.parent(),
        "cf and the pane host are built together"
    );
    let mut run = Run::new(std::env::current_exe()?)
        .args([
            "--exact",
            "layout::the_program_a_case_of_the_layout_runs",
            "--nocapture",
            "--test-threads=1",
        ])
        .inheriting_env()
        .without("CF_LIVE_AUDIT")
        .without("CF_LIVE_AUDIT_FILE")
        .without("CF_LIVE_AUDIT_MADE")
        .var(cf_e2e::cf::BUILT, built)
        .var(CASE, case)
        .var(SIGNALS, layout.signals())
        .limit(Duration::from_secs(240));
    for (name, value) in layout.vars().iter() {
        run = run.var(name, value);
    }
    for (name, value) in more {
        run = run.var(name, value);
    }
    Ok(run)
}

/// The folders a program said it made.
fn folders_of(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.split_once(FOLDER))
        .map(|(_, folder)| PathBuf::from(folder.trim_end()))
        .collect()
}

/// Whether `folder` is in the live folder of the layout, and none of another run's.
fn is_a_folder_of_its_own(layout: &Layout, folder: &Path) -> bool {
    let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let folder = real(folder);
    folder.starts_with(real(&layout.live()))
        && OTHER_RUNS_PROJECTS
            .iter()
            .all(|name| !folder.starts_with(real(&layout.live().join(name))))
}

/// A case, run to its end: what it printed, once it passed.
fn passes(layout: &Layout, case: &str, more: &[(&str, &str)]) -> Result<String, String> {
    let ran = program(layout, case, more)
        .map_err(|error| error.to_string())?
        .run()
        .map_err(|error| error.to_string())?;
    if ran.code == Some(0) && !ran.timed_out {
        return Ok(ran.stdout);
    }
    Err(format!("the case {case} did not pass: {ran}"))
}

/// What a case does, in the program that is the case: a project in the live folders of the
/// home it is given, whose chief and member are the stand-in agent, its evidence written in the
/// folder, and the rig closed.
#[test]
fn the_program_a_case_of_the_layout_runs() -> Outcome {
    let Some(case) = own_var(CASE) else {
        return Ok(());
    };
    let signals = PathBuf::from(own_var(SIGNALS).ok_or("a case of the layout has no signals")?);
    let home = own_var("HOME").ok_or("a case of the layout has no home")?;
    let log = Log::new("layout case");
    let name = if case == "whole" {
        "finish-claude"
    } else {
        "bench"
    };
    let plan = Plan::new(name, members::stand_in_chief())
        .with(members::stand_in_worker())
        .with_var("HOME", &home);
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let folder = members.folder().to_path_buf();
    std::fs::write(
        folder.join("evidence.txt"),
        format!("the evidence of the {case} case\n"),
    )?;
    println!("{FOLDER}{}", folder.display());

    if case == "whole" {
        // The folders the cases of the live checks name, each asked for twice: where one case
        // keeps a path across its steps (a Devin conversation resumed, a restart over the same
        // rig, a harness's trust) it is the same folder of this run, and never another run's.
        for fixed in [
            "devin-resume-at-rest/work",
            "own-cf-home",
            "pi-parity",
            "matrix-1",
        ] {
            let first = live_folder_in(Path::new(&home), fixed)?;
            assert_eq!(first, live_folder_in(Path::new(&home), fixed)?, "{fixed}");
            println!("{FOLDER}{}", first.display());
        }
        let tier = members.add("worker", &[])?;
        assert!(!tier.is_empty());
        let number = members.give("worker", "SLEEP 1 Reply with exactly: LAYOUT_OK")?;
        find(
            &log,
            "the member's task to be done",
            secs(60),
            Duration::from_millis(200),
            || Ok((members.task(number)?["state"] == "done").then_some(())),
        )?
        .ok_or("the task was never done")?;
    }
    if case == "hold" {
        // Another invocation of the same case runs while this one is up.
        std::fs::write(
            signals.join("held"),
            format!(
                "{}\n{}\n{}\n",
                folder.display(),
                members.rig().daemon_pid(),
                members.rig().host_pid()
            ),
        )?;
        let released = cf_e2e::live::until(secs(200), Duration::from_millis(100), || {
            signals.join("release").is_file().then_some(())
        });
        assert!(released.is_some(), "the case was never released");
    }
    members.close()?;
    Ok(())
}

/// The layout the cases run in, made.
fn layout() -> Layout {
    Layout::new().expect("the layout is made")
}

#[test]
fn a_start_and_a_whole_case_write_nothing_over_what_another_run_left_in_the_shared_folders(
) -> Outcome {
    let layout = layout();
    for case in ["start", "whole"] {
        let output = passes(&layout, case, &[])?;
        let made = folders_of(&output);
        assert!(!made.is_empty(), "the case {case} made no folder: {output}");
        for folder in &made {
            assert!(
                is_a_folder_of_its_own(&layout, folder),
                "the case {case} used {}, which is in another run's folder or outside the live folder",
                folder.display()
            );
        }
        layout.untouched()?;
    }
    Ok(())
}

#[test]
fn a_second_invocation_of_the_same_case_beside_the_first_has_folders_of_its_own_and_ends_nothing_of_the_first(
) -> Outcome {
    let layout = layout();
    let mut first = program(&layout, "hold", &[])?.spawn()?;
    let held = find(
        &Log::new("layout first"),
        "the first invocation to be up",
        secs(120),
        Duration::from_millis(200),
        || {
            Ok(std::fs::read_to_string(layout.signals().join("held"))
                .ok()
                .filter(|text| text.lines().count() == 3))
        },
    )?
    .ok_or("the first invocation never came up")?;
    let mut lines = held.lines();
    let first_folder = PathBuf::from(lines.next().unwrap_or_default());
    let pids: Vec<u32> = lines.filter_map(|line| line.parse().ok()).collect();
    let evidence = std::fs::read(first_folder.join("evidence.txt"))?;
    let brief = std::fs::read(first_folder.join("CLAUDE.md"))?;

    // The second, whole, beside it.
    let output = passes(&layout, "start", &[])?;
    let second = folders_of(&output);
    assert_eq!(second.len(), 1, "{output}");
    assert_ne!(second[0], first_folder, "both used one project folder");
    assert!(is_a_folder_of_its_own(&layout, &second[0]));
    assert!(first_folder.is_dir(), "the second took the first's folder");
    assert_eq!(std::fs::read(first_folder.join("evidence.txt"))?, evidence);
    assert_eq!(std::fs::read(first_folder.join("CLAUDE.md"))?, brief);
    assert!(
        pids.iter().all(|pid| is_alive(*pid)),
        "the second's end took a process of the first's: {pids:?}"
    );
    layout.untouched()?;

    std::fs::write(layout.signals().join("release"), "go\n")?;
    let ended = first.wait(secs(120))?;
    assert!(
        ended.is_some_and(|status| status.success()),
        "the first did not end well: {ended:?} {}",
        first.errors()
    );
    layout.untouched()?;
    Ok(())
}

#[test]
fn a_say_only_run_changes_nothing_below_another_runs_folders() -> Outcome {
    let layout = layout();
    let audit = layout.signals().join("audit.txt");
    let audit_name = audit.to_string_lossy().into_owned();
    passes(
        &layout,
        "start",
        &[("CF_LIVE_AUDIT", "1"), ("CF_LIVE_AUDIT_FILE", &audit_name)],
    )?;
    layout.unchanged()?;
    Ok(())
}

/// The program `cargo xtask live sweep` runs.
const SWEEP: &str = env!("CARGO_BIN_EXE_live-sweep");

/// `live sweep` in the layout's home, with `words` after it.
fn sweep(
    layout: &Layout,
    words: &[&str],
) -> Result<cf_e2e::process::Ran, Box<dyn std::error::Error>> {
    let mut run = Run::new(SWEEP).args(words).inheriting_env();
    for (name, value) in layout.vars().iter() {
        run = run.var(name, value);
    }
    Ok(run.run()?)
}

/// The lines of `listing` that belong to `folder`: its own line and the indented ones after it.
fn about<'a>(listing: &'a str, folder: &Path) -> String {
    let name = folder.display().to_string();
    let mut lines = listing.lines().skip_while(|line| !line.starts_with(&name));
    let mut found: Vec<&str> = lines.next().into_iter().collect();
    found.extend(lines.take_while(|line| line.starts_with(' ')));
    found.join("\n")
}

#[test]
fn live_sweep_lists_what_earlier_runs_left_and_says_what_each_is_and_takes_nothing() -> Outcome {
    let layout = layout();
    // Another run's folders, old and young, in the temp folder and in the live folder.
    let ran = sweep(&layout, &[])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let listing = ran.stdout;
    let (other, kept) = (
        about(&listing, &layout.other_rig()),
        about(&listing, &layout.kept_rig()),
    );
    assert!(
        other.contains("rig") && other.contains("not marked kept"),
        "{listing}"
    );
    assert!(other.contains("no process"), "{other}");
    assert!(
        kept.contains("marked kept") && !kept.contains("not marked kept"),
        "{kept}"
    );
    assert!(kept.contains("kept for a person to look at"), "{kept}");
    let [gone, there] = layout.login_copies();
    let (gone, there) = (about(&listing, &gone), about(&listing, &there));
    assert!(
        gone.contains("login") && gone.contains("its run (process") && gone.contains(") is gone"),
        "{gone}"
    );
    assert!(there.contains(") is there"), "{there}");
    assert!(
        about(&listing, &layout.harness_home()).contains("login"),
        "{listing}"
    );
    for name in OTHER_RUNS_PROJECTS {
        let project = about(&listing, &layout.live().join(name));
        assert!(project.contains("project"), "{name}: {listing}");
    }
    layout.untouched()?;
    layout.unchanged()?;
    Ok(())
}

#[test]
fn live_sweep_has_no_word_that_takes_a_folder() -> Outcome {
    let layout = layout();
    let ran = sweep(&layout, &["--yes"])?;
    assert_eq!(ran.code, Some(2), "{ran}");
    assert!(ran.stderr.contains("--yes"), "{ran}");
    layout.untouched()?;
    layout.unchanged()?;
    Ok(())
}
