//! Every place a harness keeps something, in one table (K3, `live-kit-isolation-doors.md`
//! section 1): a row for each, with whose it is, its kind, and where it is found from the
//! account's environment. The protected roots the write door refuses, the stores the spawn
//! door's seal keeps out of a child's environment, the logins a machine is asked for and the
//! harness homes Muse's sandbox keeps a program out of all read this table, and nothing else
//! names a store.
//!
//! **A variable and its default are both roots.** Where the account has `XDG_DATA_HOME`,
//! `APPDATA`, `CODEX_HOME` or another such variable set, the folder it names and the folder used
//! without it are both protected. Every home of the account counts as a default: `HOME` and
//! `USERPROFILE` (distinct on Windows, and Codex reads both), and for Muse on a Mac
//! `CFFIXED_USER_HOME`. The table holds the rows of every platform on every platform: a row of
//! another platform's names a folder no harness of this one keeps anything in, and protecting it
//! costs nothing.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Whose a store is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Whose {
    Claude,
    Codex,
    Pi,
    Devin,
    Muse,
    Agy,
    /// The account's own, which no harness owns: the Keychain.
    Account,
}

/// What a store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A login: what signs the harness in.
    Login,
    /// Its settings.
    Settings,
    /// What it keeps of its sessions.
    Records,
    /// What it keeps to save work.
    Cache,
    /// Sockets and files of a running program.
    Runtime,
    /// A folder where a harness's own installer puts its program inside its store (Pi's
    /// `agent/bin`, Devin's `cli/_versions`): started and read, never written by the door.
    Program,
}

