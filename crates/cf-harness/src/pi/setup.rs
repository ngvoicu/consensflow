//! What gives a Pi its providers, named: the one thing the human needs said when Pi lists no
//! model (HMD-13). Pi's own answer to `get_available_models` is an empty list for a Pi with
//! nothing set up and for a Pi whose provider could not list (a package's provider whose server
//! is down lists none, and says nothing), so the list cannot tell them apart. This can, by
//! names alone.
//!
//! **Names only, and never a value** (the review T-1270, F2). What is read is the keys of
//! `auth.json` (the providers), the keys of `models.json`'s `providers`, and the sources of
//! `settings.json`'s `packages` and `extensions`; each file is parsed into those and into
//! `serde`'s `IgnoredAny` for everything else, so that no login, key or address is ever held
//! as a value, and the bytes are read through a small buffer, not kept. The environment is only
//! asked whether a variable is set to something (no copy of what it holds). A package source is
//! shown without the user and password of its URL, its query and its fragment ([`shown`]).
//!
//! **Read only when it is wanted, and bounded** (F4). The caller asks for a setup only when
//! Pi listed no model or the read failed. A file is read only if it is a regular file of at
//! most [`FILE_CAP`] bytes, so a pipe where a file should be is never opened (it would wait for
//! a writer for ever) and a file of a size no setup is is not read whole; the `extensions` folder
//! is read to [`ENTRIES`] entries. A file that is there and cannot be read so is *unread*, and is
//! said so: a setup that could not be read is never called "no provider is set up".
//!
//! Pi (1.5.0, `model-runtime.js`, `resource-loader.js`) has its models from, in the order the
//! docs give them:
//! - a login in the agent folder's `auth.json`, an OAuth one or an API key, one entry a provider;
//! - a provider key in the environment (`providers.md`, "Use an API key from the environment");
//! - a custom provider in `models.json`;
//! - a provider an installed Pi package registers, named in `settings.json`'s `packages`;
//! - a provider an extension registers: one in the agent folder's `extensions` folder, or one
//!   `settings.json`'s `extensions` names (a path; `-path` and `!pattern` leave one out, and
//!   `builtin:…` names Pi's own, none of which is a provider the human set up).
//!
//! A project's own `.pi` folder can name more of each; a read from here knows no project.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;

use cf_base::env::Env;
use cf_base::path;
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use super::paths::agent_dir;

/// The largest file of Pi's folder that is read: a setup's files are some kilobytes.
const FILE_CAP: u64 = 1 << 20;

/// The most entries of the `extensions` folder that are read.
const ENTRIES: usize = 256;

/// The environment variables Pi takes a provider's credentials from (`providers.md`): a key or a
/// token, or the base of a local server's address. The ones that only tune a provider that has a
/// key (`AWS_REGION`, `AZURE_OPENAI_API_VERSION`) are left out, and so is the key of the one
/// provider that belongs to the harness this repository no longer names: Pi lists a provider's
/// models whenever its key is set, so a Pi that lists nothing never has that one set.
const CREDENTIALS: [&str; 40] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "ANTHROPIC_AUTH_TOKEN",
    "ANT_LING_API_KEY",
    "OPENAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "NVIDIA_API_KEY",
    "GEMINI_API_KEY",
    "COPILOT_GITHUB_TOKEN",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "CEREBRAS_API_KEY",
    "XAI_API_KEY",
    "OPENROUTER_API_KEY",
    "AI_GATEWAY_API_KEY",
    "ZAI_API_KEY",
    "ZAI_CODING_CN_API_KEY",
    "RADIUS_API_KEY",
    "TYPESAFE_API_KEY",
    "HF_TOKEN",
    "FIREWORKS_API_KEY",
    "TOGETHER_API_KEY",
    "BASETEN_API_KEY",
    "KIMI_API_KEY",
    "META_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "MOONSHOT_API_KEY",
    "QWEN_TOKEN_PLAN_API_KEY",
    "QWEN_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_PROFILE",
    "AWS_ACCESS_KEY_ID",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "LLAMA_BASE_URL",
];

/// What the Pi folder and the environment name as the sources of Pi's providers: each list the
/// names of one source, empty where it has none.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Setup {
    /// The providers `auth.json` holds an entry for.
    pub logins: Vec<String>,
    /// The providers `models.json` defines.
    pub custom: Vec<String>,
    /// The sources `settings.json` lists under `packages` (`npm:@scope/name`, a path), shown
    /// without what could carry a credential ([`shown`]).
    pub packages: Vec<String>,
    /// What stands in the agent folder's `extensions` folder, and the extensions
    /// `settings.json` names.
    pub extensions: Vec<String>,
    /// The credential variables set in the environment, by name.
    pub keys: Vec<String>,
    /// The files that are there and could not be read: not a regular file, larger than a setup
    /// is, or not what Pi's files are. By name.
    pub unread: Vec<String>,
}

