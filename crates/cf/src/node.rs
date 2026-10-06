//! The way back to Node: the CLI's sources, `cf.mjs` beside this binary, run on
//! the Node the bundle carries. A home that has taken it (the `use-node` file in
//! it) sends every command of a terminal here, as it sends the app's daemon.
//! Nothing else does: it is run with the file that sent the home, and for no
//! other reason.
//!
//! The Node is found from this binary's own place in the bundle, and from
//! nowhere else: a terminal has no `CONSENSFLOW_NODE` (the app names it to the
//! windows it opens, not to a shell), and a `node` off `PATH` could be any
//! Node, the one an older install left first on it.
//!
//! ```text
//! macOS     <app>/Contents/Resources/cli/bin/cf     Node: Resources/binaries/node
//!                                                         or <app>/Contents/MacOS/node
//! Windows   <root>\cli\bin\cf.exe                   Node: <root>\binaries\node.exe
//!                                                         or <root>\node.exe
//! ```
//!
//! Each is where the app itself looks for the runtime it starts its daemon on
//! (`bundled_cli` in the app's `daemon_command.rs`), in the same order. The
//! Windows layout is the installed app's and the portable runtime's alike: the
//! runtime folder the portable exe unpacks holds `node.exe` and `cli` side by
//! side.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Where the bundle keeps its Node, for the `cf` at `cf`, in the order they
/// are looked in: the places of the layout above. `windows` says whose layout.
fn candidates(cf: &Path, windows: bool) -> Vec<PathBuf> {
    // `cf` is `<resources>/cli/bin/cf`: three folders up is the folder the
    // bundle's resources are in, which on Windows is the install folder.
    let Some(resources) = cf.ancestors().nth(3) else {
        return Vec::new();
    };
    if windows {
        return vec![
            resources.join("binaries").join("node.exe"),
            resources.join("node.exe"),
        ];
    }
    let mut places = vec![resources.join("binaries").join("node")];
    places.extend(
        resources
            .parent()
            .map(|contents| contents.join("MacOS").join("node")),
    );
    places
}

/// The Node of the bundle `cf` is in: the first place it is looked for that
/// has one, or every place that was looked in.
fn bundled_node(cf: &Path) -> Result<PathBuf, Vec<PathBuf>> {
    let tried = candidates(cf, cfg!(windows));
    tried
        .iter()
        .find(|node| node.is_file())
        .cloned()
        .ok_or(tried)
}

/// What is said where no Node is bundled: which file asked for one, where it
/// was looked for, and what to do.
fn missing(file: &Path, script: &Path, tried: &[PathBuf]) -> String {
    let places: Vec<_> = tried
        .iter()
        .map(|place| place.display().to_string())
        .collect();
    format!(
        "cf: {} sends this home's commands to Node, and none is bundled beside this cf (looked for {}): delete the file to run the native cf, or run {} with a node of your choosing.",
        file.display(),
        places.join(", "),
        script.display()
    )
}

/// Runs `cf.mjs` with `args` in this process's place, on the Node of this
/// bundle: an exit code only when it could not be run. `file` is the one that
/// sent the home to Node (`Choice::node_file`), which a refusal names.
pub fn run(args: &[OsString], file: &Path, err: &mut dyn Write) -> io::Result<u8> {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(cause) => {
            writeln!(err, "cf: cannot tell which folder it is in: {cause}")?;
            return Ok(1);
        }
    };
    let script = exe.with_file_name("cf.mjs");
    let node = match bundled_node(&exe) {
        Ok(node) => node,
        Err(tried) => {
            writeln!(err, "{}", missing(file, &script, &tried))?;
            return Ok(1);
        }
    };
    let mut command = vec![script.into_os_string()];
    command.extend(args.iter().cloned());
    let cause = cf_process::run_in_place(node.as_os_str(), &command);
    writeln!(err, "cf: {} did not start: {cause}", node.display())?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle's folders, with `files` made in them.
    fn bundle(files: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a bundle");
        for file in files {
            let path = root.path().join(file);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("folders");
            std::fs::write(path, "").expect("a file");
        }
        root
    }

    #[test]
    fn the_macs_node_is_in_resources_or_beside_the_apps_own_executable() {
        let root = bundle(&[]);
        let cf = root.path().join("Contents/Resources/cli/bin/cf");
        assert_eq!(
            candidates(&cf, false),
            [
                root.path().join("Contents/Resources/binaries/node"),
                root.path().join("Contents/MacOS/node"),
            ]
        );
    }

    #[test]
    fn windows_node_is_beside_the_cli_folder_or_in_binaries_there() {
        let root = bundle(&[]);
        let cf = root.path().join("cli/bin/cf.exe");
        assert_eq!(
            candidates(&cf, true),
            [
                root.path().join("binaries/node.exe"),
                root.path().join("node.exe"),
            ]
        );
    }

    #[test]
    fn a_cf_with_no_folders_above_it_has_no_place_to_look() {
        assert!(candidates(Path::new("cf"), false).is_empty());
        assert!(candidates(Path::new("/cf"), true).is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn the_first_place_that_has_a_node_is_the_one() {
        let root = bundle(&[
            "Contents/Resources/cli/bin/cf",
            "Contents/MacOS/node",
            "Contents/Resources/binaries/node",
        ]);
        let cf = root.path().join("Contents/Resources/cli/bin/cf");
        assert_eq!(
            bundled_node(&cf),
            Ok(root.path().join("Contents/Resources/binaries/node"))
        );
        std::fs::remove_file(root.path().join("Contents/Resources/binaries/node"))
            .expect("one is gone");
        assert_eq!(
            bundled_node(&cf),
            Ok(root.path().join("Contents/MacOS/node"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn a_bundle_with_no_node_says_every_place_it_looked() {
        let root = bundle(&["Contents/Resources/cli/bin/cf"]);
        let cf = root.path().join("Contents/Resources/cli/bin/cf");
        assert_eq!(
            bundled_node(&cf),
            Err(vec![
                root.path().join("Contents/Resources/binaries/node"),
                root.path().join("Contents/MacOS/node"),
            ])
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_bundle_with_no_node_says_where_it_looked() {
        let root = bundle(&["cli/bin/cf.exe"]);
        let cf = root.path().join("cli/bin/cf.exe");
        assert_eq!(
            bundled_node(&cf),
            Err(vec![
                root.path().join("binaries").join("node.exe"),
                root.path().join("node.exe"),
            ])
        );
        std::fs::write(root.path().join("node.exe"), "").expect("a node");
        assert_eq!(bundled_node(&cf), Ok(root.path().join("node.exe")));
    }

    #[test]
    fn the_refusal_names_the_file_that_asked_for_node_and_how_to_be_rid_of_it() {
        let tried = [PathBuf::from("/app/Contents/MacOS/node")];
        let script = Path::new("/app/Contents/Resources/cli/bin/cf.mjs");
        let file = Path::new("/home/me/.consensflow/use-node");
        assert_eq!(
            missing(file, script, &tried),
            "cf: /home/me/.consensflow/use-node sends this home's commands to Node, and none is bundled \
             beside this cf (looked for /app/Contents/MacOS/node): delete the file to run the native cf, \
             or run /app/Contents/Resources/cli/bin/cf.mjs with a node of your choosing."
        );
    }
}
