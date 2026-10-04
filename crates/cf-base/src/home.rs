//! Where ConsensFlow keeps what it owns on this machine: one folder.

use std::path::PathBuf;

use crate::env::Env;
use crate::path;

/// ConsensFlow's folder: `CONSENSFLOW_HOME` when it is set to something, as
/// it is; else `.consensflow` in the user's own home (`HOME`; on Windows
/// `USERPROFILE` too), joined as Node's `path.join` joins it. None when
/// neither names a folder.
///
/// This is `configRoot` of `src/roster.js` (its `rosterHome`): everything
/// ConsensFlow owns lives under it, so a test that sets it owns the whole
/// machine's worth. Node returns `CONSENSFLOW_HOME` untouched, with no
/// normalization, and so does this.
pub fn config_root(env: &Env) -> Option<PathBuf> {
    // Node reads its environment as UTF-8, and a byte that is none as U+FFFD:
    // this reads the text Node's `path.join` is given.
    let text = |name: &str| {
        env.path(name)
            .map(|value| value.to_string_lossy().into_owned())
    };
    if let Some(root) = text("CONSENSFLOW_HOME") {
        return Some(PathBuf::from(root));
    }
    let home = text("HOME").or_else(|| text("USERPROFILE").filter(|_| cfg!(windows)))?;
    Some(PathBuf::from(path::join(&[&home, ".consensflow"])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder as text, which is what Node's `path.join` makes of it.
    fn text(root: Option<PathBuf>) -> Option<String> {
        root.map(|root| root.to_string_lossy().into_owned())
    }

    #[test]
    fn the_variable_wins_over_the_users_home() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", "/work/cf"), ("HOME", "/home/me")]);
        assert_eq!(text(config_root(&env)).as_deref(), Some("/work/cf"));
    }

    #[test]
    fn the_variable_is_the_folder_as_it_is_whatever_it_holds() {
        for value in ["/base/missing/..", r"C:..\cf", "C:", "./a/../b/", "~"] {
            let env = Env::from_vars([("CONSENSFLOW_HOME", value)]);
            assert_eq!(text(config_root(&env)).as_deref(), Some(value));
        }
    }

    #[test]
    fn without_it_the_folder_is_dot_consensflow_in_the_users_home() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", ""), ("HOME", "/home/me")]);
        let expected = if cfg!(windows) {
            r"\home\me\.consensflow"
        } else {
            "/home/me/.consensflow"
        };
        assert_eq!(text(config_root(&env)).as_deref(), Some(expected));
    }

    #[cfg(unix)]
    #[test]
    fn the_users_home_is_joined_as_node_joins_it() {
        let env = Env::from_vars([("HOME", "/home/me/..")]);
        assert_eq!(
            text(config_root(&env)).as_deref(),
            Some("/home/.consensflow")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_bare_drive_as_the_users_home_is_given_its_root() {
        let env = Env::from_vars([("USERPROFILE", "C:")]);
        assert_eq!(text(config_root(&env)).as_deref(), Some(r"C:\.consensflow"));
    }

    #[cfg(unix)]
    #[test]
    fn bytes_that_are_no_utf8_read_as_node_reads_them() {
        use std::os::unix::ffi::OsStrExt;
        let env = Env::from_vars([(
            "CONSENSFLOW_HOME",
            std::ffi::OsStr::from_bytes(b"/tmp/\xFF"),
        )]);
        assert_eq!(text(config_root(&env)).as_deref(), Some("/tmp/\u{FFFD}"));
    }

    #[test]
    fn with_no_home_at_all_there_is_no_folder() {
        assert_eq!(config_root(&Env::default()), None);
        let profile = Env::from_vars([("USERPROFILE", r"C:\Users\me")]);
        assert_eq!(config_root(&profile).is_some(), cfg!(windows));
    }
}