#[cfg(test)]
thread_local! {
    /// How many times a setup was read on this thread: a read that lists models reads none.
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many setups this thread has read (a test's).
#[cfg(test)]
pub(crate) fn reads() -> usize {
    READS.with(std::cell::Cell::get)
}

impl Setup {
    /// The setup of the Pi `env` names (`PI_CODING_AGENT_DIR`, else `.pi/agent` in the home).
    /// A file that is not there names nothing and one that cannot be read is said so
    /// ([`Setup::unread`]); no file is written.
    pub(crate) fn read(env: &Env) -> Self {
        #[cfg(test)]
        READS.with(|reads| reads.set(reads.get() + 1));
        let keys = CREDENTIALS
            .into_iter()
            .filter(|name| env.os(name).is_some_and(|value| !value.is_empty()))
            .map(str::to_owned)
            .collect();
        let Ok(agent) = agent_dir(env) else {
            return Self {
                keys,
                ..Self::default()
            };
        };
        let in_agent = |name: &str| path::join(&[&agent, name]);
        let mut unread = Vec::new();
        let mut logins = Vec::new();
        let mut custom = Vec::new();
        let mut packages = Vec::new();
        let mut extensions = Vec::new();
        match parsed::<HashMap<String, IgnoredAny>>(&in_agent("auth.json")) {
            Got::Found(table) => logins = sorted(table.into_keys()),
            Got::Absent => {}
            Got::Unread => unread.push("auth.json".to_owned()),
        }
        match parsed::<Models>(&in_agent("models.json")) {
            Got::Found(models) => custom = sorted(models.providers.into_keys()),
            Got::Absent => {}
            Got::Unread => unread.push("models.json".to_owned()),
        }
        match extension_folder(&in_agent("extensions")) {
            Got::Found(names) => extensions = names,
            Got::Absent => {}
            Got::Unread => unread.push("extensions".to_owned()),
        }
        match parsed::<Settings>(&in_agent("settings.json")) {
            Got::Found(settings) => {
                packages = settings
                    .packages
                    .into_iter()
                    .filter_map(|source| source.0)
                    .map(|source| shown(&source))
                    .collect();
                for declared in settings
                    .extensions
                    .into_iter()
                    .filter_map(|source| source.0)
                {
                    if let Some(declared) = declares_one(&declared) {
                        if !extensions.contains(&declared) {
                            extensions.push(declared);
                        }
                    }
                }
            }
            Got::Absent => {}
            Got::Unread => unread.push("settings.json".to_owned()),
        }
        Self {
            logins,
            custom,
            packages,
            extensions,
            keys,
            unread,
        }
    }

    /// Whether nothing at all is set up, and nothing is unread.
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// What a Pi that is loading a package or an extension may be waiting for: their names, for
    /// the words of a read given up on. None where there are none.
    pub(crate) fn loading(&self) -> Option<String> {
        let parts: Vec<String> = [
            ("packages", &self.packages),
            ("extensions", &self.extensions),
        ]
        .into_iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(what, names)| format!("{what} {}", names.join(", ")))
        .collect();
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// What Pi's empty list is to be told as: that no provider is set up and how to give one
    /// where that is true, else what is set up that listed nothing, and what could not be read.
    pub(crate) fn unlisted(&self) -> String {
        if self.is_empty() {
            return "Pi lists no model: no provider is set up (no login in auth.json, no provider \
                    in models.json, no installed package or extension, no provider key in the \
                    environment). Run pi and sign in with /login, or set a provider key such as \
                    OPENROUTER_API_KEY, or add a provider to models.json (Pi's providers.md and \
                    models.md say how)"
                .to_owned();
        }
        let parts: Vec<String> = [
            ("a login for", &self.logins),
            ("custom providers", &self.custom),
            ("packages", &self.packages),
            ("extensions", &self.extensions),
            ("provider keys", &self.keys),
        ]
        .into_iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(what, names)| format!("{what} {}", names.join(", ")))
        .collect();
        let unread = (!self.unread.is_empty())
            .then(|| format!("ConsensFlow could not read {}", self.unread.join(", ")));
        match (parts.is_empty(), unread) {
            (true, Some(unread)) => format!(
                "Pi lists no model, and {unread}: it cannot say whether a provider is set up there \
                 (each is a plain file of Pi's, of the size a setup is)"
            ),
            (_, unread) => format!(
                "Pi lists no model though it has {}{}: none of them gave it one. A provider that \
                 a package or an extension registers can list its models from a server of its \
                 own, and lists none while that server is down; a login Pi cannot use lists none",
                parts.join("; "),
                unread.map_or_else(String::new, |unread| format!("; and {unread}")),
            ),
        }
    }
}

/// What came of reading one file of Pi's folder.
enum Got<T> {
    /// It is not there.
    Absent,
    /// It was read, as the shape asked for.
    Found(T),
    /// It is there and was not read: no regular file, too large, or not that shape.
    Unread,
}

/// The file at `file`, read as `T`: only a regular file of at most [`FILE_CAP`] bytes is
/// opened, and `T` takes the names and ignores every value.
fn parsed<T: for<'de> Deserialize<'de>>(file: &str) -> Got<T> {
    let file = Path::new(file);
    match fs::metadata(file) {
        Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => return Got::Absent,
        Err(_) => return Got::Unread,
        Ok(found) if !found.is_file() || found.len() > FILE_CAP => return Got::Unread,
        Ok(_) => {}
    }
    let Ok(opened) = File::open(file) else {
        return Got::Unread;
    };
    match serde_json::from_reader(BufReader::new(opened.take(FILE_CAP))) {
        Ok(found) => Got::Found(found),
        Err(_) => Got::Unread,
    }
}

/// The names in the agent folder's `extensions` folder, but the hidden ones.
fn extension_folder(folder: &str) -> Got<Vec<String>> {
    let folder = Path::new(folder);
    match fs::metadata(folder) {
        Err(failed) if failed.kind() == std::io::ErrorKind::NotFound => return Got::Absent,
        Ok(found) if found.is_dir() => {}
        _ => return Got::Unread,
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return Got::Unread;
    };
    let names = entries
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| !name.starts_with('.'))
        .take(ENTRIES);
    Got::Found(sorted(names))
}

