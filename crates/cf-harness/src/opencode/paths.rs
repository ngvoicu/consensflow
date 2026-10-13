//! Where OpenCode keeps its store.

use cf_base::env::Env;
use cf_base::path;
use cf_proto::specs::Variant;

use crate::shared::paths::{agents_skills, config_home, data_home, home, set};
use crate::shared::specmint;

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

/// The `SKILL.md` of the Specmint variant the human installed for OpenCode, in
/// the places it reads the skills of the user's: `skills` and `skill` in its
/// own folder of the XDG config (`.config/opencode`), then the standards it
/// shares with Claude Code and the agents (`.claude/skills`, `.agents/skills`
/// in the home). None where it is in none of them.
pub(crate) fn global_specmint(env: &Env, variant: Variant) -> Option<String> {
    let mut roots = Vec::new();
    if let Ok(config) = config_home(env) {
        roots.extend(["skills", "skill"].map(|folder| path::join(&[&config, "opencode", folder])));
    }
    if let Ok(home) = home(env) {
        roots.push(path::join(&[&home, ".claude", "skills"]));
    }
    roots.extend(agents_skills(env).ok());
    specmint::installed(&roots, variant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_specmint_variant_the_human_installed_is_found_in_the_places_opencode_reads_in_its_order_and_nowhere_else(
    ) {
        use crate::shared::specmint::testing::is_held_to;
        is_held_to(
            &|env, variant| global_specmint(env, variant),
            &[],
            &[
                ".config/opencode/skills",
                ".config/opencode/skill",
                ".claude/skills",
                ".agents/skills",
            ],
            &[
                ".codex/skills",
                ".pi/agent/skills",
                ".config/devin/skills",
                ".config/opencode/plugins",
                ".local/share/opencode/skills",
            ],
        );
        // `XDG_CONFIG_HOME` moves OpenCode's own folder only.
        is_held_to(
            &|env, variant| global_specmint(env, variant),
            &[("XDG_CONFIG_HOME", "xdg")],
            &[
                "xdg/opencode/skills",
                "xdg/opencode/skill",
                ".claude/skills",
                ".agents/skills",
            ],
            &[".config/opencode/skills", ".config/opencode/skill"],
        );
    }

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