/// Where a store is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// `suffix` below each home of the account (`HOME`, `USERPROFILE`).
    Home(&'static str),
    /// `suffix` below each home of the account and below `CFFIXED_USER_HOME` (Muse on a Mac).
    FixedHome(&'static str),
    /// The folder the variable names, whole.
    Var(&'static str),
    /// `suffix` in the folder the variable names, and in `default` below each home, whether the
    /// variable is set or not.
    Under {
        var: &'static str,
        default: &'static str,
        suffix: &'static str,
    },
    /// `suffix` in the folder the variable names, where it is set; it has no default.
    UnderSet {
        var: &'static str,
        suffix: &'static str,
    },
    /// A folder of the system named for the account's user number (`{uid}`).
    System(&'static str),
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    pub whose: Whose,
    pub kind: Kind,
    pub place: Place,
}

const fn row(whose: Whose, kind: Kind, place: Place) -> Store {
    Store { whose, kind, place }
}

const fn under(var: &'static str, default: &'static str, suffix: &'static str) -> Place {
    Place::Under {
        var,
        default,
        suffix,
    }
}

use Kind::{Cache, Login, Program, Records, Runtime, Settings};
use Whose::{Account, Agy, Claude, Codex, Devin, Muse, Pi};

/// The table.
pub const STORES: &[Store] = &[
    // Claude Code.
    row(Claude, Login, Place::Home(".claude.json")),
    row(Claude, Login, Place::Home(".claude")),
    row(Claude, Settings, Place::Var("CLAUDE_CONFIG_DIR")),
    // Codex.
    row(Codex, Login, Place::Home(".codex")),
    row(Codex, Login, Place::Var("CODEX_HOME")),
    // Pi, and the program its installer puts in its store.
    row(Pi, Login, Place::Home(".pi")),
    row(Pi, Program, Place::Home(".pi/agent/bin")),
    row(Pi, Login, Place::Var("PI_CODING_AGENT_DIR")),
    row(Pi, Records, Place::Var("PI_CODING_AGENT_SESSION_DIR")),
    // Devin: the XDG folders on a Mac and Linux, `%APPDATA%` and `%LOCALAPPDATA%` on Windows,
    // each with the folder used without the variable.
    row(Devin, Login, under("XDG_CONFIG_HOME", ".config", "devin")),
    row(
        Devin,
        Login,
        under("XDG_DATA_HOME", ".local/share", "devin"),
    ),
    row(
        Devin,
        Program,
        under("XDG_DATA_HOME", ".local/share", "devin/cli/_versions"),
    ),
    row(
        Devin,
        Records,
        under("XDG_STATE_HOME", ".local/state", "devin"),
    ),
    row(Devin, Cache, under("XDG_CACHE_HOME", ".cache", "devin")),
    row(Devin, Login, under("APPDATA", "AppData/Roaming", "devin")),
    row(
        Devin,
        Records,
        under("LOCALAPPDATA", "AppData/Local", "devin"),
    ),
    // Muse Code: the XDG folders (on Windows too, below the profile), its runtime folders, and on
    // a Mac its store below the home and below the home the system was told to give it.
    row(Muse, Login, under("XDG_CONFIG_HOME", ".config", "muse")),
    row(
        Muse,
        Records,
        under("XDG_DATA_HOME", ".local/share", "muse"),
    ),
    row(
        Muse,
        Records,
        under("XDG_STATE_HOME", ".local/state", "muse"),
    ),
    row(Muse, Cache, under("XDG_CACHE_HOME", ".cache", "muse")),
    row(
        Muse,
        Runtime,
        Place::UnderSet {
            var: "XDG_RUNTIME_DIR",
            suffix: "muse",
        },
    ),
    row(
        Muse,
        Runtime,
        Place::System("/private/tmp/tbh-{uid}-rt/muse"),
    ),
    row(
        Muse,
        Settings,
        Place::FixedHome("Library/Application Support/Muse"),
    ),
    // agy.
    row(Agy, Login, Place::Home(".gemini")),
    // The account's Keychain.
    row(Account, Login, Place::Home("Library/Keychains")),
];

/// The names of the account's homes, each a default below which a [`Place::Home`] row is.
pub const HOMES: [&str; 2] = ["HOME", "USERPROFILE"];

/// A root the table gives: a place, the row it is of, and whether a variable named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    pub store: Store,
}

/// The account's user number, for a [`Place::System`] row.
#[cfg(unix)]
fn user_number() -> Option<u32> {
    Some(nix::unistd::getuid().as_raw())
}

#[cfg(not(unix))]
fn user_number() -> Option<u32> {
    None
}

/// The value of `name` as a folder: none where it is unset or empty. Relative values are made
/// absolute against nothing: a relative home is no home, and is left out.
fn folder(lookup: &dyn Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    let value = lookup(name).filter(|value| !value.is_empty())?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

/// A path the table spells with `/` below `base`, a component at a time.
fn below(base: &Path, suffix: &str) -> PathBuf {
    suffix
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// Every root the table gives for an account whose environment `lookup` reads: each row's
/// folder below each home and below each variable that names one, as spelled (the write door
/// resolves each when it judges).
pub fn roots(lookup: &dyn Fn(&str) -> Option<OsString>) -> Vec<Root> {
    roots_for(lookup, user_number())
}

/// [`roots`] with the user number `uid`.
pub fn roots_for(lookup: &dyn Fn(&str) -> Option<OsString>, uid: Option<u32>) -> Vec<Root> {
    let homes: Vec<PathBuf> = HOMES
        .iter()
        .filter_map(|name| folder(lookup, name))
        .collect();
    let mut roots = Vec::new();
    let mut add = |path: PathBuf, store: Store| {
        if !roots.iter().any(|root: &Root| root.path == path) {
            roots.push(Root { path, store });
        }
    };
    for store in STORES {
        match store.place {
            Place::Home(suffix) => {
                for home in &homes {
                    add(below(home, suffix), *store);
                }
            }
            Place::FixedHome(suffix) => {
                for home in homes
                    .iter()
                    .cloned()
                    .chain(folder(lookup, "CFFIXED_USER_HOME"))
                {
                    add(below(&home, suffix), *store);
                }
            }
            Place::Var(name) => {
                if let Some(named) = folder(lookup, name) {
                    add(named, *store);
                }
            }
            Place::Under {
                var,
                default,
                suffix,
            } => {
                if let Some(named) = folder(lookup, var) {
                    add(below(&named, suffix), *store);
                }
                for home in &homes {
                    add(below(&below(home, default), suffix), *store);
                }
            }
            Place::UnderSet { var, suffix } => {
                if let Some(named) = folder(lookup, var) {
                    add(below(&named, suffix), *store);
                }
            }
            Place::System(template) => {
                if let Some(uid) = uid {
                    add(
                        PathBuf::from(template.replace("{uid}", &uid.to_string())),
                        *store,
                    );
                }
            }
        }
    }
    roots
}

/// The places below a home where `whose` keeps its login, as the table has them and in its
/// order, spelled with `/`: what a machine's login of it is looked for by. Codex's is its whole
/// home, which `CODEX_HOME` may name instead, and is none of these.
pub fn login_marks(whose: Whose) -> Vec<String> {
    STORES
        .iter()
        .filter(|store| store.whose == whose && store.kind == Login)
        .filter_map(|store| match store.place {
            Place::Home(suffix) | Place::FixedHome(suffix) => Some(suffix.to_owned()),
            // The folder used without the variable; Windows' own are found by its variables.
            Place::Under {
                default, suffix, ..
            } if !default.starts_with("AppData") => Some(format!("{default}/{suffix}")),
            _ => None,
        })
        .collect()
}

/// Whether `path`, as spelled, is inside a row of the table for an account whose environment
/// `lookup` reads (compared by components, in any case where the platform's names are not
/// case-sensitive): for a plan that names a store, which must be one of the table's.
pub fn in_a_row(path: &Path, lookup: &dyn Fn(&str) -> Option<OsString>) -> Option<Store> {
    roots(lookup)
        .into_iter()
        .find(|root| spelled_inside(path, &root.path))
        .map(|root| root.store)
}

/// Whether `path` is `root` or below it, by components as spelled; in any case on a Mac and on
/// Windows, whose volumes are not case-sensitive by default.
pub fn spelled_inside(path: &Path, root: &Path) -> bool {
    let fold = |path: &Path| -> Vec<String> {
        path.components()
            .map(|part| {
                let name = part.as_os_str().to_string_lossy();
                if cfg!(any(windows, target_os = "macos")) {
                    name.to_lowercase()
                } else {
                    name.into_owned()
                }
            })
            .collect()
    };
    let (path, root) = (fold(path), fold(root));
    path.len() >= root.len() && path[..root.len()] == root[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An account whose every variable names a folder of its own, none of them the home's
    /// default: a rule that derives a store from the home alone misses each.
    fn decoy() -> HashMap<&'static str, PathBuf> {
        let base = PathBuf::from(if cfg!(windows) { r"D:\decoy" } else { "/decoy" });
        assert!(base.is_absolute());
        [
            "HOME",
            "USERPROFILE",
            "CFFIXED_USER_HOME",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "PI_CODING_AGENT_DIR",
            "PI_CODING_AGENT_SESSION_DIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "XDG_RUNTIME_DIR",
            "APPDATA",
            "LOCALAPPDATA",
        ]
        .into_iter()
        .map(|name| (name, base.join(name.to_lowercase())))
        .collect()
    }

    fn lookup_of<'a>(
        vars: &'a HashMap<&'static str, PathBuf>,
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| vars.get(name).map(|path| path.clone().into_os_string())
    }

    #[test]
    fn each_row_of_the_table_is_refused_on_a_decoy_of_another_name() {
        let vars = decoy();
        let lookup = lookup_of(&vars);
        let roots = roots_for(&lookup, Some(501));
        let has = |path: PathBuf| {
            assert!(
                roots.iter().any(|root| root.path == path),
                "{} is no root of {:#?}",
                path.display(),
                roots.iter().map(|root| &root.path).collect::<Vec<_>>()
            );
        };
        let at = |name: &str, suffix: &str| below(&vars[name], suffix);
        // Each variable's folder, and the default below each home of the account, distinct.
        for home in ["HOME", "USERPROFILE"] {
            for suffix in [
                ".claude",
                ".claude.json",
                ".codex",
                ".pi",
                ".gemini",
                "Library/Keychains",
            ] {
                has(at(home, suffix));
            }
            // Windows Devin with no `APPDATA` falls to the profile; Windows Muse to its XDG
            // defaults below the profile.
            has(at(home, "AppData/Roaming/devin"));
            has(at(home, "AppData/Local/devin"));
            has(at(home, ".config/muse"));
            has(at(home, ".local/share/muse"));
            // Mac Muse's cache and state, and its store below the home.
            has(at(home, ".cache/muse"));
            has(at(home, ".local/state/muse"));
            has(at(home, "Library/Application Support/Muse"));
            has(at(home, ".local/share/devin/cli/_versions"));
        }
        has(vars["CLAUDE_CONFIG_DIR"].clone());
        has(vars["CODEX_HOME"].clone());
        has(vars["PI_CODING_AGENT_DIR"].clone());
        has(vars["PI_CODING_AGENT_SESSION_DIR"].clone());
        for var in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
        ] {
            has(at(var, "muse"));
            has(at(var, "devin"));
        }
        has(at("XDG_RUNTIME_DIR", "muse"));
        has(at("APPDATA", "devin"));
        has(at("LOCALAPPDATA", "devin"));
        has(at("CFFIXED_USER_HOME", "Library/Application Support/Muse"));
        has(PathBuf::from("/private/tmp/tbh-501-rt/muse"));
    }

    #[test]
    fn a_windows_home_and_profile_that_differ_are_each_a_default() {
        // Codex reads both (`cf-harness/src/codex/paths.rs`): neither is left out for the other.
        let (one, two) = if cfg!(windows) {
            (r"C:\one", r"C:\two")
        } else {
            ("/one", "/two")
        };
        let vars: HashMap<&'static str, PathBuf> = [
            ("HOME", PathBuf::from(one)),
            ("USERPROFILE", PathBuf::from(two)),
        ]
        .into();
        let roots = roots_for(&lookup_of(&vars), None);
        for home in [one, two] {
            assert!(roots
                .iter()
                .any(|root| root.path == Path::new(home).join(".codex")));
        }
    }

    #[test]
    fn the_logins_a_machine_is_asked_for_are_the_tables() {
        // In the order a machine is asked: the first that is there says it is set up.
        assert_eq!(login_marks(Whose::Claude), [".claude.json", ".claude"]);
        assert_eq!(login_marks(Whose::Pi), [".pi"]);
        assert_eq!(
            login_marks(Whose::Devin),
            [".config/devin", ".local/share/devin"]
        );
        assert_eq!(login_marks(Whose::Muse), [".config/muse"]);
    }
}
