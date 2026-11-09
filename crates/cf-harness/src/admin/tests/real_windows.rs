//! The admin over Windows' own programs: a Codex installed with npm as npm lays it out there, whose
//! shim and npm are batch files (stand-ins), run for real through cmd.exe as the admin runs a script,
//! the feed alone scripted.

use std::fs;
use std::rc::Rc;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use serde_json::Value;

use crate::admin::tests::npm_windows::{canonical, lay_npm, lay_npm_cmd};
use crate::admin::{Capture, HarnessAdmin, Latest, Outcome};
use crate::seams::{SystemProcesses, SystemTime, Time};
use crate::testing::ScriptedLatest;

/// The shim: it prints the version a file beside it holds.
const SHIM: &str = "@echo off\r\nset /p V=<\"%~dp0VERSION\"\r\necho codex-cli %V%\r\n";

/// npm: like npm's own shim it hands `%*` on to what runs (here a label, by `call`), which is where
/// the second layer of cmd.exe escaping the admin puts on every argument is read; that writes the
/// arguments it was given, the prefix (the fourth) read as `%~4` reads one with a space in it, and
/// the new version beside the shim. (A batch file is handed each argument quoted, as cmd.exe
/// quotes them: `%~1` reads one without.)
const NPM: &str = "@echo off\r\n\
call :record %*\r\n\
echo added 1 package\r\n\
exit /b 0\r\n\
:record\r\n\
echo %~1> \"%~4\\npm-args.txt\"\r\n\
echo %~2>> \"%~4\\npm-args.txt\"\r\n\
echo %~3>> \"%~4\\npm-args.txt\"\r\n\
echo %~4>> \"%~4\\npm-args.txt\"\r\n\
echo %~5>> \"%~4\\npm-args.txt\"\r\n\
echo 0.167.1> \"%~4\\VERSION\"\r\n\
exit /b 0\r\n";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn an_npm_install_on_windows_is_updated_for_real_through_the_npm_found_told_its_prefix_and_read_again(
) {
    let temp = tempfile::tempdir().unwrap();
    // Where it leads, as the admin spells a real path: a runner's temp folder has a short name
    // (`RUNNER~1`) the admin does not give.
    let root = canonical(temp.path());
    let prefix = root
        .join("Users")
        .join("A B")
        .join("AppData")
        .join("Roaming")
        .join("npm");
    let nodejs = root.join("Program Files").join("nodejs");
    let shim = lay_npm(&prefix, Harness::Codex);
    fs::write(&shim, SHIM).unwrap();
    fs::write(prefix.join("VERSION"), "0.167.0\r\n").unwrap();
    let npm = lay_npm_cmd(&nodejs);
    fs::write(&npm, NPM).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    let path = std::env::join_paths([&prefix, &nodejs]).unwrap();
    let env = Env::from_vars([
        ("PATH", path.to_string_lossy().into_owned()),
        ("HOME", root.join("home").to_string_lossy().into_owned()),
        (
            "USERPROFILE",
            root.join("home").to_string_lossy().into_owned(),
        ),
        (
            "CONSENSFLOW_HOME",
            root.join("cf").to_string_lossy().into_owned(),
        ),
    ]);
    let latest = Rc::new(ScriptedLatest::default());
    for _ in 0..4 {
        latest.says(Harness::Codex, "0.167.1");
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
    assert_eq!(row.version.value(), Some("0.167.0"), "{row:?}");
    let json: Value = serde_json::to_value(&*row).unwrap();
    assert_eq!(json["distribution"], "npm");
    assert_eq!(json["update"]["state"], "available");

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
    assert_eq!(format!("{state:?}"), "Updated", "{reason:?} {output}");
    assert_eq!(
        (before.as_deref(), after.as_deref()),
        (Some("0.167.0"), Some("0.167.1"))
    );
    assert_eq!(output, "added 1 package");
    // The prefix, a path with a space in it, reached npm as one argument.
    let written = fs::read_to_string(prefix.join("npm-args.txt")).unwrap();
    let lines: Vec<&str> = written.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        [
            "install",
            "-g",
            "--prefix",
            prefix.to_string_lossy().as_ref(),
            "@openai/codex@latest"
        ]
    );
}
