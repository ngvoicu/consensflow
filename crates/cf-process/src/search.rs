//! Where a command is on this machine's PATH, as Windows or Unix looks for
//! one, and whether to look as Windows does: on Windows itself, or wherever
//! the environment says it is Windows's (the tests of a Windows layout).

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

/// The extensions Windows can start a program by; a PATHEXT naming another
/// (`.JS`, `.PS1`) names nothing a process can start.
const STARTABLE: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

/// Whether `env` is Windows's.
pub fn on_windows(env: &Env) -> bool {
    cfg!(windows)
        || env
            .text("OS")
            .is_some_and(|os| os.to_lowercase().contains("windows"))
}

/// The names `command` may have on disk: with each startable PATHEXT
/// extension on Windows, as it is elsewhere.
fn candidate_names(command: &str, env: &Env) -> Vec<String> {
    if !on_windows(env) {
        return vec![command.to_string()];
    }
    env.text("PATHEXT")
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .map(str::to_lowercase)
        .filter(|extension| STARTABLE.contains(&extension.as_str()))
        .map(|extension| format!("{command}{extension}"))
        .collect()
}

/// Where `command` resolves on `env`'s PATH, as an absolute path: the pane
/// host refuses a relative one, and a PATH may hold relative folders.
pub fn on_path(command: &str, env: &Env) -> Option<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let path = env.os("PATH")?.to_string_lossy().into_owned();
    let names = candidate_names(command, env);
    path.split(separator)
        .filter(|folder| !folder.is_empty())
        .flat_map(|folder| names.iter().map(move |name| Path::new(folder).join(name)))
        .find(|candidate| startable(candidate))
        .and_then(|found| std::path::absolute(found).ok())
}

/// Whether `file` is a file this user can start: on Unix, one with an execute bit.
fn startable(file: &Path) -> bool {
    let Ok(metadata) = fs::metadata(file) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_as_windows_does_on_windows_or_when_the_environment_says_so() {
        assert!(on_windows(&Env::from_vars([("OS", "Windows_NT")])));
        assert_eq!(on_windows(&Env::default()), cfg!(windows));
        let windows = Env::from_vars([("OS", "Windows_NT"), ("PATHEXT", ".JS;.EXE;.Cmd;.PS1")]);
        assert_eq!(candidate_names("node", &windows), ["node.exe", "node.cmd"]);
        let default = Env::from_vars([("OS", "Windows_NT")]);
        assert_eq!(
            candidate_names("node", &default),
            ["node.com", "node.exe", "node.bat", "node.cmd"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn finds_the_first_startable_one_on_path_as_an_absolute_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (plain, runs) = (dir.path().join("plain"), dir.path().join("runs"));
        fs::create_dir_all(&plain).unwrap();
        fs::create_dir_all(&runs).unwrap();
        fs::write(plain.join("node"), "").unwrap();
        fs::write(runs.join("node"), "").unwrap();
        fs::set_permissions(runs.join("node"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}::{}", plain.display(), runs.display());
        let env = Env::from_vars([("PATH", path.as_str())]);
        assert_eq!(on_path("node", &env), Some(runs.join("node")));
        assert_eq!(on_path("codex", &env), None);
        assert_eq!(on_path("node", &Env::default()), None);
    }
}
