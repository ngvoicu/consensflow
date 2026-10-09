//! What `cf` leaves alone of a person's saved data: a roster an older version
//! wrote, the installation and the history a retired command once cleared, and
//! the role files of a launch.

use std::path::PathBuf;

use cf_e2e::{checkout, files, ScratchHome};
use serde_json::Value;

use crate::{agent, launcher, role_file, Outcome};

/// Where the roster is kept in a home of a case's own.
fn roster(home: &ScratchHome) -> PathBuf {
    home.consensflow().join("agents.json")
}

/// Whether JavaScript takes `value` for true, as a check that it is set does.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[test]
fn setup_preserves_a_legacy_roster_and_prepares_no_role_or_manifest_before_pane_launch() -> Outcome
{
    let home = ScratchHome::new()?;
    let kept = roster(&home);
    files::copy(&checkout::path("tests/fixtures/v1-agents.json"), &kept)?;
    let before = files::read(&kept)?;
    for _ in 0..2 {
        let ran = home.cf(["setup"])?;
        assert_eq!(ran.code, Some(0), "{ran}");
    }
    assert_eq!(files::read(&kept)?, before);
    // Role files are a launch's: setup writes none.
    assert!(!home.consensflow().join("integrations").exists());
    assert!(!home.consensflow().join("skills-manifest.json").exists());
    let listed = home.cf(["agent", "list"])?;
    assert!(listed.stdout.contains("pygmalion"), "{listed}");
    Ok(())
}

/// The bytes of each of `paths`.
fn read_all(paths: &[PathBuf]) -> cf_e2e::Result<Vec<Vec<u8>>> {
    paths.iter().map(|path| files::read(path)).collect()
}

/// `cf <args>`, a command that is retired, is answered as no command, and the
/// installation and the history are as they were.
fn rejected(args: &[&str]) -> Outcome {
    let home = ScratchHome::new()?;
    home.cf([
        "agent",
        "add",
        "mine",
        "--harness",
        "claude",
        "--model",
        "example",
    ])?;
    let set_up = home.cf(["setup"])?;
    assert_eq!(set_up.code, Some(0), "{set_up}");
    let history = home
        .consensflow()
        .join("workspaces")
        .join("project")
        .join("runs")
        .join("run-1");
    files::make_dir(&history)?;
    let saved = [
        roster(&home),
        home.bin().join(launcher()),
        history.join("answer.txt"),
    ];
    files::write(&saved[2], "saved result")?;
    let before = read_all(&saved)?;
    let ran = home.cf(args)?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert!(
        ran.stderr.to_lowercase().contains("unknown command"),
        "{ran}"
    );
    assert_eq!(read_all(&saved)?, before);
    Ok(())
}

#[test]
fn rejects_cf_off() -> Outcome {
    rejected(&["off"])
}

#[test]
fn rejects_cf_off_force() -> Outcome {
    rejected(&["off", "--force"])
}

#[test]
fn rejects_cf_reset() -> Outcome {
    rejected(&["reset"])
}

#[test]
fn rejects_cf_reset_yes() -> Outcome {
    rejected(&["reset", "--yes"])
}

#[test]
fn the_chief_can_discover_saved_capability_profiles_without_refreshing_or_changing_role_files(
) -> Outcome {
    let home = ScratchHome::new()?;
    let added = home.cf([
        "agent",
        "add",
        "mine",
        "--harness",
        "codex",
        "--model",
        "gpt-5.6-luna",
    ])?;
    assert_eq!(added.code, Some(0), "{}", added.stderr);
    let refused = home.cf(["agent", "add", "hyperion"])?;
    assert_eq!(refused.code, Some(1), "{refused}");
    assert!(refused.stderr.contains("catalog agent"), "{refused}");
    let role = role_file(&home);
    files::write(&role, "Keep existing chief context untouched")?;
    let kept = roster(&home);
    let roster_before = files::read_string(&kept)?;
    let result = home.cf_with([("CONSENSFLOW_ROLE", "chief")], ["agent", "list", "--json"])?;
    assert_eq!(result.code, Some(0), "{}", result.stderr);
    let listed = result.json()?;
    let hyperion = agent(&listed, "hyperion");
    assert!(!hyperion.is_null(), "every catalog agent is listed");
    assert_eq!(agent(&listed, "mine")["custom"], true, "{listed}");
    assert!(
        hyperion["profile"]
            .as_object()
            .is_some_and(|profile| !profile.contains_key("categories")),
        "no role pills: {hyperion}"
    );
    assert!(truthy(&hyperion["profile"]["workTier"]), "{hyperion}");
    assert_eq!(files::read_string(&kept)?, roster_before);
    assert_eq!(
        files::read_string(&role)?,
        "Keep existing chief context untouched"
    );
    Ok(())
}