fn sorted(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names.collect();
    names.sort();
    names
}

/// The extension an entry of `settings.json`'s `extensions` declares, as it names it: none for
/// an exclusion (`-path`, `!pattern`) and for one of Pi's own (`builtin:mcp`, `+builtin:…`),
/// which is no provider the human set up.
fn declares_one(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() || entry.starts_with(['-', '!']) {
        return None;
    }
    let entry = entry.strip_prefix('+').unwrap_or(entry);
    (!entry.starts_with("builtin:")).then(|| entry.to_owned())
}

/// A package source as it is shown, without what could carry a credential: the user and
/// password of its URL, its query and its fragment. What names the package stays (`npm:`,
/// `git:github.com/owner/repo@ref`, a path).
///
/// A source is a URL where it has `://` or is a `git:` or `git@` one, and only then is it
/// looked at so: an npm name begins with the `@` of its scope.
pub(crate) fn shown(source: &str) -> String {
    let url = source.contains("://") || source.starts_with("git:") || source.starts_with("git@");
    if !url {
        return source.to_owned();
    }
    let mut shown = source
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_owned();
    // The authority of every URL in it, up to its first `/`: its user goes, whatever follows.
    let mut from = 0;
    while let Some(found) = shown[from..].find("://") {
        let start = from + found + 3;
        let end = shown[start..]
            .find('/')
            .map_or(shown.len(), |end| start + end);
        if let Some(at) = shown[start..end].rfind('@') {
            shown.replace_range(start..=start + at, "");
        }
        from = start;
    }
    // `git:user@host/path` and `git@host:path`, which have no `://`: the user is what comes
    // before the last `@` of the first part, up to its first `/`.
    let (scheme, rest) = shown
        .strip_prefix("git:")
        .map_or(("", shown.as_str()), |rest| ("git:", rest));
    if !rest.contains("://") {
        let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
        if let Some(at) = authority.rfind('@') {
            shown = format!("{scheme}{}", &rest[at + 1..]);
        }
    }
    shown
}

/// `models.json`: the providers it defines, by name.
#[derive(Deserialize)]
struct Models {
    #[serde(default)]
    providers: HashMap<String, IgnoredAny>,
}

/// `settings.json`: the sources of its packages and of its extensions.
#[derive(Deserialize)]
struct Settings {
    #[serde(default)]
    packages: Vec<Source>,
    #[serde(default)]
    extensions: Vec<Source>,
}

/// An entry of a list of sources: a string, or a table with a `source` string (`packages` takes
/// both); whatever else is in a table is ignored, never held. An entry of another kind is none.
struct Source(Option<String>);

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct One;

        impl<'de> Visitor<'de> for One {
            type Value = Source;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a source")
            }

            fn visit_str<E>(self, source: &str) -> Result<Source, E> {
                Ok(Source(Some(source.to_owned())))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut table: A) -> Result<Source, A::Error> {
                let mut source = None;
                while let Some(key) = table.next_key::<String>()? {
                    if key == "source" {
                        source = table.next_value::<Option<String>>()?;
                    } else {
                        table.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(Source(source))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<Source, A::Error> {
                while list.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Source(None))
            }

            fn visit_bool<E>(self, _: bool) -> Result<Source, E> {
                Ok(Source(None))
            }

            fn visit_i64<E>(self, _: i64) -> Result<Source, E> {
                Ok(Source(None))
            }

            fn visit_u64<E>(self, _: u64) -> Result<Source, E> {
                Ok(Source(None))
            }

            fn visit_f64<E>(self, _: f64) -> Result<Source, E> {
                Ok(Source(None))
            }

            fn visit_unit<E>(self) -> Result<Source, E> {
                Ok(Source(None))
            }
        }

        deserializer.deserialize_any(One)
    }
}

#[cfg(test)]
mod tests;
