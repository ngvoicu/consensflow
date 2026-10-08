//! Where Devin keeps its things.
//!
//! The home is `shared::paths::home`.

use cf_base::env::Env;
use cf_base::path;

use crate::shared::paths::{data_home, home};

/// Devin's store of its sessions: `cli/sessions.db` in its data folder.
pub(crate) fn store(env: &Env) -> Result<String, String> {
    Ok(path::join(&[&data(env)?, "cli", "sessions.db"]))
}

/// Devin's config folder, where its `config.json` is: on Windows its data
/// folder, and elsewhere `devin` in `XDG_CONFIG_HOME` (`.config` in the
/// home). A variable set empty is kept, as `??` kept it.
pub(crate) fn config(env: &Env) -> Result<String, String> {
    let base = if env.on_windows() {
        roaming(env)?
    } else if let Some(config) = env.os("XDG_CONFIG_HOME") {
        config.to_string_lossy().into_owned()
    } else {
        path::join(&[&home(env)?, ".config"])
    };
    Ok(path::join(&[&base, "devin"]))
}

/// Devin's data folder: `devin` in `%APPDATA%` on Windows (`AppData/Roaming`
/// in the home when it is not set), and elsewhere in `XDG_DATA_HOME`
/// (`.local/share` in the home). A variable set empty is kept, as `??`
/// kept it.
fn data(env: &Env) -> Result<String, String> {
    let base = if env.on_windows() {
        roaming(env)?
    } else {
        data_home(env)?
    };
    Ok(path::join(&[&base, "devin"]))
}

/// Where Windows keeps what an application keeps for its user: `%APPDATA%`,
/// else `AppData/Roaming` in the home.
fn roaming(env: &Env) -> Result<String, String> {
    match env.os("APPDATA") {
        Some(roaming) => Ok(roaming.to_string_lossy().into_owned()),
        None => Ok(path::join(&[&home(env)?, "AppData", "Roaming"])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_of(vars: &[(&str, &str)]) -> Result<String, String> {
        store(&Env::from_vars(vars.iter().copied()))
    }

    fn config_of(vars: &[(&str, &str)]) -> Result<String, String> {
        config(&Env::from_vars(vars.iter().copied()))
    }

    #[test]
    fn on_windows_the_config_is_under_appdata_else_the_homes_roaming_folder() {
        let windows = ("OS", "Windows_NT");
        assert_eq!(
            config_of(&[windows, ("APPDATA", "/r"), ("XDG_CONFIG_HOME", "/x")]).unwrap(),
            path::join(&["/r", "devin"])
        );
        assert_eq!(
            config_of(&[windows, ("HOME", "/h")]).unwrap(),
            path::join(&["/h", "AppData", "Roaming", "devin"])
        );
        assert_eq!(config_of(&[windows]).unwrap_err(), "missing home in env");
    }

    #[test]
    #[cfg(unix)]
    fn elsewhere_the_config_is_under_xdg_config_home_else_the_homes_dot_config_folder() {
        assert_eq!(
            config_of(&[("APPDATA", "/r"), ("XDG_CONFIG_HOME", "/x")]).unwrap(),
            "/x/devin"
        );
        assert_eq!(config_of(&[("HOME", "/h")]).unwrap(), "/h/.config/devin");
        // `??` keeps an empty variable: a folder under the working one.
        assert_eq!(
            config_of(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "")]).unwrap(),
            "devin"
        );
        assert_eq!(config_of(&[]).unwrap_err(), "missing home in env");
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
