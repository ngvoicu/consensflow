//! Where ConsensFlow keeps what it owns on this machine: one folder.

use std::path::{Path, PathBuf};

use crate::env::Env;

/// ConsensFlow's folder: `CONSENSFLOW_HOME` when it is set to something,
/// else `.consensflow` in the user's own home (`HOME`; on Windows
/// `USERPROFILE` too). None when neither names a folder.
///
/// This is `configRoot` of `src/roster.js`: everything ConsensFlow owns
/// lives under it, so a test that sets it owns the whole machine's worth.
pub fn config_root(env: &Env) -> Option<PathBuf> {
    if let Some(root) = env.path("CONSENSFLOW_HOME") {
        return Some(root.to_path_buf());
    }
    let user = env.path("HOME").or_else(|| {
        env.path("USERPROFILE")
            .filter(|_| cfg!(windows))
            .map(Path::new)
    });
    user.map(|home| home.join(".consensflow"))
}

/// `path` as Node's `path.join` leaves what it joins, by the text alone: each
/// `.` gone, and each `..` taking off the name before it, whether or not
/// that folder is there. A `..` at the root stays at the root; one at the
/// start of a relative path stays.
pub fn normalized(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return PathBuf::from(".");
    }
    parts.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_normalized_as_node_joins_it() {
        let root = std::env::temp_dir();
        assert_eq!(
            normalized(&root.join("base").join("missing").join("..").join("agents.json")),
            root.join("base").join("agents.json")
        );
        assert_eq!(normalized(&root.join(".").join("a")), root.join("a"));
        assert_eq!(normalized(Path::new("../a/./b/..")), Path::new("..").join("a"));
        assert_eq!(normalized(Path::new("a/..")), Path::new("."));
        let top = root.ancestors().last().unwrap().to_path_buf();
        assert_eq!(normalized(&top.join("..").join("x")), top.join("x"));
    }

    #[test]
    fn the_variable_wins_over_the_users_home() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", "/work/cf"), ("HOME", "/home/me")]);
        assert_eq!(config_root(&env), Some(PathBuf::from("/work/cf")));
    }

    #[test]
    fn without_it_the_folder_is_dot_consensflow_in_the_users_home() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", ""), ("HOME", "/home/me")]);
        assert_eq!(
            config_root(&env),
            Some(Path::new("/home/me").join(".consensflow"))
        );
    }

    #[test]
    fn with_no_home_at_all_there_is_no_folder() {
        assert_eq!(config_root(&Env::default()), None);
        let profile = Env::from_vars([("USERPROFILE", r"C:\Users\me")]);
        assert_eq!(config_root(&profile).is_some(), cfg!(windows));
    }
}
