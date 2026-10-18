//! What a window leaves in ConsensFlow's folder under its launch id: the
//! settings and integration files written for it, in
//! `integrations/<harness>/<launch>`. A launch id lives as long as its window,
//! and nothing else reads these folders, so they go when it does, and every one
//! a start finds once it has settled what was on its way, which may read a
//! window's record by them. What is kept from one launch to the next is in no harness's
//! folder: ConsensFlow's own copy of Specmint is in `integrations/specmint`
//! ([`super::specmint`]), which neither a window's end nor a start touches, and
//! a harness's folder holds launches' folders and nothing else (Devin's wire
//! logs are read by listing it).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::path;
use cf_proto::agents::Harness;

use crate::contract::LaunchId;
use crate::shared::record::find::entries;

/// The name Codex goes by under `integrations`. It keeps no files of a launch's
/// own (no role, and the Specmint skill of a chief given specs is kept for every
/// launch, [`super::specmint`]): its folder holds only what a build that kept the
/// skill per launch left, which a start's sweep clears.
pub(crate) const CODEX_FOLDER: &str = "codex";

/// The harness folders a launch's files are in, in the order Node goes through
/// them: a removal that fails stops there, leaving the folders after it. Codex's,
/// which Node had no files for, comes last.
const FOLDERS: [&str; 5] = ["claude", "pi", "devin", "opencode", CODEX_FOLDER];

/// The folder under `integrations` a harness keeps its launches' files in:
/// Codex is given what it runs with, and keeps no role there ([`CODEX_FOLDER`]).
pub(crate) fn harness_folder(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("claude"),
        Harness::Pi => Some("pi"),
        Harness::Devin => Some("devin"),
        Harness::Opencode => Some("opencode"),
        Harness::Codex => None,
    }
}

/// The folder of `launch`'s files in `harness_folder`, joined as
/// `path.join` joins it, or the failure of an environment that names no
/// ConsensFlow folder (Node read the process's own home there).
pub(crate) fn launch_folder(
    harness_folder: &str,
    env: &Env,
    launch: &LaunchId,
) -> Result<String, String> {
    integrations_path(env, &[harness_folder, launch.as_str()])
}

/// `parts` under ConsensFlow's `integrations`, joined as `path.join` joins
/// them, or the failure of an environment that names no ConsensFlow folder.
pub(crate) fn integrations_path(env: &Env, parts: &[&str]) -> Result<String, String> {
    let home = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;
    let home = home.to_string_lossy();
    let mut all = vec![&*home, "integrations"];
    all.extend(parts);
    Ok(path::join(&all))
}

fn folder(home: &str, harness_folder: &str, launch: &str) -> String {
    path::join(&[home, "integrations", harness_folder, launch])
}

/// A window closed: its files go (`forgetLaunch`), from every harness's
/// folder in `home`, ConsensFlow's folder.
pub fn forget_launch(home: &str, launch: &LaunchId) -> io::Result<()> {
    for harness in FOLDERS {
        remove(Path::new(&folder(home, harness, launch.as_str())))?;
    }
    Ok(())
}

/// At a start, when no window is open: every launch's folder in `home`
/// (`sweepLaunches`), in the order of the harnesses' folders, and nothing that is
/// not named by a launch id. A harness's folder that cannot be read holds none.
pub fn launches_left(home: &str) -> Vec<PathBuf> {
    let mut left = Vec::new();
    for harness in FOLDERS {
        let parent = path::join(&[home, "integrations", harness]);
        let Ok(names) = entries(Path::new(&parent)) else {
            continue;
        };
        left.extend(
            names
                .into_iter()
                .filter(|entry| LaunchId::new(&entry.file_name().to_string_lossy()).is_some())
                .map(|entry| entry.path()),
        );
    }
    left
}

/// The launches' folders a start found ([`launches_left`]) go, with all they hold,
/// once what was on its way is settled, which may read them (the prompt a Devin
/// launch was given behind `/bypass`): one after the other, a removal that fails
/// stopping there. How many went.
pub fn sweep_launches(left: &[PathBuf]) -> io::Result<usize> {
    for folder in left {
        remove(folder)?;
    }
    Ok(left.len())
}

/// `rmSync(path, { recursive: true, force: true })`: a folder with all it
/// holds, a file, or nothing, which is no failure.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(found) if found.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn go_when_their_window_closes_and_every_one_at_a_start() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().into_owned();
        let at = |relative: &str| dir.path().join(relative);
        for relative in [
            format!("integrations/claude/{A}"),
            format!("integrations/pi/{A}"),
            format!("integrations/opencode/{A}"),
            format!("integrations/codex/{A}"),
            format!("integrations/claude/{B}"),
            "integrations/claude/not-a-launch".to_owned(),
            "extensions/pi".to_owned(),
        ] {
            fs::create_dir_all(at(&relative)).unwrap();
            fs::write(at(&relative).join("file"), "x").unwrap();
        }
        forget_launch(&home, &LaunchId::new(A).unwrap()).unwrap();
        // An id that is not a launch id names nothing: no launch is made of it.
        assert!(LaunchId::new("..").is_none());
        for harness in ["claude", "pi", "opencode", "codex"] {
            assert!(
                !at(&format!("integrations/{harness}/{A}")).exists(),
                "{harness}"
            );
        }
        assert!(
            at(&format!("integrations/claude/{B}")).exists(),
            "another launch stays"
        );
        let left = launches_left(&home);
        assert_eq!(
            left,
            [at(&format!("integrations/claude/{B}"))],
            "B is left; nothing else is a launch"
        );
        // A window opened after the start found what was left has a folder of its own.
        fs::create_dir_all(at(&format!("integrations/pi/{A}"))).unwrap();
        assert_eq!(sweep_launches(&left).unwrap(), 1);
        assert!(!at(&format!("integrations/claude/{B}")).exists());
        assert!(
            at(&format!("integrations/pi/{A}")).exists(),
            "what was not left stays"
        );
        assert!(at("integrations/claude/not-a-launch").exists());
        assert!(at("extensions/pi").exists());
        let nowhere = path::join(&[&home, "nowhere"]);
        assert!(launches_left(&nowhere).is_empty());
    }
}
