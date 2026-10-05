//! Where OpenCode keeps its store (`opencodeStores`, `src/harnesses.js`).

use cf_base::env::Env;
use cf_base::path;

use crate::shared::paths::{data_home, set};

/// Where OpenCode's store may be, likeliest first: `OPENCODE_DB`, or
/// `opencode.db` in `OPENCODE_DATA`, when set (OpenCode honours both
/// everywhere); else its XDG place, and on Windows `opencode` in
/// `%LOCALAPPDATA%` and `%APPDATA%` after it, where some versions keep it.
pub(crate) fn stores(env: &Env) -> Result<Vec<String>, String> {
    if let Some(store) = set(env, "OPENCODE_DB") {
        return Ok(vec![store]);
    }
    if let Some(data) = set(env, "OPENCODE_DATA") {
        return Ok(vec![path::join(&[&data, "opencode.db"])]);
    }
    let xdg = path::join(&[&data_home(env)?, "opencode", "opencode.db"]);
    if !env.on_windows() {
        return Ok(vec![xdg]);
    }
    let windows = ["LOCALAPPDATA", "APPDATA"]
        .into_iter()
        .filter_map(|name| set(env, name))
        .map(|folder| path::join(&[&folder, "opencode", "opencode.db"]));
    Ok(std::iter::once(xdg).chain(windows).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores_of(vars: &[(&str, &str)]) -> Result<Vec<String>, String> {
        stores(&Env::from_vars(vars.iter().copied()))
    }

    #[test]
    fn a_store_opencode_is_told_of_is_the_only_one() {
        let everything = [
            ("OPENCODE_DB", "/db/one.db"),
            ("OPENCODE_DATA", "/data"),
            ("XDG_DATA_HOME", "/x"),
        ];
        assert_eq!(stores_of(&everything).unwrap(), ["/db/one.db"]);
        assert_eq!(
            stores_of(&everything[1..]).unwrap(),
            [path::join(&["/data", "opencode.db"])]
        );
        // Set to nothing is not set.
        assert_eq!(
            stores_of(&[
                ("OPENCODE_DB", ""),
                ("OPENCODE_DATA", ""),
                ("XDG_DATA_HOME", "/x")
            ])
            .unwrap(),
            [path::join(&["/x", "opencode", "opencode.db"])]
        );
    }

    #[test]
    fn on_windows_its_local_and_roaming_folders_follow_the_xdg_place() {
        let windows = ("OS", "Windows_NT");
        assert_eq!(
            stores_of(&[
                windows,
                ("HOME", "/h"),
                ("LOCALAPPDATA", "/l"),
                ("APPDATA", "/r")
            ])
            .unwrap(),
            [
                path::join(&["/h", ".local", "share", "opencode", "opencode.db"]),
                path::join(&["/l", "opencode", "opencode.db"]),
                path::join(&["/r", "opencode", "opencode.db"]),
            ]
        );
        assert_eq!(
            stores_of(&[windows, ("XDG_DATA_HOME", "/x"), ("APPDATA", "")]).unwrap(),
            [path::join(&["/x", "opencode", "opencode.db"])]
        );
        assert_eq!(stores_of(&[windows]).unwrap_err(), "missing home in env");
    }
}
