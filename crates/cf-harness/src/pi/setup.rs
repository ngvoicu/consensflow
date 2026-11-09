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
//! **On a thread of its own** (the second reading, F4). The daemon runs every project's
//! requests, timers and deliveries on one thread, and a file that does not open (a stalled
//! network mount) held all of them for as long as it stalled: a 3 s stall was a 3.035 s gap in
//! the heartbeat, with a bound of 1 s on the read. [`inspect`] reads the folder on a thread
//! made for the one read, waits [`WAIT_MS`] for its answer, and says "Pi's folder could not be
//! read in time" where it has none. The records' reads are on a thread of their own too
//! (`records/thread.rs`).
//!
//! **One read at a time, and an ask that overlaps it takes its answer** (T-1281). While a read
//! is on its thread no other is started beside it, so a mount that never answers holds one
//! thread, not one for every list. An ask for the same folder that comes meanwhile waits for
//! that read and is handed what it found, for a bound of its own: the lookup of the default
//! model reads outside the daemon's turn for the harness, so it can overlap a listing, and it
//! was told the folder was late in 14 ms when the folder answers in 2. "Late" is said only when
//! an ask's own [`WAIT_MS`] has passed. An ask for another folder (another agent folder, or
//! another set of provider variables, as the tests of one process make) takes no answer that is
//! not its own: it waits for the read in flight to end and reads its own.
//!
//! **Words that are no credential's** (the second reading, F2). The words of a setup reach the
//! ledger and the page, where the engine withholds a line that may hold a credential
//! (`cf_engine::quotable`): ConsensFlow's own words here use none of the words that rule takes
//! for one. A variable of the environment is named by whose it is (`OPENROUTER` for
//! `OPENROUTER_API_KEY`, [`provider_of`]), the logins file is not named, and Pi is told how to
//! give a provider without a variable's name. A test in the daemon holds each of these words to
//! it.
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

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::Ordering;
#[cfg(not(test))]
use std::sync::LazyLock;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Poll;

use cf_base::env::Env;
use cf_base::path;
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use tokio::sync::watch;

use super::paths::agent_dir;
use crate::seams::{arm, Time};

/// How long the answer of a read of the folder is waited for. Its files are some kilobytes of
/// a local disk: past this a file is not opening, and the words say so.
pub(crate) const WAIT_MS: u64 = 2_000;

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

/// The words of a credential variable's name that say it is a key.
const SAYING_KEY: [&str; 9] = [
    "API",
    "KEY",
    "TOKEN",
    "OAUTH",
    "AUTH",
    "BEARER",
    "CREDENTIALS",
    "ACCESS",
    "ID",
];

/// The provider a credential variable is for, as the words name it: the variable's name without
/// the words that say it is a key (`OPENROUTER_API_KEY` is `OPENROUTER`, `QWEN_TOKEN_PLAN_API_KEY`
/// is `QWEN_PLAN`, `AWS_ACCESS_KEY_ID` is `AWS`). The name of a variable says "key", "token" or
/// "credentials", and a line of a harness's words that says them is withheld, not quoted.
fn provider_of(name: &str) -> String {
    let parts: Vec<&str> = name
        .split('_')
        .filter(|part| !SAYING_KEY.contains(part))
        .collect();
    if parts.is_empty() {
        name.to_owned()
    } else {
        parts.join("_")
    }
}

/// What the Pi folder and the environment name as the sources of Pi's providers: each list the
/// names of one source, empty where it has none.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
    /// The providers whose credential variable is set in the environment, by name
    /// ([`provider_of`]): never the variable's own name or its value.
    pub keys: Vec<String>,
    /// The files that are there and could not be read: not a regular file, larger than a setup
    /// is, or not what Pi's files are. By name.
    pub unread: Vec<String>,
    /// The folder was not read in time: a file did not open and the ask's bound passed
    /// ([`inspect`]). Nothing is known of it.
    pub late: bool,
}

/// What a test sees and does of the reads of the folder its thread asks for. A read is on a
/// thread of its own ([`inspect`]), so what a test sets and counts is the asking thread's, handed
/// to the reading one.
#[cfg(test)]
mod probe {
    use std::cell::RefCell;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    pub(super) type Hook = Arc<dyn Fn() + Send + Sync>;

