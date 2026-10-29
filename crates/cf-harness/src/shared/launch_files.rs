//! What a window leaves in ConsensFlow's folder under its launch id: the
//! settings and integration files written for it, in
//! `integrations/<harness>/<launch>`. A launch id lives as long as its window,
//! and nothing else reads these folders, so they go when it does, and every one
//! at a start. What is kept from one launch to the next is in no harness's
//! folder: ConsensFlow's own copy of Specmint is in `integrations/specmint`
//! ([`super::specmint`]), which neither a window's end nor a start touches, and
//! a harness's folder holds launches' folders and nothing else (Devin's wire
//! logs are read by listing it). What a Devin launch proves of its first message is
//! kept when its folder goes, at a window's close as at a start, until a start has
//! settled what was on its way (`devin::kept`): a window's exit reads its record
//! once its files went, and a start once it swept them.
//!
//! Each folder goes on its own: one that cannot go (a file Windows holds open)
//! stops none after it, and is said with the others that could not (gap 5's P2 of
//! the Devin arrival gaps: one error left every later folder for good).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::path;
use cf_proto::agents::Harness;

use crate::contract::LaunchId;
use crate::devin::kept;
use crate::shared::record::find::entries;

/// The name Codex goes by under `integrations`. A launch's folder there holds the
/// window's role, the file its broker reads (`crate::codex`'s `role`); the Specmint
/// skill of a chief given specs is kept for every launch ([`super::specmint`]).
pub(crate) const CODEX_FOLDER: &str = "codex";

/// The harness folders a launch's files are in, in the order Node goes through
/// them. Codex's, which Node had no files for, comes last.
const FOLDERS: [&str; 5] = ["claude", "pi", "devin", "opencode", CODEX_FOLDER];

/// The folders a removal could not take, each with why: every other went.
#[derive(Debug)]
pub struct NotRemoved(Vec<(String, io::Error)>);

impl fmt::Display for NotRemoved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, (folder, error)) in self.0.iter().enumerate() {
            if at > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{folder}: {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for NotRemoved {}

/// What went wrong of removals each tried on its own: nothing, or the folders that stayed.
fn removed(failed: Vec<(String, io::Error)>) -> Result<(), NotRemoved> {
    if failed.is_empty() {
        Ok(())
    } else {
        Err(NotRemoved(failed))
    }
}

/// The folder under `integrations` a harness keeps its launches' role skills in:
/// Codex's role is no skill but a file of its own, which its module writes in
/// [`CODEX_FOLDER`].
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
/// folder in `home`, ConsensFlow's folder, each on its own.
pub fn forget_launch(home: &str, launch: &LaunchId) -> Result<(), NotRemoved> {
    let mut failed = Vec::new();
    for harness in FOLDERS {
        let folder = folder(home, harness, launch.as_str());
        if let Err(error) = remove_launch(home, harness, Path::new(&folder), launch.as_str()) {
            failed.push((folder, error));
        }
    }
    removed(failed)
}

/// The folder `folder` of the launch `name` in `harness`'s folder goes, what a Devin
/// launch proves of its first message kept first (`devin::kept`): a folder whose proof
/// cannot be kept stays.
fn remove_launch(home: &str, harness: &str, folder: &Path, name: &str) -> io::Result<()> {
    if harness == "devin" {
        let kept = Path::new(home).join("integrations").join(kept::FOLDER);
        // Said as the keep, and why, not as the removal it stopped (Calliope's P3 on T-697).
        kept::keep(folder, name, &kept).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "what proves its first message could not be kept in {}: {error}",
                    kept.display()
                ),
            )
        })?;
    }
    remove(folder)
}

