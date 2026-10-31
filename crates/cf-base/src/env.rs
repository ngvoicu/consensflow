//! The environment as the process found it, read once in `main` and handed
//! to whatever needs a variable, so a test hands it any environment it likes;
//! and the machine's own folders of programs, which a test has none of unless
//! it names them. What runs, runs on the PATH with those folders after it
//! ([`Env::with_tool_folders_on_path`]), and finds there what it needs; a
//! harness's CLI is looked for on the PATH as it was given
//! ([`Env::search_path`]), so a Homebrew one is never the first choice over
//! the human's own and always the last resort.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The variable that names the system's folders of programs in place of the
/// machine's own (see [`Env::tool_folders`]), as a PATH names its folders;
/// set to nothing, it names none.
pub const TOOL_FOLDERS: &str = "CONSENSFLOW_TOOL_FOLDERS";

/// The variable the app gives its daemon naming, as a JSON list of names (never their
/// values), what it took of the environment a terminal's shell gives a program: the
/// human's interactive login shell on macOS, their PowerShell profile on Windows. The
/// daemon has the values themselves, and gives each window's pane the named ones, which
/// the app's own terminal would not have otherwise (T-540).
pub const TERMINAL_ENV: &str = "CONSENSFLOW_TERMINAL_ENV";

/// The folders a package manager of the system puts its programs in, which a
/// login shell puts on the PATH and an app the Finder started is not given:
/// on a Mac, Homebrew's (`/opt/homebrew`, and `/usr/local` on Intel, which the
/// installers of other programs use too). Elsewhere none: an app started from
/// the Start menu has the PATH of the user and the machine, which installers
/// write to, and a Linux desktop session (no app is built for one) has its
/// login profile's.
const MACHINE_TOOL_FOLDERS: &[&str] = if cfg!(target_os = "macos") {
    &["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]
} else {
    &[]
};

/// What separates the folders of a PATH.
const DELIMITER: &str = if cfg!(windows) { ";" } else { ":" };

/// A process's environment variables, and the system's folders of programs.
#[derive(Debug, Clone, Default)]
pub struct Env {
    vars: BTreeMap<OsString, OsString>,
    tool_folders: Vec<PathBuf>,
    /// The PATH it was given, before the system's folders were put after it.
    search_path: Option<OsString>,
}

impl Env {
    /// The environment this process was started with, on this machine: its
    /// folders of programs are the ones [`TOOL_FOLDERS`] names, else the
    /// machine's own.
    #[allow(clippy::disallowed_methods)] // The one place the environment is read.
    pub fn from_process() -> Self {
        Self::from_vars(std::env::vars_os()).on_this_machine()
    }

    /// This environment on this machine: with the machine's own folders of
    /// programs, unless it names some.
    fn on_this_machine(mut self) -> Self {
        if self.os(TOOL_FOLDERS).is_none() {
            self.tool_folders = MACHINE_TOOL_FOLDERS.iter().map(PathBuf::from).collect();
        }
        self
    }

    /// An environment of these variables, on no machine: its folders of
    /// programs are the ones [`TOOL_FOLDERS`] names, and none otherwise.
    pub fn from_vars<K: Into<OsString>, V: Into<OsString>>(
        vars: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let vars: BTreeMap<OsString, OsString> = vars
            .into_iter()
            .map(|(name, value)| (key(name.into()), value.into()))
            .collect();
        let tool_folders = vars
            .get(&key(TOOL_FOLDERS.into()))
            .map(|named| {
                std::env::split_paths(named)
                    .filter(|folder| !folder.as_os_str().is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let search_path = vars.get(&key("PATH".into())).cloned();
        Self {
            vars,
            tool_folders,
            search_path,
        }
    }

    /// The system's own folders of programs, which a login shell puts on the
    /// PATH and an app the Finder started is not given: where a harness's CLI
    /// is looked for last, and what the daemon puts after the PATH of
    /// everything it runs.
    pub fn tool_folders(&self) -> &[PathBuf] {
        &self.tool_folders
    }

    /// This environment as the daemon runs programs on it: its PATH with each of
    /// the system's folders that is there and not on it after it, so a window
    /// has Homebrew's `git` and `node` when the login shell gave nothing, and so
    /// does the npm shim of a harness that runs on that Node. A harness's CLI is
    /// still looked for on the PATH it was given ([`Env::search_path`]): one the
    /// human keeps in the home is found before an older one of Homebrew's.
    #[must_use]
    pub fn with_tool_folders_on_path(mut self) -> Self {
        let path = path_with_folders(self.os("PATH").unwrap_or_default(), &self.tool_folders);
        if !path.is_empty() {
            self.vars.insert(key("PATH".into()), path);
        }
        self
    }

    /// This environment with the variable `name` set to `value`, whatever it held:
    /// what a program is run with that it is not given otherwise. Its PATH as given
    /// ([`Env::search_path`]) stays the one it was given.
    #[must_use]
    pub fn with_var(mut self, name: &str, value: &str) -> Self {
        self.vars.insert(key(name.into()), value.into());
        self
    }

    /// The PATH a harness's CLI is looked for on first: the one this
    /// environment was given, before the system's folders were put after it.
    pub fn search_path(&self) -> Option<&OsStr> {
        self.search_path.as_deref()
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

    /// Whether this is Windows's environment: built for Windows, or saying so
    /// in its `OS` variable, read as Node read it, a byte that is no UTF-8 as
    /// U+FFFD.
    pub fn on_windows(&self) -> bool {
        cfg!(windows)
            || self
                .os("OS")
                .is_some_and(|os| os.to_string_lossy().to_lowercase().contains("windows"))
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

/// `path` as it is, and then each of `folders` that is there and not on it
/// already, written as a PATH writes a folder: Windows quotes one whose name
/// holds a `;`, and one no PATH can name (a `:` in it elsewhere, a `"` on
/// Windows) is left out, never put on as two folders that are not there.
pub fn path_with_folders(path: &OsStr, folders: &[PathBuf]) -> OsString {
    let mut on_it: Vec<PathBuf> = std::env::split_paths(path).collect();
    let mut joined = path.to_os_string();
    for folder in folders {
        if !folder.is_dir() || on_it.contains(folder) {
            continue;
        }
        let Ok(written) = std::env::join_paths([folder]) else {
            continue;
        };
        if !joined.is_empty() {
            joined.push(DELIMITER);
        }
        joined.push(written);
        on_it.push(folder.clone());
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_set_on_an_environment_is_its_whatever_it_held_and_the_rest_stays() {
        let given = Env::from_vars([("A", "1"), ("PATH", "/bin")]);
        let set = given.with_var("A", "2").with_var("B", "3");
        assert_eq!(set.text("A"), Some("2"));
        assert_eq!(set.text("B"), Some("3"));
        assert_eq!(set.text("PATH"), Some("/bin"));
        assert_eq!(set.search_path(), Some(OsStr::new("/bin")));
    }

    #[test]
    fn is_windows_when_built_for_it_or_when_the_environment_says_so() {
        assert!(Env::from_vars([("OS", "Windows_NT")]).on_windows());
        assert!(Env::from_vars([("OS", "MY WINDOWS")]).on_windows());
        assert_eq!(Env::default().on_windows(), cfg!(windows));
        assert_eq!(Env::from_vars([("OS", "")]).on_windows(), cfg!(windows));
        assert_eq!(
            Env::from_vars([("OS", "Darwin")]).on_windows(),
            cfg!(windows)
        );
    }

    #[test]
    #[cfg(unix)]
    fn an_os_that_is_no_utf8_still_says_windows() {
        use std::os::unix::ffi::OsStringExt;
        let os = OsString::from_vec(b"\xffWindows_NT".to_vec());
        assert!(Env::from_vars([(OsString::from("OS"), os)]).on_windows());
    }

    #[test]
    fn a_variable_set_to_nothing_is_no_text() {
        let env = Env::from_vars([
            ("CONSENSFLOW_TOKEN", ""),
            ("CONSENSFLOW_URL", "http://127.0.0.1:1"),
        ]);
        assert_eq!(env.text("CONSENSFLOW_TOKEN"), None);
        assert_eq!(env.os("CONSENSFLOW_TOKEN"), Some(OsStr::new("")));
        assert_eq!(env.text("CONSENSFLOW_URL"), Some("http://127.0.0.1:1"));
        assert_eq!(env.text("CONSENSFLOW_PROJECT"), None);
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
    fn the_folders_of_programs_are_the_ones_named_else_the_machines_for_its_own_and_none_for_a_tests(
    ) {
        let delimiter = if cfg!(windows) { ";" } else { ":" };
        let named = Env::from_vars([(TOOL_FOLDERS, format!("/a{delimiter}{delimiter}/b"))]);
        let both = [PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(named.tool_folders(), both, "an empty one is none");
        assert_eq!(named.on_this_machine().tool_folders(), both);
        // Named as nothing: none, on this machine too.
        let nothing = Env::from_vars([(TOOL_FOLDERS, "")]);
        assert!(nothing.tool_folders().is_empty());
        assert!(nothing.on_this_machine().tool_folders().is_empty());
        // Named nowhere: none for an environment a test makes, the machine's for its own.
        let unnamed = Env::from_vars([("PATH", "/usr/bin")]);
        assert!(unnamed.tool_folders().is_empty());
        assert!(Env::default().tool_folders().is_empty());
        let machine: Vec<PathBuf> = if cfg!(target_os = "macos") {
            ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]
                .map(PathBuf::from)
                .into()
        } else {
            Vec::new()
        };
        assert_eq!(unnamed.on_this_machine().tool_folders(), machine);
    }

    /// Folders `names` made under `root`, each there.
    fn made(root: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let folder = root.join(name);
                std::fs::create_dir(&folder).unwrap();
                folder
            })
            .collect()
    }

    #[test]
    fn programs_run_on_the_path_with_the_systems_folders_after_it_and_are_looked_for_on_the_one_given(
    ) {
        let machine = tempfile::tempdir().unwrap();
        let [own, homebrew]: [PathBuf; 2] = made(machine.path(), &["own", "homebrew"])
            .try_into()
            .unwrap();
        let given = own.clone().into_os_string();
        let folders = std::env::join_paths([&homebrew]).unwrap();
        let env = Env::from_vars([
            (OsString::from("PATH"), given.clone()),
            (OsString::from(TOOL_FOLDERS), folders),
        ]);
        assert_eq!(env.search_path(), Some(given.as_os_str()));
        let running = env.with_tool_folders_on_path();
        assert_eq!(
            running.os("PATH"),
            Some(std::env::join_paths([&own, &homebrew]).unwrap().as_os_str())
        );
        assert_eq!(running.search_path(), Some(given.as_os_str()));
        // With no PATH and no folders of the system's, still none.
        let bare = Env::from_vars([("HOME", "/home")]).with_tool_folders_on_path();
        assert_eq!((bare.os("PATH"), bare.search_path()), (None, None));
    }

    #[test]
    fn the_systems_folders_go_after_the_path_each_once_and_only_where_they_are() {
        let machine = tempfile::tempdir().unwrap();
        let [own, first, second]: [PathBuf; 3] = made(machine.path(), &["own", "first", "second"])
            .try_into()
            .unwrap();
        let missing = machine.path().join("missing");
        let path = std::env::join_paths([&own, &first]).unwrap();
        let folders = [missing, first.clone(), second.clone(), second.clone()];
        assert_eq!(
            path_with_folders(&path, &folders),
            std::env::join_paths([&own, &first, &second]).unwrap()
        );
        assert_eq!(
            path_with_folders(OsStr::new(""), std::slice::from_ref(&second)),
            second.clone().into_os_string()
        );
        assert_eq!(path_with_folders(&path, &[]), path);
    }

    /// A folder whose name holds the PATH's delimiter is put on it as a PATH
    /// writes such a folder (Windows quotes it), or not at all where none can
    /// (`:` elsewhere): never as two folders that are not there. And the PATH
    /// given keeps its own such folder as it wrote it (the reviews of 2026-10-16).
    #[test]
    fn a_folder_whose_name_holds_the_delimiter_is_put_on_the_path_whole_or_not_at_all() {
        let machine = tempfile::tempdir().unwrap();
        let delimited = if cfg!(windows) {
            "tools;pinned"
        } else {
            "tools:pinned"
        };
        let [own, odd, other]: [PathBuf; 3] = made(machine.path(), &["own", delimited, "other"])
            .try_into()
            .unwrap();
        let path = own.clone().into_os_string();
        let joined = path_with_folders(&path, std::slice::from_ref(&odd));
        let folders: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        if cfg!(windows) {
            assert_eq!(folders, [own.clone(), odd.clone()]);
            // A PATH that has it, quoted, has it once, and the next one after it.
            let quoted = std::env::join_paths([&odd]).unwrap();
            let joined = path_with_folders(&quoted, &[odd.clone(), other.clone()]);
            assert_eq!(
                std::env::split_paths(&joined).collect::<Vec<_>>(),
                [odd, other]
            );
        } else {
            assert_eq!(folders, [own]);
            assert!(std::env::join_paths([&odd]).is_err(), "no PATH names it");
            let joined = path_with_folders(&path, &[odd, other.clone()]);
            assert_eq!(
                joined,
                std::env::join_paths([path.as_os_str(), other.as_os_str()]).unwrap()
            );
        }
    }

    #[test]
    fn names_match_in_any_case_on_windows_only() {
        let env = Env::from_vars([("Path", "/bin")]);
        assert_eq!(env.text("PATH").is_some(), cfg!(windows));
        assert_eq!(env.text("Path"), Some("/bin"));
    }
}
