//! The spawn door's seal (K3, `live-kit-isolation-doors.md` section 2): one function, asked of the
//! whole environment a program will really have, wherever that environment leaves the kit (a
//! program [`crate::process::Run`] starts, a window the rig's pane host opens, a helper's
//! question). It holds when:
//!
//! - every home variable is there, each spelled by its canonical name, and each names a folder
//!   this run made, by where it resolves; on Windows the profile's two halves (`HOMEDRIVE` and
//!   `HOMEPATH`) are judged joined, against `USERPROFILE`, and never each alone;
//! - no other variable but the search path names a path inside a protected root;
//! - no name is given twice in different case (names are matched in any case, on every
//!   platform), and on Unix no home variable is spelled in another case: a `home` is refused,
//!   never taken for `HOME`;
//! - none of the kit's own variables (`CF_LIVE_*`) the door gives is changed or taken away.
//!
//! A window is also judged by its program ([`program_of`]): a harness's program needs the
//! switches that keep it from updating itself, and a program that needs a door of its own (the
//! Keychain's, cargo) is refused in a plain window.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Every variable by which a harness finds its home, as the seal names them: each must be there
/// and name a folder of the run's.
pub const HOME_VARS: [&str; 13] = [
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
    "APPDATA",
    "LOCALAPPDATA",
];

/// Home variables that may be absent, and name a folder of the run's where they are there.
pub const OPTIONAL_HOME_VARS: [&str; 1] = ["XDG_RUNTIME_DIR"];

/// The two halves of a Windows profile, judged joined against `USERPROFILE`.
pub const PROFILE_HALVES: [&str; 2] = ["HOMEDRIVE", "HOMEPATH"];

/// The kit's own variables, which only the door sets.
pub const KIT_PREFIX: &str = "CF_LIVE_";

/// What the seal asks of the world: whether a folder is the run's, whether a path is inside a
/// protected root, and what the door gives of its own.
pub struct Judges<'a> {
    /// Whether the environment is a Windows program's: names are the same in any case, and the
    /// profile has two halves.
    pub windows: bool,
    /// Whether `path` resolves to a folder this run made, or below one.
    pub is_runs: &'a dyn Fn(&Path) -> bool,
    /// Whether `path` resolves inside a protected root.
    pub in_protected: &'a dyn Fn(&Path) -> bool,
    /// The kit's own variables the door gives (`CF_LIVE_*`), each with its value.
    pub kit: &'a [(String, String)],
}

fn canonical_home_name(name: &str) -> Option<&'static str> {
    HOME_VARS
        .iter()
        .chain(OPTIONAL_HOME_VARS.iter())
        .chain(PROFILE_HALVES.iter())
        .find(|home| home.eq_ignore_ascii_case(name))
        .copied()
}

/// A Windows path as text the profile's halves are compared by: `\` for `/`, Windows' verbatim
/// prefix taken off, no last `\`, in any case.
fn profile_text(path: &str) -> String {
    let text = path.replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.trim_end_matches('\\').to_lowercase()
}

/// Whether `path` is absolute for a program of the platform: on Windows a drive and a `\`, or a
/// verbatim or network prefix.
fn absolute(path: &str, windows: bool) -> bool {
    if !windows {
        return path.starts_with('/');
    }
    let bytes = path.as_bytes();
    path.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

/// A stand-in program's window: the proof, once, that the environment the window gave the stand-in
/// holds the seal (every home a folder of the run's, no store named by another variable), which the
/// stand-in's children are then given as it is ([`crate::process::Run::as_this_window`]). Only the
/// stand-in programs (`src/bin`) make one; where the environment breaks the seal, the window has no
/// verdict to give and no child of the stand-in starts.
#[derive(Debug)]
pub struct Window {
    verdict: Result<(), String>,
}

impl Window {
    /// This process's window, judged once on its own environment.
    pub fn this() -> &'static Window {
        static THIS: std::sync::OnceLock<Window> = std::sync::OnceLock::new();
        THIS.get_or_init(|| {
            // The door judges the window's own environment: it is what is read here.
            #[allow(clippy::disallowed_methods)]
            let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
            let kit = kit_vars(&env);
            Window {
                verdict: super::sealed(env, &kit, cfg!(windows)).map(drop),
            }
        })
    }

    /// Whether the seal held, or why not.
    pub fn verdict(&self) -> Result<(), String> {
        self.verdict.clone()
    }
}

