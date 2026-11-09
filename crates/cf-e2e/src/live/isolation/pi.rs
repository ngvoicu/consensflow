//! What a run's Pi is given of the owner's `auth.json`: the entries of the providers its
//! model needs, and only the ones that cannot be refreshed in the run.
//!
//! Pi keeps one credential per provider in `<agent dir>/auth.json`: an API key
//! (`{"type": "api_key", "key": …}`) or an OAuth login (`{"type": "oauth", "access": …,
//! "refresh": …, "expires": <ms since the epoch>}`). Pi uses an OAuth entry as it is
//! until `expires` has passed, then exchanges the refresh token for new ones and writes
//! them back. A refresh token is used once: if the run's copy refreshes, the owner's
//! file holds a token the provider no longer honours, and they are logged out of that
//! provider the next time they use it (T-603, T-696: a live run refreshed an expired
//! `openai` entry of the owner's). So an OAuth entry is copied only if it lasts as long
//! as the run does and a margin; one that does not is a reason to refuse the run, said
//! in words, and never a copy that would refresh.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use super::super::login::span;
use super::super::renew::{self, Renewal, TestMachine};
use super::watch::Seen;
use crate::{Error, Result};

/// The entry `entry`, the owner's, as a run is given it: all but its refresh token. A copy that
/// holds only the access token cannot renew it, so it cannot use up the refresh token the owner's
/// file keeps (T-1067, measured on zeewin2: Pi 1.4.0 runs on such a copy, and where less than five
/// minutes of the access token are left it tries to renew, is refused (`invalid_grant`) and fails
/// the run loudly, the owner's file untouched).
fn without_renewal(entry: &Value) -> Value {
    let mut copy = entry.clone();
    if let Some(fields) = copy.as_object_mut() {
        fields.remove("refresh");
    }
    copy
}

/// The provider a Pi model name is on: what comes before the first `/`
/// (`openrouter/meta/muse-spark-1.3` is on `openrouter`). A name with no `/` leaves it to
/// Pi to find the provider, which is none.
pub fn provider_of(model: &str) -> Option<String> {
    let (provider, _) = model.split_once('/')?;
    (!provider.is_empty()).then(|| provider.to_owned())
}

/// Why the owner's entry of a provider is not one a run can use.
enum Unfit {
    /// There is none.
    Missing,
    /// It is neither an API key nor an OAuth login Pi knows.
    Unknown,
    /// An OAuth login that is missing its access token or its expiry.
    Incomplete,
    /// An OAuth login that has ended.
    Expired,
    /// An OAuth login that ends in the run: it lasts this much more.
    Short(Duration),
}

impl Unfit {
    fn said(&self) -> String {
        match self {
            Self::Missing => "has no entry".to_owned(),
            Self::Unknown => "has an entry of a kind Pi does not know".to_owned(),
            Self::Incomplete => "has an OAuth login with no access token or expiry".to_owned(),
            Self::Expired => "has expired".to_owned(),
            Self::Short(left) => format!("expires in {}", span(*left)),
        }
    }
}

/// When the OAuth `entry` ends: its `expires`, milliseconds since the epoch.
fn expires(entry: &Value) -> Option<i64> {
    entry["expires"]
        .as_i64()
        // Pi writes a number, which JSON may have given as a float.
        .or_else(|| entry["expires"].as_f64().map(|expires| expires as i64))
}

/// When the OAuth entry of `provider` in `text`, the owner's `auth.json`, ends; none where it
/// holds none.
pub fn expiry(text: &str, provider: &str) -> Option<i64> {
    expires(serde_json::from_str::<Value>(text).ok()?.get(provider)?)
}

/// Whether the entry of `provider` in `text` has run short of `left` from `now`, and Pi could
/// renew it: an OAuth login with a refresh token, that has ended or ends within `left`. An entry
/// that is a key, that is missing or unknown, or that has no refresh token is not for a renewal
/// to mend.
pub fn renewable(text: &str, provider: &str, now: i64, left: Duration) -> bool {
    let Some(held) = serde_json::from_str::<Value>(text).ok() else {
        return false;
    };
    let entry = held.get(provider);
    let has_refresh = entry
        .and_then(|entry| entry["refresh"].as_str())
        .is_some_and(|token| !token.is_empty());
    let near = expiry(text, provider).is_none_or(|expires| {
        Duration::from_millis(u64::try_from(expires - now).unwrap_or(0)) <= renew::RENEWS_WITHIN
    });
    has_refresh && near && matches!(fit(entry, now, left), Err(Unfit::Expired | Unfit::Short(_)))
}

