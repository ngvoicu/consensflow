//! The environment as the process found it, read once in `main` and handed
//! to whatever needs a variable, so a test hands it any environment it likes.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// A process's environment variables.
#[derive(Debug, Clone, Default)]
pub struct Env {
    vars: BTreeMap<OsString, OsString>,
}

impl Env {
    /// The environment this process was started with.
    #[allow(clippy::disallowed_methods)] // The one place the environment is read.
    pub fn from_process() -> Self {
        Self::from_vars(std::env::vars_os())
    }

    /// An environment of these variables.
    pub fn from_vars<K: Into<OsString>, V: Into<OsString>>(
        vars: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            vars: vars
                .into_iter()
                .map(|(name, value)| (key(name.into()), value.into()))
                .collect(),
        }
    }

    /// The variable's value as the system gave it.
    pub fn os(&self, name: &str) -> Option<&OsStr> {
        self.vars.get(&key(name.into())).map(OsString::as_os_str)
    }

    /// The variable's value as a path, when it is set to something.
    pub fn path(&self, name: &str) -> Option<&Path> {
        self.os(name)
            .filter(|value| !value.is_empty())
            .map(Path::new)
    }

    /// The variable's value when it is set to some text: what JavaScript's
    /// `if (env.NAME)` took for set. A value that is no UTF-8 is not text.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.os(name)
            .and_then(OsStr::to_str)
            .filter(|value| !value.is_empty())
    }

    /// Every variable, by name in order: what `Object.keys(process.env)` listed.
    /// Windows names are in upper case, as `os` and `text` look them up.
    pub fn iter(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.vars
            .iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }
}

/// A variable's name as the system compares names: on Windows, in any case.
fn key(name: OsString) -> OsString {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_set_to_nothing_is_no_text() {
        let env = Env::from_vars([
            ("CONSENSFLOW_TOKEN", ""),
            ("CONSENSFLOW_URL", "http://127.0.0.1:1"),
        ]);
        assert_eq!(env.text("CONSENSFLOW_TOKEN"), None);
        assert_eq!(env.os("CONSENSFLOW_TOKEN"), Some(OsStr::new("")));
        assert_eq!(env.text("CONSENSFLOW_URL"), Some("http://127.0.0.1:1"));
        assert_eq!(env.text("CONSENSFLOW_NODE"), None);
    }

    #[test]
    fn lists_every_variable_in_order_with_its_value() {
        let env = Env::from_vars([
            ("CONSENSFLOW_URL", "u"),
            ("A", "1"),
            ("CONSENSFLOW_HOME", ""),
        ]);
        let listed: Vec<_> = env.iter().collect();
        assert_eq!(
            listed,
            [
                (OsStr::new("A"), OsStr::new("1")),
                (OsStr::new("CONSENSFLOW_HOME"), OsStr::new("")),
                (OsStr::new("CONSENSFLOW_URL"), OsStr::new("u")),
            ]
        );
    }

    #[test]
    fn names_match_in_any_case_on_windows_only() {
        let env = Env::from_vars([("Path", "/bin")]);
        assert_eq!(env.text("PATH").is_some(), cfg!(windows));
        assert_eq!(env.text("Path"), Some("/bin"));
    }
}