/// Seals `vars`, the whole environment a program will have, in the order given (a later value of
/// the very same name wins): the environment to give it, its home variables by their canonical
/// names, or why it is refused.
pub fn seal(
    vars: Vec<(OsString, OsString)>,
    judges: &Judges,
) -> Result<Vec<(OsString, OsString)>, String> {
    // A later value of the same name wins: the list is a builder's.
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    for (name, value) in vars {
        env.retain(|(held, _)| held != &name);
        env.push((name, value));
    }
    // One name in two spellings: a program gets one of them by rules no caller controls.
    for (index, (name, _)) in env.iter().enumerate() {
        let text = name.to_string_lossy();
        if let Some((other, _)) = env[index + 1..]
            .iter()
            .find(|(other, _)| other.to_string_lossy().eq_ignore_ascii_case(&text))
        {
            return Err(format!(
                "the variable {text} is given twice, also as {}",
                other.to_string_lossy()
            ));
        }
    }
    // The canonical names of the homes: Windows reads any spelling as the one variable, and the
    // seal says it as itself; on Unix another spelling is another variable, which no harness
    // reads as its home, and is refused rather than admitted as one.
    for (name, _) in &mut env {
        let text = name.to_string_lossy().into_owned();
        if let Some(home) = canonical_home_name(&text) {
            if home != text {
                if judges.windows {
                    *name = OsString::from(home);
                } else {
                    return Err(format!(
                        "the variable {text} is {home} spelled in another case, which a program here does not read as {home}"
                    ));
                }
            }
        }
    }
    let get = |name: &str| -> Option<&OsStr> {
        env.iter()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value.as_os_str())
    };
    let run_folder = |name: &str, value: &OsStr| -> Result<(), String> {
        let path = Path::new(value);
        if value.is_empty() || !absolute(&value.to_string_lossy(), judges.windows) {
            return Err(format!(
                "{name} is {}, which is no folder this run made",
                path.display()
            ));
        }
        if !(judges.is_runs)(path) {
            return Err(format!(
                "{name} is {}, which is no folder this run made",
                path.display()
            ));
        }
        // A folder below the run's that leads into a store is the store.
        if (judges.in_protected)(path) {
            return Err(format!(
                "{name} is {}, inside a store of a harness or of the account",
                path.display()
            ));
        }
        Ok(())
    };
    for name in HOME_VARS {
        let Some(value) = get(name) else {
            return Err(format!("the home variable {name} is not given"));
        };
        run_folder(name, value)?;
    }
    for name in OPTIONAL_HOME_VARS {
        if let Some(value) = get(name) {
            run_folder(name, value)?;
        }
    }
    // The profile's two halves, joined, are the profile: each alone is no folder.
    let halves: Vec<Option<&OsStr>> = PROFILE_HALVES.iter().map(|name| get(name)).collect();
    if halves.iter().any(Option::is_some) {
        let (Some(drive), Some(rest)) = (halves[0], halves[1]) else {
            return Err("HOMEDRIVE and HOMEPATH are given one without the other".to_owned());
        };
        let joined = format!("{}{}", drive.to_string_lossy(), rest.to_string_lossy());
        let profile = get("USERPROFILE").map(|value| value.to_string_lossy().into_owned());
        if profile.as_deref().map(profile_text) != Some(profile_text(&joined)) {
            return Err(format!(
                "HOMEDRIVE and HOMEPATH say {joined}, which is not the profile USERPROFILE names"
            ));
        }
    }
    // No other variable but the search path names a store.
    for (name, value) in &env {
        let text = name.to_string_lossy();
        let upper = text.to_ascii_uppercase();
        // The door's own variables name the door's own files: the roots' file is protected
        // from every act, and is no store a program is pointed at.
        let the_doors = judges.kit.iter().any(|(kit, given)| {
            kit.eq_ignore_ascii_case(&text) && OsStr::new(given) == value.as_os_str()
        });
        if upper == "PATH" || canonical_home_name(&text).is_some() || the_doors {
            continue;
        }
        let whole = std::iter::once(PathBuf::from(value));
        let parts = std::env::split_paths(value);
        if let Some(inside) = whole
            .chain(parts)
            .filter(|path| absolute(&path.to_string_lossy(), judges.windows))
            .find(|path| (judges.in_protected)(path))
        {
            return Err(format!(
                "the variable {text} names {}, inside a store of a harness or of the account",
                inside.display()
            ));
        }
    }
    // The kit's own variables are the door's: none changed, none taken away, none added for it.
    for (name, value) in judges.kit {
        match get(name) {
            Some(given) if given == OsStr::new(value) => {}
            _ => {
                return Err(format!(
                    "the kit's own variable {name} was changed or taken away"
                ))
            }
        }
    }
    for owned in [
        super::MADE_ROOTS,
        super::account::PROTECTED_FILE,
        super::account::PROTECTED_DIGEST,
    ] {
        if get(owned).is_some() && !judges.kit.iter().any(|(name, _)| name == owned) {
            return Err(format!(
                "the kit's own variable {owned} is given by a caller, not by the door"
            ));
        }
    }
    Ok(env)
}