/// Whether the entry can be copied for a run that needs it to last `left` from `now`
/// (milliseconds since the epoch).
fn fit(entry: Option<&Value>, now: i64, left: Duration) -> std::result::Result<(), Unfit> {
    let entry = entry.ok_or(Unfit::Missing)?;
    match entry["type"].as_str() {
        // A key does not end, and nothing refreshes it.
        Some("api_key") => Ok(()),
        Some("oauth") => {
            let access = entry["access"]
                .as_str()
                .is_some_and(|text| !text.is_empty());
            let expires = expires(entry);
            // The refresh token is not asked for: OpenRouter's login is an API key kept as an OAuth
            // entry with an empty one, that ends at the largest number there is. What matters is
            // whether Pi finds the entry ended in the run, and then refreshes it.
            let (true, Some(expires)) = (access, expires) else {
                return Err(Unfit::Incomplete);
            };
            if expires <= now {
                return Err(Unfit::Expired);
            }
            let remaining = Duration::from_millis(u64::try_from(expires - now).unwrap_or(0));
            if remaining < left {
                return Err(Unfit::Short(remaining));
            }
            Ok(())
        }
        _ => Err(Unfit::Unknown),
    }
}

/// The part of `text`, the owner's `auth.json` (said as `from` in what is said), that a run
/// is given: the entries of `providers` (all the providers there are, where the run's model
/// names none), each only if it lasts `left` from `now`. An error says why the run cannot
/// have a login, naming the providers; never what the text held.
pub fn login(
    text: &str,
    providers: Option<&[String]>,
    now: i64,
    left: Duration,
    from: &str,
) -> Result<Value> {
    let not_usable =
        |why: &str| Error::Daemon(format!("Pi's login in {from} cannot be used: {why}"));
    let held: Value = serde_json::from_str(text).map_err(|_| not_usable("it is not JSON"))?;
    let held = held
        .as_object()
        .ok_or_else(|| not_usable("it is not a table of providers"))?;
    let wanted: Vec<&str> = match providers {
        Some(providers) => providers.iter().map(String::as_str).collect(),
        None => held.keys().map(String::as_str).collect(),
    };
    let (mut copied, mut unfit) = (Map::new(), Vec::new());
    for provider in wanted {
        match fit(held.get(provider), now, left) {
            Ok(()) => {
                copied.insert(provider.to_owned(), without_renewal(&held[provider]));
            }
            Err(why) => unfit.push(format!("`{provider}` {}", why.said())),
        }
    }
    // A run on a model whose provider is not known uses any login that is fit; one on a
    // named provider needs that provider's.
    let refused = if providers.is_some() {
        !unfit.is_empty()
    } else {
        copied.is_empty()
    };
    if !refused {
        return Ok(Value::Object(copied));
    }
    let who = if unfit.is_empty() {
        "it holds no login".to_owned()
    } else {
        unfit.join("; ")
    };
    Err(Error::Daemon(format!(
        "Pi's login in {from} cannot be used for this run: {who}. The run needs a login that lasts at \
         least {} more (its own length and a margin): a run's Pi refreshes an OAuth login that has ended, \
         and the refresh would leave the login this machine keeps unusable (the owner is logged out of \
         that provider the next time they use it). Pi renews an entry itself once it has ended: run again \
         then (use Pi once on that provider; on a test machine the kit asks it to), run on a model of a provider whose login lasts or is a key \
         (--models pi=…), or make the run shorter",
        span(left)
    )))
}

