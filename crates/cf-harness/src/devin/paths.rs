//! Where Devin keeps its things (`devinFolders`, `src/harnesses.js`, and
//! `readStore`, `hosts/lib/completion/devin.js`).
//!
//! The home is `shared::paths::home`, `src/harnesses.js`'s.

use cf_base::env::Env;
use cf_base::path;

use crate::shared::paths::home;

/// Devin's store of its sessions: `cli/sessions.db` in its data folder.
pub(crate) fn store(env: &Env) -> Result<String, String> {
    Ok(path::join(&[&data(env)?, "cli", "sessions.db"]))
}

/// Devin's data folder: `devin` in `%APPDATA%` on Windows (`AppData/Roaming`
/// in the home when it is not set), and elsewhere in `XDG_DATA_HOME`
/// (`.local/share` in the home). A variable set empty is kept, as `??`
/// kept it.
fn data(env: &Env) -> Result<String, String> {
    let (variable, under_home) = if env.on_windows() {
        ("APPDATA", ["AppData", "Roaming"])
    } else {
        ("XDG_DATA_HOME", [".local", "share"])
    };
    let base = match env.os(variable) {
        Some(base) => base.to_string_lossy().into_owned(),
        None => {
            let home = home(env)?;
            path::join(&[&home, under_home[0], under_home[1]])
        }
    };
    Ok(path::join(&[&base, "devin"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_of(vars: &[(&str, &str)]) -> Result<String, String> {
        store(&Env::from_vars(vars.iter().copied()))
    }

    #[test]
    fn on_windows_the_store_is_under_appdata_else_the_home_s_roaming_folder() {
        let windows = ("OS", "Windows_NT");
        assert_eq!(
            store_of(&[windows, ("APPDATA", "/r"), ("XDG_DATA_HOME", "/x")]).unwrap(),
            path::join(&["/r", "devin", "cli", "sessions.db"])
        );
        assert_eq!(
            store_of(&[windows, ("HOME", "/h")]).unwrap(),
            path::join(&["/h", "AppData", "Roaming", "devin", "cli", "sessions.db"])
        );
        assert_eq!(
            store_of(&[windows, ("USERPROFILE", "/u")]).unwrap(),
            path::join(&["/u", "AppData", "Roaming", "devin", "cli", "sessions.db"])
        );
        assert_eq!(store_of(&[windows]).unwrap_err(), "missing home in env");
    }

    #[test]
    #[cfg(unix)]
    fn elsewhere_the_store_is_under_xdg_data_home_else_the_home_s_share_folder() {
        assert_eq!(
            store_of(&[("APPDATA", "/r"), ("XDG_DATA_HOME", "/x")]).unwrap(),
            "/x/devin/cli/sessions.db"
        );
        assert_eq!(
            store_of(&[("HOME", "/h")]).unwrap(),
            "/h/.local/share/devin/cli/sessions.db"
        );
        // `??` keeps an empty variable: a folder under the working one.
        assert_eq!(
            store_of(&[("HOME", "/h"), ("XDG_DATA_HOME", "")]).unwrap(),
            "devin/cli/sessions.db"
        );
    }
}
