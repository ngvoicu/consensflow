//! npm's global folder on Windows as npm lays it out (zeewin2, 2026-11-09: Node 26 from the
//! installer, npm 11): the prefix holds the shims of each global CLI (`codex.cmd`, `codex.ps1` and
//! `codex`, a shell script) and `node_modules` beside them, with the package under it. The default
//! prefix is `%APPDATA%\npm`, which the Node installer's npm (in `C:\Program Files\nodejs`, so
//! another folder) names; a custom one (`npm config set prefix`, or a portable Node folder, or
//! nvm-windows's) has the same shape, and may hold its own `npm.cmd`. Made in a throwaway folder.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_proto::agents::Harness;

/// The package a harness is installed as with npm and the file its shim runs.
pub(in crate::admin) fn package_of(harness: Harness) -> (&'static str, &'static str) {
    match harness {
        Harness::Codex => ("@openai/codex", "bin/codex.js"),
        Harness::Claude => ("@anthropic-ai/claude-code", "cli.js"),
        Harness::Pi => ("@earendil-works/pi-coding-agent", "dist/bundle/cli.js"),
        other => panic!("{other:?} is not installed with npm"),
    }
}

/// A global install of `harness` in the npm prefix `prefix`: its shims beside `node_modules`, and
/// the package in it. The `.cmd` shim, where the person runs it from.
pub(in crate::admin) fn lay_npm(prefix: &Path, harness: Harness) -> PathBuf {
    let (package, entry) = package_of(harness);
    let name = harness.as_str();
    let package_folder = prefix.join("node_modules").join(package);
    fs::create_dir_all(package_folder.join(Path::new(entry).parent().unwrap())).unwrap();
    fs::write(package_folder.join("package.json"), "{}").unwrap();
    fs::write(package_folder.join(entry), "").unwrap();
    for shim in [
        format!("{name}.cmd"),
        format!("{name}.ps1"),
        name.to_owned(),
    ] {
        executable(&prefix.join(shim), "");
    }
    prefix.join(format!("{name}.cmd"))
}

/// An `npm.cmd` in `folder`, a file a user can start.
pub(in crate::admin) fn lay_npm_cmd(folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    let npm = folder.join("npm.cmd");
    executable(&npm, "");
    npm
}

/// A file that holds `text`, which a user can start.
pub(in crate::admin) fn executable(file: &Path, text: &str) {
    fs::write(file, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The environment of a Windows user: its PATH the folders `path`, in order, and the profile
/// `home`. Windows' own on Windows, and what it says of itself (`OS`) on any other system.
pub(in crate::admin) fn windows_env(path: &[&Path], home: &Path) -> Env {
    Env::from_vars([
        ("OS", "Windows_NT".to_owned()),
        (
            "PATH",
            std::env::join_paths(path)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
        ("HOME", home.to_string_lossy().into_owned()),
        ("USERPROFILE", home.to_string_lossy().into_owned()),
    ])
}

/// `path` where it leads, spelled as the admin spells a real path: without the `\\?\` prefix
/// Windows gives a resolved one.
pub(in crate::admin) fn canonical(path: &Path) -> PathBuf {
    let real = fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    PathBuf::from(
        real.strip_prefix(r"\\?\")
            .map_or(real.as_str(), |plain| plain),
    )
}
