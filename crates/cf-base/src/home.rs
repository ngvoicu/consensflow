//! Where ConsensFlow keeps what it owns on this machine: one folder.

use std::path::PathBuf;

use crate::env::Env;
use crate::path;

/// ConsensFlow's folder: `CONSENSFLOW_HOME` when it is set to something, as
/// it is; else `.consensflow` in the user's own home (`HOME`; on Windows
/// `USERPROFILE` too), joined as Node's `path.join` joins it. None when
/// neither names a folder.
///
/// This is the root of the home: everything ConsensFlow owns lives under it, so
/// a test that sets it owns the whole machine's worth. Node returns
/// `CONSENSFLOW_HOME` untouched, with no normalization, and so does this.
pub fn config_root(env: &Env) -> Option<PathBuf> {
    // Node reads its environment as UTF-8, and a byte that is none as U+FFFD:
    // this reads the text Node's `path.join` is given.
    if let Some(root) = named(env, "CONSENSFLOW_HOME") {
        return Some(PathBuf::from(root));
    }
    default_root(env)
}

/// The folder ConsensFlow's home is when nothing names another: `.consensflow`
/// in the user's own home, whatever `CONSENSFLOW_HOME` says. The home a
/// launcher with no pin talks to (`cf-launcher`'s repair asks which homes it
/// may touch). None when there is no user's home either.
pub fn default_root(env: &Env) -> Option<PathBuf> {
    let home =
        named(env, "HOME").or_else(|| named(env, "USERPROFILE").filter(|_| cfg!(windows)))?;
    Some(PathBuf::from(path::join(&[&home, ".consensflow"])))
}

/// The variable as text when it is set to something, a byte that is no UTF-8
/// as U+FFFD.
fn named(env: &Env, name: &str) -> Option<String> {
    env.path(name)
        .map(|value| value.to_string_lossy().into_owned())
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

    #[test]
    fn the_default_folder_is_the_users_whatever_the_variable_says() {
        let env = Env::from_vars([("CONSENSFLOW_HOME", "/work/cf"), ("HOME", "/home/me")]);
        let expected = if cfg!(windows) {
            r"\home\me\.consensflow"
        } else {
            "/home/me/.consensflow"
        };
        assert_eq!(text(default_root(&env)).as_deref(), Some(expected));
        // With no user's home there is no default, though the variable names a folder.
        let variable = Env::from_vars([("CONSENSFLOW_HOME", "/work/cf")]);
        assert_eq!(default_root(&variable), None);
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
