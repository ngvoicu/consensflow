//! The commands that end every program by its name (`pkill`, `killall`) are refused in a member's
//! window (T-2213): the folder of ConsensFlow's stand-ins is first on the PATH of every window the
//! daemon opens, whatever its harness. The stand-ins of Claude Code, Codex and Devin are windows
//! of the real daemon and pane host here, and so is Muse Code's, on Muse's own echo model where
//! the machine has Muse (the case is skipped, in plain words, where it has none: CI's). Pi has
//! no stand-in in the rig, so no Pi window can be opened here; its window's environment is the
//! daemon's alike (`WindowEnv::env` builds every harness's from the same strings, held by
//! `cf-daemon`'s `member_guard` unit tests, `every_window_has_the_guards_folder_first_on_its_path_before_the_bundle`).
//! The windows run scripts, so this is Unix's; `cf/tests/member_guard.rs` holds Windows's.
//!
//! No case here runs a command by its name (the kit refuses to): it finds the program the window's
//! PATH names first and shows it is the `cf` of the bundle that comes next on it, which the other cases run under those names.

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use cf_e2e::process::first_on_path;
#[cfg(unix)]
use cf_e2e::rig::{Project, Rig, OPEN};
#[cfg(unix)]
use serde_json::{json, Value};

#[cfg(unix)]
use crate::{config, secs, Outcome};

/// The stand-ins for Codex and Devin, built for these tests.
#[cfg(unix)]
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");
#[cfg(unix)]
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");
/// The rig's Muse on Windows runs the machine's; here it is the program that stands for it.
#[cfg(unix)]
const MUSE_STAND_IN: &str = env!("CARGO_BIN_EXE_muse-stand-in");

#[cfg(unix)]
/// The program `name` first on the PATH of the window opened as `frame`.
fn found_first(frame: &Value, name: &str) -> Option<PathBuf> {
    first_on_path(name, frame["env"]["PATH"].as_str()?, "")
}

#[cfg(unix)]
#[test]
fn every_harnesss_window_finds_the_guards_pkill_and_killall_before_any_other_and_they_refuse(
) -> Outcome {
    let rig = Rig::start(config().codex(FAKE_CODEX).devin(FAKE_DEVIN))?;
    rig.stand_ins(&json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "claude-worker", "kind": "claude-code", "model": "fake", "workTier": "light" },
            { "id": "codex-worker", "kind": "codex", "model": "gpt-6-astra", "effort": "medium", "workTier": "standard" },
            { "id": "devin-worker", "kind": "devin", "model": "fake", "workTier": "complex" },
        ],
    }))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    for agent in ["claude-worker", "codex-worker", "devin-worker"] {
        project.add_member(agent)?;
    }
    rig.tell(
        id,
        "DISPATCH --tier light Reply with exactly: CLAUDE_DONE\n\
         DISPATCH --tier standard Reply with exactly: CODEX_DONE\n\
         DISPATCH --tier complex Reply with exactly: DEVIN_DONE",
    )?;
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    rig.wait_for("the chief and three members' windows", secs(120), || {
        Ok(rig.open_frames().len() >= 4)
    })?;
    let frames = rig.open_frames();
    assert!(frames.iter().any(|frame| frame["id"] == chief["id"]));
    every_window_finds_the_guard_first(&rig, &frames)?;
    rig.close()?;
    Ok(())
}

/// Each of `frames` finds the guard's folder first for both names, the bundle's `cf` as what
/// they are, and (on a Mac) the guard's root as its `PATH_HELPER_ROOT`.
#[cfg(unix)]
fn every_window_finds_the_guard_first(rig: &Rig, frames: &[Value]) -> Outcome {
    let guard = rig.home().join("member-guard");
    for frame in frames {
        // A Mac's login shells keep the folder first through the root their `path_helper`
        // reads; nothing else of the person's shell is touched.
        if cfg!(target_os = "macos") {
            let root = rig.home().join("member-guard-root");
            assert_eq!(
                frame["env"]["PATH_HELPER_ROOT"].as_str(),
                Some(root.to_string_lossy().as_ref()),
                "{}",
                frame["id"]
            );
            let paths = std::fs::read_to_string(root.join("etc/paths"))?;
            assert_eq!(paths.lines().next(), Some(guard.to_string_lossy().as_ref()));
        }
        let env = frame["env"].as_object().ok_or("no env")?;
        assert!(
            env.keys()
                .all(|name| name != "ZDOTDIR" && !name.starts_with("BASH_FUNC_")),
            "{}",
            frame["id"]
        );
        for name in ["pkill", "killall"] {
            let found = found_first(frame, name)
                .ok_or_else(|| format!("{}: no {name} on its PATH", frame["id"]))?;
            assert_eq!(
                found.parent(),
                Some(guard.as_path()),
                "{}: {name} is {}",
                frame["id"],
                found.display()
            );
            // The stand-in is the bundle's `cf`, which refuses (`cf/tests/member_guard.rs`). The
            // kit never starts a program by these names, so it is not run here.
            let bundle = std::env::split_paths(frame["env"]["PATH"].as_str().unwrap_or_default())
                .nth(1)
                .ok_or("no second folder on the PATH")?
                .join("cf");
            assert_eq!(
                std::fs::canonicalize(&found)?,
                std::fs::canonicalize(&bundle)?,
                "{}: {name}",
                frame["id"]
            );
        }
    }
    Ok(())
}

/// A Muse Code window, on Muse's echo model, has the same: the guard's root and the guard's
/// folder first for both names.
#[cfg(unix)]
#[test]
fn a_muse_code_window_finds_the_guards_pkill_and_killall_before_any_other_and_the_helper_root(
) -> Outcome {
    if cf_e2e::muse::runner_has_no_muse() {
        eprintln!("SKIPPED: no Muse Code on this machine");
        return Ok(());
    }
    let install = cf_e2e::muse::echo_required()?;
    let rig = Rig::start(config().muse(install.through(MUSE_STAND_IN), 1_000))?;
    let project = cf_e2e::muse::members::project(&rig, &cf_e2e::muse::members::echo_worker())?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: MUSE_DONE",
    )?;
    rig.wait_for(
        "the chief's and the Muse member's windows",
        secs(120),
        || Ok(rig.open_frames().len() >= 2),
    )?;
    every_window_finds_the_guard_first(&rig, &rig.open_frames())?;
    rig.close()?;
    Ok(())
}