/// Pi's renewal on `machine`, for the owner whose home is `home` and whose Pi folder is `agent`,
/// started as `program` finds it: Pi run once on the model the run uses ([`renew::pi_words`]),
/// and the owner's files compared before and after, so that nothing but `auth.json` is changed.
pub fn renewal(
    machine: TestMachine,
    home: &Path,
    agent: &Path,
    model: &str,
    program: impl Fn() -> Result<PathBuf> + Send + Sync + 'static,
) -> Renewal {
    let (home, agent, model) = (home.to_path_buf(), agent.to_path_buf(), model.to_owned());
    Renewal::new(machine, "Pi", "only once the entry has ended", move || {
        let program = program()?;
        let files: Vec<_> = ["settings.json", "models.json", "mcp-auth.json"]
            .map(|name| agent.join(name))
            .into();
        let records = [agent.join("sessions")];
        let before = Seen::look(&files, &records);
        let ended = renew::one_shot(&program, &renew::pi_words(&model))?;
        let changed = before.changes_to(&Seen::look(&files, &records), &home);
        if changed.is_empty() {
            return Ok(ended);
        }
        Err(Error::Daemon(format!(
            "it changed the owner's Pi files beside its login: {}",
            changed.join("; ")
        )))
    })
}

/// What a provider that the agent folder gives Pi, besides its `auth.json` entry, needs of a
/// run's home ([`super::Seed::PiProvider`]): the package `settings.json`'s `packages` loads it
/// by, under whichever source Pi installed it from ([`Source`]), and the files of the folder it
/// keeps. `cliproxyapi` is the only one the kit knows; a provider with no entry here
/// (OpenRouter) is a login alone, which [`login`] copies.
#[derive(Debug)]
pub struct Provider {
    /// The provider's name, as `auth.json` keys it and a model names it before its `/`.
    pub name: &'static str,
    /// The package's own name (its `package.json`'s `name`), which an `npm:` spec names it by.
    pub package: &'static str,
    /// The repository a `git:` source of the package ends with (a fork's is named the same).
    pub repository: &'static str,
    /// The file the connection is read from: required, and its `baseUrl` is the proxy a run
    /// asks whether it answers ([`answers`]).
    pub config: &'static str,
    /// More files of the provider's that must be there.
    pub required: &'static [&'static str],
    /// The ones copied where the owner has them: the models' file is the proxy's cache of its
    /// last answer, which the proxy refills where it is missing.
    pub optional: &'static [&'static str],
}

/// Where Pi has a provider's package, as the agent folder's `packages` names it: the entry's
/// source, as written (a run's `settings.json` names the same), and the folder below the agent
/// folder Pi installs that source in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The source as `packages` names it (`npm:@scope/name`, `git:host/path@ref`).
    pub spec: String,
    /// The package's folder below the agent folder: `npm/node_modules/<name>` for an `npm:`
    /// spec, `git/<host>/<path>` for a `git:` one (the ref is the checkout's, not the folder's).
    pub folder: PathBuf,
    /// Whether npm installed it, so that npm's manifests in the agent folder record it.
    pub npm: bool,
    /// The entry of `packages` as the owner wrote it (a string, or an object with its `source`
    /// and the resources it picks): the run's `settings.json` names the package by the same
    /// entry, or Pi would load every resource the package lists.
    pub entry: Value,
}

impl Provider {
    /// The source of `packages` (the agent folder's `settings.json`'s, each a string or an
    /// object's `source`) that names this provider's package: an `npm:` spec of its name, with or
    /// without a version, or a `git:` source of a repository of its name, `host/path` with or
    /// without `@ref`. None where no entry does, or only in a form the kit does not know where
    /// Pi keeps (a URL, a local path): the run is then refused.
    pub fn source_in(&self, packages: &[Value]) -> Option<Source> {
        packages.iter().find_map(|entry| {
            let spec = entry.as_str().or_else(|| entry["source"].as_str())?;
            let mut source = self.source_of(spec)?;
            source.entry = entry.clone();
            Some(source)
        })
    }