    #[derive(Clone, Default)]
    pub(super) struct Probe {
        /// What runs where a read begins: a test's stall of a file that does not open.
        pub before: Option<Hook>,
        /// How many setups were read: a read that lists models reads none.
        pub reads: Arc<AtomicUsize>,
        /// How many times the folder was asked for ([`super::inspect`]), whether a read was
        /// begun for the ask or not.
        pub asks: Arc<AtomicUsize>,
        /// The read in flight (each test has its own, as it has its own thread).
        pub flight: super::Flight,
    }

    thread_local! {
        static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
    }

    pub(super) fn current() -> Probe {
        PROBE.with(|probe| probe.borrow().clone())
    }

    pub(super) fn adopt(probe: Probe) {
        PROBE.with(|slot| *slot.borrow_mut() = probe);
    }
}

/// How many setups the asking thread's reads have read (a test's).
#[cfg(test)]
pub(crate) fn reads() -> usize {
    probe::current()
        .reads
        .load(std::sync::atomic::Ordering::SeqCst)
}

/// How many times the asking thread's reads asked for the folder (a test's): a read that is held
/// until the second ask is in sees the asks overlap.
#[cfg(test)]
pub(crate) fn asks() -> usize {
    probe::current()
        .asks
        .load(std::sync::atomic::Ordering::SeqCst)
}

/// Whether a read of the folder asked for by the asking thread's reads is still on its thread (a
/// test's).
#[cfg(test)]
pub(crate) fn reading_now() -> bool {
    occupied(&probe::current().flight)
}

/// Has `hook` run where a read of the folder begins, on the thread that reads it.
#[cfg(test)]
pub(crate) fn before_read(hook: impl Fn() + Send + Sync + 'static) {
    let mut current = probe::current();
    current.before = Some(std::sync::Arc::new(hook));
    probe::adopt(current);
}

/// What a read of the folder is for: the agent folder it reads and the provider variables the
/// environment has set, which are all that [`Setup::read`] takes from `env`. A read answers an
/// ask for the same.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Question {
    agent: Option<String>,
    keys: Vec<String>,
}

impl Question {
    fn of(env: &Env) -> Self {
        Self {
            agent: agent_dir(env).ok(),
            keys: keys_of(env),
        }
    }
}

/// The answer of a read: nothing until the reading thread has one, then the setup. A thread that
/// ends with none (it panicked) drops its end, which the asks waiting for it see.
type Answer = watch::Receiver<Option<Setup>>;

/// A read on its thread: what it was asked, and where its answer will be.
struct Reading {
    asked: Question,
    answer: Answer,
}

/// Where the one read there is stands: none, or the read on its thread. A process has one
/// ([`flight`]); a test has one of its own.
type Flight = Arc<Mutex<Option<Reading>>>;

/// The read in flight: the process's one.
fn flight() -> Flight {
    #[cfg(not(test))]
    {
        static FLIGHT: LazyLock<Flight> = LazyLock::new(Flight::default);
        Arc::clone(&FLIGHT)
    }
    #[cfg(test)]
    {
        probe::current().flight
    }
}

