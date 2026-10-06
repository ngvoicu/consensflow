//! The way back to Node, for the flip release alone: which implementation
//! writes a home.
//!
//! The native `cf` and the native daemon are the default. A user whose home
//! trips on them goes back by making a file named [`FILE`] in ConsensFlow's
//! home (`~/.consensflow`, or the folder `CONSENSFLOW_HOME` names), and returns
//! by deleting it. While the file is there everything that writes that home
//! runs on Node: the app's daemon (`node cf.mjs ui`) and every `cf` verb
//! typed in a terminal. Without it everything runs native.
//!
//! The file is the only input. An environment variable cannot be the way back:
//! the app takes only the login shell's PATH, so one left in a shell profile
//! would run the terminal's verbs on Node while the app ran native, and one
//! file would have two writers. A home has one answer, and every reader of it
//! asks [`choose`]. `src/use-node.js` is the same function for `cf.mjs`;
//! `tests/way-back.json` here holds the two to one answer, case by case.
//!
//! What counts as the file is the name being there. Its content is never read,
//! and neither is the file: whatever `stat` finds at the name, following
//! links, is the user's act, and a name nothing can be found at is no act.
//! So a file, an empty one, a folder (what Finder makes of "New Folder") and a
//! file nobody may read all send the home to Node: reading it is no part of
//! the choice, so a failure to read it cannot be one, and a way back must be
//! easy to take. A link to nowhere, a home whose folder cannot be searched, and
//! no home at all send it to native: a daemon cannot keep the ledger of a home
//! it cannot reach either, so there is nothing there to be wrong about.
//!
//! Removed with Node, in the release that deletes it.

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::home::config_root;
use crate::path;

/// The name of the file whose presence in the home is the choice.
pub const FILE: &str = "use-node";

/// What the choice came to for a home, and what it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// Where the file is looked for: none when ConsensFlow has no folder to
    /// keep its things in.
    pub file: Option<PathBuf>,
    /// Whether Node writes the home: the file is there.
    pub node: bool,
}

/// The implementation that writes the home `env` names, and where that was
/// read. Nothing else in the environment is looked at.
pub fn choose(env: &Env) -> Choice {
    let file =
        config_root(env).map(|root| PathBuf::from(path::join(&[&root.to_string_lossy(), FILE])));
    let node = file.as_deref().is_some_and(Path::exists);
    Choice { file, node }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_in_the_home_the_environment_names() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", "/work/cf")]);
        assert_eq!(
            choose(&env).file,
            Some(PathBuf::from(path::join(&["/work/cf", "use-node"])))
        );
        let user = Env::from_vars([("HOME", "/home/me")]);
        assert_eq!(
            choose(&user).file,
            Some(PathBuf::from(path::join(&[
                "/home/me",
                ".consensflow",
                "use-node"
            ])))
        );
    }

    #[test]
    fn with_no_home_to_look_in_nothing_is_looked_for_and_it_is_native() {
        assert_eq!(
            choose(&Env::default()),
            Choice {
                file: None,
                node: false
            }
        );
    }

    #[test]
    fn a_home_with_no_file_is_native() {
        let home = tempfile::tempdir().expect("a home");
        let env = Env::from_vars([("CONSENSFLOW_HOME", home.path())]);
        assert!(!choose(&env).node);
    }

    #[test]
    fn a_home_with_the_file_is_node() {
        let home = tempfile::tempdir().expect("a home");
        std::fs::write(home.path().join(FILE), "").expect("the file");
        let env = Env::from_vars([("CONSENSFLOW_HOME", home.path())]);
        assert!(choose(&env).node);
    }
}