/// At a start, when no window is open: every launch's files go
/// (`sweepLaunches`), each on its own ([`remove_launch`]), and what is not named
/// by a launch id stays. How many went; a harness's folder that cannot be read
/// holds none.
pub fn sweep_launches(home: &str) -> Result<usize, NotRemoved> {
    let mut swept = 0;
    let mut failed = Vec::new();
    for harness in FOLDERS {
        let parent = path::join(&[home, "integrations", harness]);
        let Ok(names) = entries(Path::new(&parent)) else {
            continue;
        };
        for entry in names {
            let name = entry.file_name().to_string_lossy().into_owned();
            if LaunchId::new(&name).is_none() {
                continue;
            }
            let launch = entry.path();
            match remove_launch(home, harness, &launch, &name) {
                Ok(()) => swept += 1,
                Err(error) => failed.push((launch.to_string_lossy().into_owned(), error)),
            }
        }
    }
    removed(failed).map(|()| swept)
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

    /// Gap 5 of the Devin arrival gaps: what a swept Devin launch proves of its first
    /// message is kept, outside the folders a start sweeps: the prompt it was given
    /// behind `/bypass`, and the sessions its log named. A launch given no such prompt,
    /// or whose log named no session, keeps nothing, and nor does another harness's.
    #[test]
    fn a_sweep_keeps_what_a_devin_launch_proves_of_its_first_message_behind_bypass() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().into_owned();
        let launch = |harness: &str, id: &str, prompt: Option<&str>, wire: &[&str]| {
            let folder = dir.path().join("integrations").join(harness).join(id);
            fs::create_dir_all(&folder).unwrap();
            if let Some(prompt) = prompt {
                fs::write(folder.join("prompt.txt"), prompt).unwrap();
            }
            let lines: String = wire.iter().map(|line| format!("{line}\n")).collect();
            fs::write(folder.join("wire.jsonl"), lines).unwrap();
        };
        let named = |session: &str| {
            format!(
                r#"{{"sessionId":"{session}","update":{{"sessionUpdate":"available_commands_update"}}}}"#
            )
        };
        let (one, two) = (named("calm-river"), named("still-lake"));
        launch(
            "devin",
            A,
            Some("/bypass [ConsensFlow m-2 · note]\nGo on."),
            &[&one, "{torn", &two, &one],
        );
        launch(
            "devin",
            B,
            Some("[ConsensFlow m-3 · note]\nNo command."),
            &[&one],
        );
        let c = "33333333-3333-4333-8333-333333333333";
        launch(
            "devin",
            c,
            Some("/bypass [ConsensFlow m-4 · note]\nNo session."),
            &[],
        );
        launch(
            "claude",
            c,
            Some("/bypass [ConsensFlow m-5 · note]\nNot Devin's."),
            &[&one],
        );
        assert_eq!(sweep_launches(&home).unwrap(), 4);
        let kept = dir.path().join("integrations").join(kept::FOLDER);
        let names: Vec<String> = fs::read_dir(&kept)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [format!("{A}.json")]);
        // A copy of a prompt is as private as the prompt (Calliope's P2 on T-697): the
        // folder its owner's alone, the file its owner's to read and write.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |at: &Path| fs::metadata(at).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&kept), 0o700, "the kept folder");
            assert_eq!(
                mode(&kept.join(format!("{A}.json"))),
                0o600,
                "the kept file"
            );
        }
        let said: serde_json::Value =
            serde_json::from_slice(&fs::read(kept.join(format!("{A}.json"))).unwrap()).unwrap();
        assert_eq!(
            said,
            serde_json::json!({
                "sessions": ["calm-river", "still-lake"],
                "prompt": "[ConsensFlow m-2 · note]\nGo on.",
            })
        );
        // A start that stopped before its settle sweeps nothing more, and keeps it.
        assert_eq!(sweep_launches(&home).unwrap(), 0);
        assert!(kept.join(format!("{A}.json")).exists());
        assert!(crate::devin::forget_kept(&home).is_empty());
        assert_eq!(fs::read_dir(&kept).unwrap().count(), 0);
        // A window's close keeps it as a start does: its exit reads its record after.
        launch(
            "devin",
            B,
            Some("/bypass [ConsensFlow m-6 · note]\nHandoff."),
            &[&two],
        );
        forget_launch(&home, &LaunchId::new(B).unwrap()).unwrap();
        assert!(!dir.path().join("integrations/devin").join(B).exists());
        let said: serde_json::Value =
            serde_json::from_slice(&fs::read(kept.join(format!("{B}.json"))).unwrap()).unwrap();
        assert_eq!(said["sessions"], serde_json::json!(["still-lake"]));
    }

    /// Gap 5's P2 (both reviewers of B5's round 12): one folder that could not go stopped
    /// the sweep there, and every later one stayed. Each goes on its own now, and the one
    /// that stays is said.
    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_go_stops_no_other() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().into_owned();
        for relative in [
            format!("integrations/claude/{A}/inner"),
            format!("integrations/pi/{A}"),
            format!("integrations/devin/{B}"),
            format!("integrations/opencode/{A}"),
        ] {
            fs::create_dir_all(dir.path().join(&relative)).unwrap();
            fs::write(dir.path().join(&relative).join("file"), "x").unwrap();
        }
        // Claude's launch holds a folder nothing can be removed from.
        let stuck = dir.path().join(format!("integrations/claude/{A}/inner"));
        fs::set_permissions(&stuck, fs::Permissions::from_mode(0o555)).unwrap();
        let swept = sweep_launches(&home);
        let forgot = forget_launch(&home, &LaunchId::new(A).unwrap());
        fs::set_permissions(&stuck, fs::Permissions::from_mode(0o755)).unwrap();
        let failed = swept.unwrap_err().to_string();
        assert!(failed.contains(&format!("claude/{A}")), "{failed}");
        assert!(!failed.contains("devin"), "{failed}");
        for gone in [
            format!("integrations/pi/{A}"),
            format!("integrations/devin/{B}"),
            format!("integrations/opencode/{A}"),
        ] {
            assert!(!dir.path().join(&gone).exists(), "{gone} went");
        }
        assert!(forgot.is_err(), "the window's close says it too");
        // Once it can go, the next sweep takes it.
        assert_eq!(sweep_launches(&home).unwrap(), 1);
    }

    /// Calliope's P3 on T-697: a keep that fails leaves its launch's folder, and the
    /// start's failure says it was the keep, and why, not only that the folder stayed.
    #[cfg(unix)]
    #[test]
    fn a_keep_that_fails_is_said_as_the_keep() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().into_owned();
        let launch = dir.path().join("integrations/devin").join(A);
        fs::create_dir_all(&launch).unwrap();
        fs::write(
            launch.join("prompt.txt"),
            "/bypass [ConsensFlow m-2 · note]\nGo on.",
        )
        .unwrap();
        fs::write(
            launch.join("wire.jsonl"),
            "{\"sessionId\":\"calm-river\",\"update\":{}}\n",
        )
        .unwrap();
        let kept = dir.path().join("integrations").join(kept::FOLDER);
        fs::create_dir_all(&kept).unwrap();
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o500)).unwrap();
        let swept = sweep_launches(&home);
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o700)).unwrap();
        let failed = swept.unwrap_err().to_string();
        assert!(
            failed.contains("its first message could not be kept")
                && failed.contains(&kept.to_string_lossy().into_owned())
                && failed.contains("ermission denied"),
            "{failed}"
        );
        assert!(launch.exists(), "the folder stays with what it proves");
        assert_eq!(
            sweep_launches(&home).unwrap(),
            1,
            "and goes once it can be kept"
        );
    }
}