/// The home variables `vars` do not name, below `base` ([`crate::standin_process::home_vars`]),
/// on Windows with the profile's halves where neither is named.
pub fn missing_homes(vars: &[(OsString, OsString)], base: &Path) -> Vec<(OsString, OsString)> {
    let named = |wanted: &str| {
        vars.iter()
            .any(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(wanted))
    };
    let mut missing: Vec<(OsString, OsString)> = crate::standin_process::home_vars(base)
        .into_iter()
        .filter(|(name, _)| !named(name) && !PROFILE_HALVES.contains(name))
        .map(|(name, value)| (OsString::from(name), value.into_os_string()))
        .collect();
    // The halves come with the profile they say, where neither is named: on Windows, of the
    // profile the program is given.
    if cfg!(windows) && !named("HOMEDRIVE") && !named("HOMEPATH") {
        let profile = vars
            .iter()
            .chain(missing.iter())
            .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("USERPROFILE"))
            .map(|(_, value)| PathBuf::from(value));
        if let Some((drive, path)) = profile
            .as_deref()
            .and_then(crate::standin_process::drive_and_path)
        {
            missing.push(("HOMEDRIVE".into(), drive.into()));
            missing.push(("HOMEPATH".into(), path.into()));
        }
    }
    missing
}

/// What a program is, by the file it resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// Any program of the machine's or the run's own.
    Plain,
    /// A harness's program: named for the harness it is (`claude`, `codex`, `pi`, `devin`,
    /// `muse`, `agy`).
    Harness(&'static str),
    /// The Keychain's utility, which only its door starts.
    Keychain,
    /// Cargo, which only its door starts.
    Cargo,
}

/// The harnesses a program's name is matched against, the last net.
const HARNESS_NAMES: [&str; 7] = ["claude", "codex", "pi", "devin", "muse", "agy", "gemini"];

/// The harness's word for a name (`gemini` is agy's).
fn harness_word(name: &str) -> Option<&'static str> {
    HARNESS_NAMES
        .iter()
        .find(|word| **word == name)
        .map(|word| if *word == "gemini" { "agy" } else { *word })
}