    /// The version an `npm:` spec of this package asks, as written after its name: none where it
    /// asks none (`npm:@scope/name`).
    pub fn npm_version<'a>(&self, spec: &'a str) -> Option<&'a str> {
        let named = spec.strip_prefix("npm:")?.strip_prefix(self.package)?;
        named
            .strip_prefix('@')
            .filter(|version| !version.is_empty())
    }

    fn source_of(&self, spec: &str) -> Option<Source> {
        if let Some(named) = spec.strip_prefix("npm:") {
            let name = match named.strip_prefix('@') {
                Some(scoped) => format!("@{}", scoped.split('@').next()?),
                None => named.split('@').next()?.to_owned(),
            };
            return (name == self.package).then(|| Source {
                spec: spec.to_owned(),
                folder: Path::new("npm/node_modules").join(self.package),
                npm: true,
                entry: Value::from(spec),
            });
        }
        let rest = spec.strip_prefix("git:")?;
        // `host/path@ref`: the ref follows the first `@` after the host (a ref may hold `/`). A
        // host that is none, a folder (`.`, `..`), a URL's scheme or holds a user is no form the
        // kit knows Pi's folder of.
        let (host, path) = rest.split_once('/')?;
        if host.is_empty() || host.starts_with('.') || host.contains([':', '@']) {
            return None;
        }
        let path = path.split('@').next()?;
        let repository = path.rsplit('/').next()?.trim_end_matches(".git");
        if repository != self.repository {
            return None;
        }
        let mut folder = PathBuf::from("git").join(host);
        for part in path.trim_end_matches(".git").split('/') {
            if part.is_empty() || part == "." || part == ".." {
                return None;
            }
            folder.push(part);
        }
        Some(Source {
            spec: spec.to_owned(),
            folder,
            npm: false,
            entry: Value::from(spec),
        })
    }

    /// The source the agent folder `agent`'s `settings.json` names this provider's package by,
    /// read as [`Provider::source_in`] reads it: none where the file or the entry is not there.
    pub fn source(&self, agent: &Path) -> Option<Source> {
        let settings: Value =
            serde_json::from_slice(&fs::read(agent.join("settings.json")).ok()?).ok()?;
        self.source_in(settings["packages"].as_array()?)
    }

    /// The files of `agent` a run on this provider is held not to change: its own, the
    /// package's every file where `settings.json` names it, and npm's manifests where npm
    /// installed it (they record the install).
    pub fn watched(&self, agent: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = [self.config]
            .into_iter()
            .chain(self.required.iter().copied())
            .chain(self.optional.iter().copied())
            .map(|name| agent.join(name))
            .collect();
        if let Some(source) = self.source(agent) {
            if source.npm {
                files.extend(
                    ["npm/package.json", "npm/package-lock.json"].map(|name| agent.join(name)),
                );
            }
            files.extend(below(&agent.join(&source.folder)));
        }
        files
    }
}

/// What the npm install `package` of `source` is, in words, where it is the version the spec
/// asks: a spec with no version is what is on disk; one with an exact version must be that one.
/// A range or a tag is refused, since Pi installs at its start when what is on disk does not
/// satisfy it, and the kit holds to what it copied. `Err` is why, in words.
pub fn npm_copy(
    provider: &Provider,
    source: &Source,
    package: &Path,
) -> std::result::Result<String, String> {
    let on_disk = fs::read(package.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|manifest| manifest["version"].as_str().map(str::to_owned))
        .ok_or_else(|| "has a package.json that names no version".to_owned())?;
    match provider.npm_version(&source.spec) {
        None => Ok(format!("npm, version {on_disk}")),
        Some(asked) if asked == on_disk => Ok(format!("npm, version {on_disk}")),
        Some(asked) if is_exact(asked) => Err(format!(
            "is version {on_disk}, and `{}` asks {asked}",
            source.spec
        )),
        Some(asked) => Err(format!(
            "is version {on_disk}, and `{}` asks `{asked}`, not an exact version, which the kit \
             cannot hold to what is on disk",
            source.spec
        )),
    }
}

