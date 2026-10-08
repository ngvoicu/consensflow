//! What a window leaves in ConsensFlow's folder under its launch id: the
//! settings and integration files written for it, in
//! `integrations/<harness>/<launch>`. A launch id lives as long as its window,
//! and nothing else reads these folders, so they go when it does, and every one
//! at a start.

use std::fs;
use std::io;
use std::path::Path;

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::path;
use cf_proto::agents::Harness;

use crate::contract::LaunchId;
use crate::shared::record::find::entries;

/// The harness folders a launch's files are in, in the order Node goes through
/// them: a removal that fails stops there, leaving the folders after it.
const FOLDERS: [&str; 4] = ["claude", "pi", "devin", "opencode"];

/// The folder under `integrations` a harness keeps its launches' files in:
/// Codex is given what it runs with and keeps none.
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
    let home = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;
    Ok(folder(
        &home.to_string_lossy(),
        harness_folder,
        launch.as_str(),
    ))
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

/// At a start, when no window is open: every launch's files go
/// (`sweepLaunches`), and what is not named by a launch id stays. How many
/// went; a harness's folder that cannot be read holds none.
pub fn sweep_launches(home: &str) -> io::Result<usize> {
    let mut swept = 0;
    for harness in FOLDERS {
        let parent = path::join(&[home, "integrations", harness]);
        let Ok(names) = entries(Path::new(&parent)) else {
            continue;
        };
        for entry in names {
            if LaunchId::new(&entry.file_name().to_string_lossy()).is_some() {
                remove(&entry.path())?;
                swept += 1;
            }
        }
    }
    Ok(swept)
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
        for harness in ["claude", "pi", "opencode"] {
            assert!(
                !at(&format!("integrations/{harness}/{A}")).exists(),
                "{harness}"
            );
        }
        assert!(
            at(&format!("integrations/claude/{B}")).exists(),
            "another launch stays"
        );
        assert_eq!(
            sweep_launches(&home).unwrap(),
            1,
            "B went; nothing else is a launch"
        );
        assert!(at("integrations/claude/not-a-launch").exists());
        assert!(at("extensions/pi").exists());
        let nowhere = path::join(&[&home, "nowhere"]);
        assert_eq!(sweep_launches(&nowhere).unwrap(), 0);
    }
}
