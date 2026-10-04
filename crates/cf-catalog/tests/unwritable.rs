//! What the roster says when it cannot save: the golden of the platform this
//! runs on (`tests/goldens/unwritable/<platform>.json`, which `npm run
//! goldens:unwritable` writes by playing each situation through the real
//! `src/roster.js`). Each situation is made again under a temporary root,
//! the same call is made, and the refusal's message is held to Node's, as
//! text, with the root and this process's id put back. A platform with no
//! golden fails: it is to be recorded there.
//!
//! The roster reads the file before it writes it, so on Unix some failures
//! of the write are out of its reach (a folder that is a file is refused at
//! the read). The situations that call `saveDocument` are for those: they
//! write the file as the roster would, with no read first.

// The golden's own reading and the scaffolding of each situation: a failure
// in them is the test's.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::write_whole;
use cf_base::refusal::Refusal;
use cf_base::time::Clock;
use cf_catalog::{roster_path, Catalog, Roster};
use serde_json::Value;

/// The platform as Node names it (`process.platform`).
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// The number of situations the golden of `platform` holds: a golden that
/// shrinks fails, and is recorded again on purpose. Windows leaves out the
/// five that need a folder's permissions, and has a name only it refuses.
fn situations_in_the_golden(platform: &str) -> usize {
    match platform {
        "darwin" => 15,
        "win32" => 11,
        other => panic!("no situations are counted for {other}: say how many its golden holds"),
    }
}

fn golden() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("unwritable")
        .join(format!("{}.json", platform()));
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "no failed-save golden for {}: {error}; run `node tests/goldens/unwritable/record.mjs` on it ({})",
            platform(),
            path.display()
        )
    });
    let golden: Value = serde_json::from_str(&text).unwrap();
    golden["situations"].as_array().unwrap().clone()
}

/// A path of a step under `root`, with this process's id in it.
fn under(root: &Path, relative: &str) -> PathBuf {
    relative
        .replace("$PID", &std::process::id().to_string())
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// The paths a situation sets the permissions of, with their bits.
fn modes(root: &Path, steps: &[Value]) -> Vec<(PathBuf, u32)> {
    steps
        .iter()
        .filter_map(|step| {
            let path = under(root, step["mode"].as_str()?);
            Some((
                path,
                u32::from_str_radix(step["bits"].as_str()?, 8).unwrap(),
            ))
        })
        .collect()
}

#[cfg(unix)]
fn set_mode(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
}

/// Node's `chmod` on Windows: the read-only attribute, set when the owner
/// may not write. No Windows situation sets a folder's.
#[cfg(windows)]
fn set_mode(path: &Path, bits: u32) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly((bits & 0o200) == 0);
    fs::set_permissions(path, permissions).unwrap();
}

/// Makes the situation: its folders, files and permissions, in order.
fn make(root: &Path, steps: &[Value]) {
    for step in steps {
        if let Some(folder) = step["folder"].as_str() {
            fs::create_dir_all(under(root, folder)).unwrap();
        } else if let Some(file) = step["file"].as_str() {
            let path = under(root, file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, step["text"].as_str().unwrap()).unwrap();
        }
    }
    for (path, bits) in modes(root, steps) {
        set_mode(&path, bits);
    }
}

/// Whether a permission holds for this user: root writes where it likes.
#[cfg(unix)]
fn permissions_hold() -> bool {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("ro");
    fs::create_dir(&folder).unwrap();
    set_mode(&folder, 0o555);
    let holds = fs::File::create(folder.join("probe")).is_err();
    set_mode(&folder, 0o755);
    holds
}

#[cfg(windows)]
fn permissions_hold() -> bool {
    true
}

/// The clock at one instant: the time is no part of a refusal.
struct At(i64);

impl Clock for At {
    fn now_ms(&mut self) -> i64 {
        self.0
    }
}

/// The call a situation makes, and what it refused with. `saveDocument`
/// writes the text it is given to the roster's file as the roster does.
fn refused(roster: &Roster<'_>, file: &Path, call: &[Value]) -> Result<(), Refusal> {
    match call[0].as_str().unwrap() {
        "setPreferences" => roster.set_preferences(Some(&call[1])).map(|_| ()),
        "addAgent" => roster
            .add(call[1].as_object().unwrap(), &mut At(0))
            .map(|_| ()),
        "saveDocument" => write_whole(file, call[1].as_str().unwrap().as_bytes())
            .map_err(|error| Refusal::new("agents-file-unwritable", error.to_string())),
        other => panic!("{other} is no call the golden makes"),
    }
}

#[test]
fn every_save_that_fails_says_what_node_says_of_it() {
    let catalog = Catalog::bundled().unwrap();
    let situations = golden();
    assert_eq!(situations.len(), situations_in_the_golden(platform()));
    let holds = permissions_hold();
    let mut skipped = Vec::new();
    for situation in &situations {
        let name = situation["name"].as_str().unwrap();
        let steps = situation["make"].as_array().unwrap();
        let root = tempfile::tempdir().unwrap();
        let permissions = modes(root.path(), steps);
        if !holds && !permissions.is_empty() {
            skipped.push(name);
            continue;
        }
        make(root.path(), steps);
        let home = under(root.path(), situation["home"].as_str().unwrap());
        let env = Env::from_vars([("CONSENSFLOW_HOME", home)]);
        let file = roster_path(&env).unwrap();
        let roster = Roster::new(&catalog, file.clone());
        let outcome = refused(&roster, &file, situation["call"].as_array().unwrap());
        // A folder with no permission cannot be removed with what is in it,
        // nor a read-only file on Windows; a file the save removed is gone.
        for (path, _) in &permissions {
            if fs::symlink_metadata(path).is_ok() {
                set_mode(path, 0o755);
            }
        }
        let Err(refusal) = outcome else {
            panic!("{name}: saved, where Node refused");
        };
        let expected = situation["message"]
            .as_str()
            .unwrap()
            .replace("$ROOT", &root.path().display().to_string())
            .replace("$PID", &std::process::id().to_string());
        assert_eq!(refusal.message, expected, "{name}");
        let code = match situation["stage"].as_str().unwrap() {
            "read" => "agents-file-unreadable",
            "write" => "agents-file-unwritable",
            other => panic!("{name}: {other} is no stage"),
        };
        assert_eq!((refusal.code, refusal.status), (code, 400), "{name}");
    }
    println!(
        "{} situations of {}, {} skipped for want of permissions that hold: {skipped:?}",
        situations.len(),
        platform(),
        skipped.len()
    );
}
