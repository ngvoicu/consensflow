//! A Muse chief's role, as the daemon gives it to the machine's own Muse (on Muse's built-in
//! fake model, `--provider echo`: the real program and its hooks, no model, no login, no quota).
//!
//! A Muse window has its role from its SessionStart hook alone, and Muse 1.4.14 takes at most
//! 16,384 bytes of a hook's output: past that the hook `failed` with `output_too_large` and
//! nothing of what it printed was kept (T-1246: every chief of alpha.2, whose text is 17 to 29
//! thousand bytes, started with no role). So the role goes in parts, a hook for each, and the
//! session's log says what each hook did and what context it gave: the real program is the
//! judge here, not a stand-in that knows the limit.
//!
//! Each case is skipped, in plain words, on a machine with no Muse Code installed: CI's.

use std::fs;
use std::path::PathBuf;

use cf_e2e::muse::{self, hook_blocks, hook_runs};
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How long an echo turn takes: short, nothing here waits on one.
const TURN_MS: u64 = 500;

/// The bytes Muse 1.4.14 takes of a SessionStart hook's output, measured on 2026-11-05
/// (`output_too_large` at 16,385, none at 16,384; plain words and JSON alike).
const MUSE_HOOK_OUTPUT_LIMIT: usize = 16_384;

/// The members of the staff of the largest project: 25, each with two roles.
const STAFF: usize = 25;

/// A rig whose Muse windows run the machine's Muse, or why there is none.
fn muse_rig() -> Option<Config> {
    match muse::installed() {
        Ok(install) => {
            Some(config().muse(install.through(env!("CARGO_BIN_EXE_muse-stand-in")), TURN_MS))
        }
        Err(why) => {
            #[allow(clippy::print_stderr)] // Said in the output of the case it skips.
            {
                eprintln!("skipped: {why}");
            }
            None
        }
    }
}

/// The most rules for the staff the ledger takes: 8,000 bytes in 150 lines, the lines in
/// characters of three bytes where they fit (a script outside the Latin ones), so the text
/// that says them is as long as any can make it.
fn the_most_rules() -> String {
    let lines = 150;
    let each = (8_000 - (lines - 1)) / lines;
    let line = format!("{}{}", "日".repeat(each / 3), "a".repeat(each % 3));
    let mut rules = vec![line; lines].join("\n");
    rules += &"a".repeat(8_000 - rules.len());
    assert_eq!((rules.len(), rules.split('\n').count()), (8_000, 150));
    rules
}

/// The project of the largest chief a Muse window is given: a staff of [`STAFF`] who each hold
/// two roles, its specs on, and the most rules for the staff, the chief's window opened after
/// them so that its text has them all. The roster names the chief on Muse and its staff on the
/// stand-in `claude`.
fn largest_chiefs_project(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    let mut agents = vec![json!({
        "id": "chief", "kind": "muse", "model": "muse-spark-1.3", "effort": "minimal"
    })];
    let mut staff = Vec::new();
    for at in 0..STAFF {
        let name = format!("member-{at:02}");
        agents.push(json!({ "id": name, "kind": "claude-code", "model": "fake" }));
        staff.push(json!({ "agent": name, "roles": ["worker", "reviewer"] }));
    }
    rig.stand_ins(&json!({ "schemaVersion": 1, "agents": agents }))?;
    let project = Project::open(
        rig,
        "chief",
        json!({
            "staff": staff,
            "specs": { "on": true, "folder": "docs/specs", "variant": "tdd" }
        }),
    )?;
    let said = rig.page(
        "project.rules",
        json!({ "project": project.id(), "rules": the_most_rules() }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    // The chief's window opened with the project, before its rules: its next one has them.
    crate::specs::relaunch(rig, &project, 2)?;
    Ok(project)
}

/// The role text of the chief's latest launch: the file its hooks read, in the launch's folder
/// (the newest of the rig's, which is the one the chief's window runs on).
fn the_latest_role(rig: &Rig) -> Option<String> {
    let launches = rig.home().join("integrations").join("muse");
    let mut roles: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(launches)
        .ok()?
        .flatten()
        .map(|launch| {
            launch
                .path()
                .join("role/.claude/skills/consensflow-chief/SKILL.md")
        })
        .filter_map(|file| Some((fs::metadata(&file).ok()?.modified().ok()?, file)))
        .collect();
    roles.sort();
    fs::read_to_string(roles.pop()?.1).ok()
}

/// What the chief's latest session says of its SessionStart hooks: the session's records once
/// the hooks have run in it.
fn hooked(rig: &Rig) -> Option<Vec<Value>> {
    rig.muse_sessions()
        .into_iter()
        .rev()
        .map(|(_, log)| muse::records(&log))
        .find(|records| !hook_runs(records).is_empty())
}

#[test]
fn a_muse_chief_with_the_largest_role_is_given_all_of_it_by_its_hooks() -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = largest_chiefs_project(&rig)?;
    rig.tell(project.id(), "Reply with exactly: ROLE_GIVEN")?;
    rig.wait_for(
        "the chief's Muse to run its SessionStart hooks",
        secs(120),
        || Ok(hooked(&rig).is_some()),
    )?;
    let log = hooked(&rig).ok_or("the chief's session has no hook run")?;
    let role = the_latest_role(&rig).ok_or("the chief's launch wrote no role")?;
    // The role is the largest ConsensFlow makes: past what one hook says.
    assert!(
        role.len() > MUSE_HOOK_OUTPUT_LIMIT * 3 / 2,
        "the case is the real thing: {} bytes",
        role.len()
    );
    assert!(role.contains("## This project's specs"), "specs on");
    assert!(role.contains("member-24"), "all of the staff");
    // Every hook ended well. (Before T-1246 the one hook ended `failed`: `output_too_large:
    // hook stdout exceeded its 16384-byte ceiling; the process tree was terminated and no
    // stream was parsed`.)
    let runs = hook_runs(&log);
    assert!(
        !runs.is_empty() && runs.iter().all(|(status, _)| status == "completed"),
        "the hooks: {runs:?}"
    );
    // And the context they gave is the whole role, in order.
    let blocks = hook_blocks(&log);
    assert_eq!(
        blocks.iter().map(|(number, _)| *number).collect::<Vec<_>>(),
        (0..blocks.len() as u32).collect::<Vec<_>>(),
        "one block for each hook, from the first"
    );
    assert!(
        blocks.iter().all(|(_, text)| text.len() <= MUSE_HOOK_OUTPUT_LIMIT),
        "{:?}",
        blocks.iter().map(|(_, text)| text.len()).collect::<Vec<_>>()
    );
    let given: String = blocks.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(given.len(), role.len());
    assert!(given == role, "the context is not the role text");
    rig.close()?;
    Ok(())
}
