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

/// Codex's standalone program, as a release of it holds it: it prints the version of the
/// release it is in (`VERSION` beside it), and its update does what the installer's script does
/// once it has a new release in the releases folder: it moves `current` to it
/// (`install.sh`'s `update_current_link`).
const STANDALONE: &str = r#"#!/bin/sh
here="$(cd "$(dirname "$0")" && pwd -P)"
if [ "$1" = "update" ]; then
  root="$(cd "$here/../../.." && pwd -P)"
  new="$root/releases/0.167.0-x86_64-unknown-linux-musl"
  mkdir -p "$new/bin"
  cp "$here/codex" "$new/bin/codex"
  printf '0.167.0\n' > "$new/VERSION"
  ln -sfn "$new" "$root/current"
  echo "Updating Codex via the installer"
else
  cat "$here/../VERSION"
fi
"#;

/// Codex as its standalone installer lays it down (a release, a `current` link to it, the visible
/// folder a link to `current/bin`): the Admin page's Update runs Codex's own update by the visible
/// link, which moves `current` to the new release, and the look after it, by the same link, reads
/// the new version.
#[test]
fn a_standalone_codex_is_updated_through_its_visible_link_and_read_again_from_the_release_current_moved_to(
) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let standalone = root
        .join("home")
        .join(".codex")
        .join("packages")
        .join("standalone");
    let first = standalone
        .join("releases")
        .join("0.166.0-x86_64-unknown-linux-musl");
    fs::create_dir_all(first.join("bin")).unwrap();
    let codex = first.join("bin").join("codex");
    fs::write(&codex, STANDALONE).unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(first.join("VERSION"), "0.166.0\n").unwrap();
    std::os::unix::fs::symlink(&first, standalone.join("current")).unwrap();
    let visible = root.join("local").join("bin");
    fs::create_dir_all(root.join("local")).unwrap();
    std::os::unix::fs::symlink(standalone.join("current").join("bin"), &visible).unwrap();
    let env = Env::from_vars([
        ("HOME", root.join("home").to_str().unwrap().to_owned()),
        ("PATH", format!("{}:/usr/bin:/bin", visible.display())),
        (
            "CONSENSFLOW_HOME",
            root.join("cf").to_str().unwrap().to_owned(),
        ),
    ]);
    let latest = Rc::new(ScriptedLatest::default());
    for _ in 0..4 {
        latest.says(Harness::Codex, "0.167.0");
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
    assert_eq!(row.version.value(), Some("0.166.0"));
    let json: Value = serde_json::to_value(&*row).unwrap();
    assert_eq!(json["distribution"], "Codex's installer");
    assert_eq!(json["update"]["state"], "available");

    let Outcome::Ran {
        state,
        before,
        after,
        command,
        ..
    } = runtime.block_on(admin.update("codex")).unwrap()
    else {
        panic!("it ran")
    };
    assert_eq!(format!("{state:?}"), "Updated");
    assert_eq!(
        (before.as_deref(), after.as_deref()),
        (Some("0.166.0"), Some("0.167.0"))
    );
    assert_eq!(
        command,
        format!("{} update", visible.join("codex").display())
    );
    assert_eq!(
        fs::canonicalize(standalone.join("current")).unwrap(),
        standalone
            .join("releases")
            .join("0.167.0-x86_64-unknown-linux-musl"),
        "current leads to the new release"
    );
}