/// Whether a read is on its thread.
#[cfg(test)]
fn occupied(flight: &Flight) -> bool {
    flight
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

/// Clears the read in flight when its thread ends, however it ends.
struct Done(Flight);

impl Drop for Done {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// What an ask is to do about the read in flight.
enum Turn {
    /// None is: this ask begins it, and sends its answer.
    Begin(watch::Sender<Option<Setup>>, Answer),
    /// One is, of the same question: wait for its answer.
    Share(Answer),
    /// One is, of another question: wait for it to end, and ask again.
    Wait(Answer),
}

/// Gives the executor a turn before the caller goes on.
async fn give_a_turn() {
    let mut given = false;
    std::future::poll_fn(|context| {
        if given {
            return Poll::Ready(());
        }
        given = true;
        context.waker().wake_by_ref();
        Poll::Pending
    })
    .await;
}

/// The setup of the Pi `env` names, read on a thread of its own and waited for [`WAIT_MS`]:
/// the thread that asks is the daemon's, which runs every project's requests, timers and
/// deliveries, and is not held by a file that does not open. One read at a time, so that a
/// mount that never answers holds one thread and no more: an ask that comes while a read of the
/// same folder is on its thread waits for that read and takes its answer, for a bound of its own.
/// Where the folder does not answer in time, the setup is [`Setup::late`].
pub(crate) async fn inspect(time: &dyn Time, env: &Env) -> Setup {
    #[cfg(test)]
    probe::current().asks.fetch_add(1, Ordering::SeqCst);
    let asked = Question::of(env);
    let flight = flight();
    let waited = arm(time, WAIT_MS);
    loop {
        let turn = {
            let mut reading = flight.lock().unwrap_or_else(PoisonError::into_inner);
            match reading.as_ref() {
                Some(going) if going.asked == asked => Turn::Share(going.answer.clone()),
                Some(going) => Turn::Wait(going.answer.clone()),
                None => {
                    let (sender, answer) = watch::channel(None);
                    *reading = Some(Reading {
                        asked: asked.clone(),
                        answer: answer.clone(),
                    });
                    Turn::Begin(sender, answer)
                }
            }
        };
        let (mut answered, same) = match turn {
            Turn::Begin(sender, answer) => {
                let env = env.clone();
                #[cfg(test)]
                let probe = probe::current();
                let done = Done(Arc::clone(&flight));
                let worker = std::thread::Builder::new()
                    .name("pi-setup".to_owned())
                    .spawn(move || {
                        #[cfg(test)]
                        probe::adopt(probe);
                        let setup = Setup::read(&env);
                        // The read is over before its answer is sent: whoever sees the answer
                        // finds the folder free for the next read.
                        drop(done);
                        let _ = sender.send(Some(setup));
                    });
                if worker.is_err() {
                    // The closure, and the read it held, went with the thread that did not start.
                    return Setup::late();
                }
                (answer, true)
            }
            Turn::Share(answer) => (answer, true),
            Turn::Wait(answer) => (answer, false),
        };
        let got = waited
            .bound(async {
                answered
                    .wait_for(Option::is_some)
                    .await
                    .ok()
                    .and_then(|got| got.clone())
            })
            .await;
        match (got, same) {
            (Some(Some(setup)), true) => return setup,
            // The read of another folder has ended: ask again, after a turn of the executor's, so
            // that an ask never spins on a read that is over and is still shown as in flight.
            (Some(_), false) => give_a_turn().await,
            _ => return Setup::late(),
        }
    }
}

/// The providers whose credential variable `env` sets to something, by name ([`provider_of`]).
fn keys_of(env: &Env) -> Vec<String> {
    sorted(
        CREDENTIALS
            .into_iter()
            .filter(|name| env.os(name).is_some_and(|value| !value.is_empty()))
            .map(provider_of)
            .collect::<BTreeSet<_>>()
            .into_iter(),
    )
}

impl Setup {
    /// The setup of the Pi `env` names (`PI_CODING_AGENT_DIR`, else `.pi/agent` in the home).
    /// A file that is not there names nothing and one that cannot be read is said so
    /// ([`Setup::unread`]); no file is written.
    pub(crate) fn read(env: &Env) -> Self {
        #[cfg(test)]
        {
            let probe = probe::current();
            probe.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(hook) = probe.before {
                hook();
            }
        }
        let keys = keys_of(env);
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
            Got::Unread => unread.push("the file of logins".to_owned()),
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
            late: false,
        }
    }

    /// A setup nothing is known of: the folder was not read in time ([`inspect`]).
    fn late() -> Self {
        Self {
            late: true,
            ..Self::default()
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
        if self.late {
            return "Pi lists no model, and Pi's folder could not be read in time: it cannot say \
                    whether a provider is set up there (a file of it did not open)"
                .to_owned();
        }
        if self.is_empty() {
            // No name of a variable, of a file of logins or of a key: a line that says one is a
            // line the engine withholds (`cf_engine::quotable`), and this is the commonest
            // message of all.
            return "Pi lists no model: no provider is set up (no login, no provider in \
                    models.json, no installed package or extension, no provider key in the \
                    environment). Run pi and sign in with /login, or set the variable of a \
                    provider's key in the environment, or add a provider to models.json (Pi's \
                    providers.md and models.md say how)"
                .to_owned();
        }
        let parts: Vec<String> = [
            ("a login for", &self.logins),
            ("custom providers", &self.custom),
            ("packages", &self.packages),
            ("extensions", &self.extensions),
            ("a key in the environment for", &self.keys),
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