/// Whether `version` is an exact one (`1.4.24`, `1.5.0-beta.1`), not a range or a tag.
fn is_exact(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// What the git checkout `package` is, in words (its commit, clean), where nothing in it is
/// changed or is a file of no commit: Pi loads the files as they are, and a checkout that is
/// not its commit's is bytes no source names. Read with `--no-optional-locks`, which writes
/// nothing in the owner's checkout. `Err` is why, in words.
pub fn git_copy(package: &Path) -> std::result::Result<String, String> {
    let git = |args: &[&str]| -> std::result::Result<String, String> {
        let ran = crate::process::Run::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(package)
            .args(args.iter().copied())
            .finding_programs()
            .inheriting_env()
            .run()
            .map_err(|why| format!("could not be read with git ({why})"))?;
        if ran.code != Some(0) {
            return Err(format!(
                "is no git checkout git can read ({})",
                ran.stderr.trim()
            ));
        }
        Ok(ran.stdout)
    };
    let commit = git(&["rev-parse", "HEAD"])?.trim().to_owned();
    let changed: Vec<String> = git(&["status", "--porcelain=v1", "--untracked-files=all"])?
        .lines()
        .map(|line| line.get(3..).unwrap_or(line).to_owned())
        .collect();
    if !changed.is_empty() {
        return Err(format!(
            "is not clean at commit {commit}: {} differ from it",
            changed.join(", ")
        ));
    }
    Ok(format!("a git checkout at commit {commit}, clean"))
}

/// [`Provider`] of the provider `name`, where the kit knows the agent folder gives it to Pi:
/// `cliproxyapi`, which Pi 1.5.0 loads as the extension package `settings.json`'s `packages`
/// names (`@router-for-me/pi-cliproxyapi-provider` from npm, T-1172, or a git checkout of a
/// repository of that name, the owner's Mac's since 2026-11-09, T-1358), whose extensions read
/// `cliproxyapi.json` for the proxy's base URL and key and `cliproxyapi-models.json` for the
/// models it last served, with the login in `auth.json`.
pub fn provider(name: &str) -> Option<&'static Provider> {
    const CLIPROXYAPI: Provider = Provider {
        name: "cliproxyapi",
        package: "@router-for-me/pi-cliproxyapi-provider",
        repository: "pi-cliproxyapi-provider",
        config: "cliproxyapi.json",
        required: &[],
        optional: &["cliproxyapi-models.json"],
    };
    (name == CLIPROXYAPI.name).then_some(&CLIPROXYAPI)
}

/// Every file below `folder`, folders walked into.
fn below(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                files.extend(below(&path));
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// The proxy's base URL of `config`'s bytes, the provider's `cliproxyapi.json`: its `baseUrl`,
/// or the extension's own default where it names none (the extension's own chain, less its
/// environment variables, which a run's window never gets: the copied config answers there).
pub fn base_url(config: &[u8]) -> String {
    serde_json::from_slice::<Value>(config)
        .ok()
        .and_then(|config| config["baseUrl"].as_str().map(str::to_owned))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8317".to_owned())
}

/// `url` as a run may say it: its scheme, host, port and path, and nothing of it that can hold a
/// secret (a user and password, a query, a fragment).
pub fn shown_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "a URL with no scheme".to_owned();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    format!("{scheme}://{}{path}", without_user(authority))
}

/// The authority `authority` without the user and password before its `@`.
fn without_user(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

/// Whether the proxy `base_url` answers, asked before anything is made for a run on the
/// provider: an `http` URL gets a `GET /v1/models`, which the proxy answers even a keyless
/// request to (a `401` is an answer); a URL of another scheme is asked for a listener alone.
/// `Err` is why it does not, in words; never the key.
pub fn answers(base_url: &str) -> std::result::Result<(), String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let Some((scheme, rest)) = base_url.split_once("://") else {
        return Err(format!("no scheme in {}", shown_url(base_url)));
    };
    let authority = without_user(rest.split(['/', '?', '#']).next().unwrap_or_default());
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().unwrap_or(0)),
        None => (
            authority,
            match scheme {
                "https" => 443,
                _ => 80,
            },
        ),
    };
    if host.is_empty() || port == 0 {
        return Err(format!("no host:port in {}", shown_url(base_url)));
    }
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|why| format!("{host}:{port} does not resolve ({why})"))?
        .next()
        .ok_or_else(|| format!("{host}:{port} does not resolve"))?;
    let mut stream =
        TcpStream::connect_timeout(&address, ANSWERS).map_err(|why| format!("{why}"))?;
    stream
        .set_read_timeout(Some(ANSWERS))
        .and_then(|()| stream.set_write_timeout(Some(ANSWERS)))
        .map_err(|why| format!("{why}"))?;
    if scheme != "http" {
        return Ok(());
    }
    stream
        .write_all(
            format!("GET /v1/models HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|why| format!("{why}"))?;
    let mut head = [0_u8; 64];
    let read = stream.read(&mut head).map_err(|why| format!("{why}"))?;
    match std::str::from_utf8(&head[..read]).map(|text| text.starts_with("HTTP/")) {
        Ok(true) => Ok(()),
        _ => Err("it answers with no HTTP".to_owned()),
    }
}

/// How long the proxy is given to answer, before the run is refused.
const ANSWERS: Duration = Duration::from_secs(5);
