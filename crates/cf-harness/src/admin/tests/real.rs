//! The admin over the system's own programs: a CLI that is a script, asked
//! its version and updated for real, through the seam the daemon gives
//! (`SystemProcesses`), the feed alone scripted.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use serde_json::Value;

use crate::admin::{Capture, HarnessAdmin, Latest, Outcome};
use crate::seams::{SystemProcesses, SystemTime, Time};
use crate::testing::ScriptedLatest;

/// Codex as its own installer puts it: it prints the version a file holds,
/// and its updater writes to both streams and rewrites the file, or fails
/// when a file says so.
const CODEX: &str = r#"#!/bin/sh
if [ "$1" = "update" ]; then
  if [ -e "$HOME/fail" ]; then
    echo "no network" 1>&2
    exit 1
  fi
  echo "Updated to 1.0.1"
  echo "a warning" 1>&2
  printf '1.0.1\n' > "$HOME/version"
else
  cat "$HOME/version"
fi
"#;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn a_cli_is_asked_its_version_and_updated_for_real_through_the_systems_programs() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".codex").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let codex = bin.join("codex");
    fs::write(&codex, CODEX).unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(home.join("version"), "1.0.0\n").unwrap();
    let env = Env::from_vars([
        ("HOME", home.to_str().unwrap()),
        ("PATH", "/usr/bin:/bin"),
        ("CONSENSFLOW_HOME", root.path().join("cf").to_str().unwrap()),
    ]);
    let latest = Rc::new(ScriptedLatest::default());
    for _ in 0..4 {
        latest.says(Harness::Codex, "1.0.1");
    }
    let admin = HarnessAdmin::new(
        env.clone(),
        Rc::new(SystemTime) as Rc<dyn Time>,
        Rc::clone(&latest) as Rc<dyn Latest>,
        Rc::new(SystemProcesses::new(Env::from_process())) as Rc<dyn Capture>,
    );
    let runtime = runtime();
    let row = runtime
        .block_on(admin.check(Some("codex"), false))
        .unwrap()
        .remove(0);
    assert_eq!(row.version.value(), Some("1.0.0"));
    let json: Value = serde_json::to_value(&*row).unwrap();
    assert_eq!(json["update"]["state"], "available");
    assert_eq!(
        json["update"]["command"],
        format!("{} update", codex.display())
    );

    let Outcome::Ran {
        state,
        before,
        after,
        output,
        reason,
        ..
    } = runtime.block_on(admin.update("codex")).unwrap()
    else {
        panic!("it ran")
    };
    assert_eq!(format!("{state:?}"), "Updated");
    assert_eq!(
        (before.as_deref(), after.as_deref()),
        (Some("1.0.0"), Some("1.0.1"))
    );
    assert_eq!(output, "Updated to 1.0.1\na warning", "stdout, then stderr");
    assert_eq!(reason, None);

    fs::write(home.join("fail"), "").unwrap();
    let Outcome::Ran {
        state,
        output,
        reason,
        ..
    } = runtime.block_on(admin.update("codex")).unwrap()
    else {
        panic!("it ran")
    };
    assert_eq!(format!("{state:?}"), "Failed");
    assert_eq!(output, "no network");
    assert_eq!(
        reason.as_deref(),
        Some(format!("Command failed: {} update\nno network\n", codex.display()).as_str())
    );
}
