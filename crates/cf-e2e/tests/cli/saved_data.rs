//! What `cf` leaves alone of a person's saved data: an agents file an older version
//! wrote (which this one reads nothing of but the app's specs setting), the installation
//! and the history a retired command once cleared, and the role files of a launch.

use std::path::PathBuf;

use cf_e2e::{checkout, files, ScratchHome};

use crate::{launcher, Outcome};

/// Where an older version kept its roster, in a home of a case's own.
fn roster(home: &ScratchHome) -> PathBuf {
    home.consensflow().join("agents.json")
}

#[test]
fn setup_leaves_a_legacy_agents_file_as_it_is_and_prepares_no_role_or_manifest_before_pane_launch(
) -> Outcome {
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
    files::write(&roster(&home), r#"{"schemaVersion":1,"agents":[]}"#)?;
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