/// What `program` is, known by the file it resolves to (`real`, where it resolves; `program` as
/// given otherwise): inside a store of the table (Pi's `agent/bin`, Devin's `cli/_versions`), Muse's
/// `muse-bin-<version>`, Claude's `versions/<version>`, the Keychain's `security`, cargo; and by
/// its name as the last net, where it is no file the run made (a stand-in named `claude` in a
/// rig's folder is the run's own).
pub fn program_of(
    program: &Path,
    real: Option<&Path>,
    in_store: bool,
    made_by_run: bool,
) -> Program {
    let file = real.unwrap_or(program);
    let name_of = |path: &Path| -> String {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        name.split('.').next().unwrap_or_default().to_owned()
    };
    let name = name_of(file);
    if file == Path::new("/usr/bin/security") || program == Path::new("/usr/bin/security") {
        return Program::Keychain;
    }
    if name.starts_with("muse-bin-") {
        return Program::Harness("muse");
    }
    let parent = file.parent();
    let versions_of_claude = parent.is_some_and(|folder| {
        folder.file_name().is_some_and(|name| name == "versions")
            && folder
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.eq_ignore_ascii_case("claude"))
    });
    if versions_of_claude {
        return Program::Harness("claude");
    }
    if in_store {
        return Program::Harness(harness_word(&name).unwrap_or("a harness"));
    }
    if made_by_run {
        return Program::Plain;
    }
    let given = name_of(program);
    for candidate in [name.as_str(), given.as_str()] {
        if candidate == "security" {
            return Program::Keychain;
        }
        if candidate == "cargo" {
            return Program::Cargo;
        }
        if let Some(word) = harness_word(candidate) {
            return Program::Harness(word);
        }
    }
    Program::Plain
}

/// The switches a harness's program is given so that it updates and installs nothing itself, as
/// the kit gives them ([`crate::live::launcher::updates_off_of`]); none for one told by its own
/// configuration.
pub fn update_switches(harness: &str) -> &'static [(&'static str, &'static str)] {
    use crate::live::launcher::{CLAUDE_UPDATES_OFF, MUSE_UPDATES_OFF, PI_UPDATES_OFF};
    match harness {
        "claude" => &CLAUDE_UPDATES_OFF,
        "muse" => &MUSE_UPDATES_OFF,
        "pi" => &PI_UPDATES_OFF,
        _ => &[],
    }
}

/// Whether `env` holds every update switch `program` needs: why not, where it does not.
pub fn has_update_switches(program: &Program, env: &[(OsString, OsString)]) -> Result<(), String> {
    let Program::Harness(harness) = program else {
        return Ok(());
    };
    for (name, value) in update_switches(harness) {
        let given = env
            .iter()
            .find(|(held, _)| held.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, given)| given.as_os_str());
        if given != Some(OsStr::new(value)) {
            return Err(format!(
                "{harness}'s program is started without {name}={value}, which keeps it from updating itself"
            ));
        }
    }
    Ok(())
}

/// The environment a window will really have, as the pane host makes it: its own environment
/// (`host`), the request's `env` over it, and its `dropEnv` taken out last (`cf-panes`'
/// `pty::spawn`: removal comes last so that a guarded name cannot be put back by the map); names
/// in any case on Windows. An error where the request's environment is no map of text, or its
/// `dropEnv` no list of names.
pub fn pane_environment(
    host: &[(OsString, OsString)],
    body: &serde_json::Value,
    windows: bool,
) -> Result<Vec<(OsString, OsString)>, String> {
    let same = |left: &OsStr, right: &str| {
        if windows {
            left.to_string_lossy().eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    };
    let mut env = host.to_vec();
    match &body["env"] {
        serde_json::Value::Null => {}
        serde_json::Value::Object(vars) => {
            for (name, value) in vars {
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("the window's variable {name} is no text"))?;
                env.retain(|(held, _)| !same(held, name));
                env.push((OsString::from(name), OsString::from(value)));
            }
        }
        other => return Err(format!("the window's environment is no map: {other}")),
    }
    match &body["dropEnv"] {
        serde_json::Value::Null => {}
        serde_json::Value::Array(names) => {
            for name in names {
                let name = name
                    .as_str()
                    .ok_or_else(|| format!("a name the window drops is no text: {name}"))?;
                env.retain(|(held, _)| !same(held, name));
            }
        }
        other => return Err(format!("the names the window drops are no list: {other}")),
    }
    Ok(env)
}

/// The kit's own variables of `env`, as the door gave them to the pane host: a window keeps
/// each.
pub fn kit_vars(env: &[(OsString, OsString)]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(name, _)| {
            name.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with(KIT_PREFIX)
        })
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}
