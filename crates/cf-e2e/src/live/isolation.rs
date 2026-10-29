//! The homes the real harnesses of a live run start on, and what keeps the run out of the
//! owner's own: a folder of the run's own that holds a copy of the login and nothing else
//! of the owner's, the variables that make a harness read and write there, and a look at
//! the owner's setup before and after, which fails the run if it differs.
//!
//! A rig clears the environment, and a harness reads its login by what the rig sets: Pi
//! reads `~/.pi/agent` unless `PI_CODING_AGENT_DIR` says another, and no case set it, so a
//! live run on Pi used the owner's folder, and Pi refreshed their expired `openai` entry
//! (T-696, 2026-10-26). The kit does it, not the case: [`Isolation`] is made for every
//! harness [`installed`](super::installed) finds, and the rig sets its variables **last**
//! ([`HarnessHome`], [`crate::rig::Config::isolated`]), over everything else it set and over
//! every variable a case gave it.
//!
//! | Harness | Variables | Copied from the owner's | Watched in the owner's |
//! |---|---|---|---|
//! | Claude Code | `CLAUDE_CONFIG_DIR` ([`super::login`]) | the Keychain's login, with 2 h of token | `settings.json`, `.claude.json`'s projects, `history.jsonl`, `projects/`, the Keychain's items |
//! | Codex | `CODEX_HOME` | `auth.json` | `auth.json`, `config.toml` |
//! | Pi | `PI_CODING_AGENT_DIR` | the entries of `auth.json` the run's model needs and that last the run and a margin | `auth.json`, `settings.json`, `models.json`, `mcp-auth.json`, `sessions/` |
//! | Devin | `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` | `credentials.toml`, `config.json` | those two and `trusted_workspaces.json` |
//!
//! **Muse Code** (`wt/muse-harness`, not on this base) is one more row for the day it
//! lands: `XDG_DATA_HOME` and `XDG_CONFIG_HOME` scratch folders with the login reachable
//! read-only ([`Seed::Reach`] is how Devin's reaches the other tools' folders), and
//! `MUSE_NO_AUTO_UPDATE=1`, `MUSE_LOGIN=0` as plain values beside those folders, in the plan
//! of a `Harness::Muse` arm of [`Isolation::of_machine_lasting`]. **OpenCode**, which a later change takes out of
//! ConsensFlow, keeps running on the owner's folders: Devin's scratch reaches them.
//!
//! **A login that refreshes.** A refresh token is used once, so a copy that refreshes leaves
//! the owner's file with a token that no longer works. Pi refreshes an OAuth entry that has
//! ended, so the run is given an entry only if it lasts the run's length and [`MARGIN`]
//! ([`pi::login`]); one that does not is a refusal, in words, before anything is spent. Codex
//! and Devin keep a login that a run does not outlast (Devin's is a key); what Codex rewrote
//! is said when the run ends. At the end the copies are compared with what was copied, and
//! the owner's files with their hashes: a difference fails the run, naming the file, and
//! nothing a login holds is said anywhere.
//!
//! **`HOME` stays the person's.** The windows are given the person's home all the same, and none
//! of the variables above is derived from it: their project folders are below it
//! (`~/.consensflow-candidate/live/…`, where a harness's question about trusting a folder is asked
//! once and kept), and a rig's own home would put them under the temporary folder, which macOS
//! spells through the link `/var` (`/private/var`). Devin's window opened in such a folder shows no
//! answer (its TUI and its agent disagree about the folder, 2026-10-27, Devin 3000.11.92 and
//! 3000.11.205): `live paste devin` failed on it three times and passed on the same directory
//! spelled `/private/var/…`. A harness that wrote in `HOME` anyway is found by the hashes.
//!
//! **A case that is its own home.** A case that gives its windows a `HOME` of its own, with
//! the harness's folder in it ([`Installed::on_own_home`]), is none of this kit's: it reads
//! and writes nothing of the owner's.

mod pi;
mod watch;

#[cfg(test)]
mod tests;

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

use super::agents;
use super::login::{private_folder, sweep, write_private};
use super::machine::Harness;
use crate::process::own_var;
use crate::rig::HarnessHome;
use crate::{files, Error, Result};
use watch::{shown, Seen};

/// How long the longest live suite (the approval run) takes: a login must last that much
/// and [`MARGIN`] for a live suite to start on a copy of it.
pub const RUN: Duration = Duration::from_secs(90 * 60);

/// What a login must last beyond the run: the end of the run (the last look, the app's
/// close) and the time the owner's own use of the harness needs to refresh it first.
pub const MARGIN: Duration = Duration::from_secs(30 * 60);

/// What the name of a run's home begins with, in the temporary folder: the id of the process
/// that made it follows, so that one a killed run left behind is known to be nobody's.
const PREFIX: &str = "consensflow-live-home-";

/// What the run's home is given.
#[derive(Debug, Clone)]
enum Seed {
    /// A file of the owner's, copied: a login if `login` (then it must be there, and a
    /// change in the copy is a refresh), a config otherwise (copied if there).
    Copy {
        from: PathBuf,
        to: PathBuf,
        login: bool,
    },
    /// The owner's `auth.json` of Pi, with the entries of `providers` that last `left`.
    PiLogin {
        from: PathBuf,
        to: PathBuf,
        providers: Option<Vec<String>>,
        left: Duration,
    },
    /// A link in `to` to each entry of the owner's folder `from` but the one named `except`:
    /// what other programs of the rig keep in a folder the variables move (OpenCode's login
    /// in `XDG_DATA_HOME`), left as it is for them. Where the folders are links' (Devin's, on Unix).
    #[cfg(unix)]
    Reach {
        from: PathBuf,
        to: PathBuf,
        except: &'static str,
    },
}

/// What a harness's home is made of, and what is watched of the owner's.
#[derive(Debug, Clone)]
struct Plan {
    /// The folders below the run's home made empty.
    folders: Vec<PathBuf>,
    /// The variables the windows are given, each the folder below the run's home it names.
    vars: Vec<(&'static str, PathBuf)>,
    seeds: Vec<Seed>,
    /// The owner's files hashed before and after.
    watched: Vec<PathBuf>,
    /// The owner's folders of records, in which no entry may name a folder of the run.
    records: Vec<PathBuf>,
    /// Where the login is copied from, as a run says it.
    described: String,
}

impl Plan {
    /// Codex: `CODEX_HOME`, holding a copy of `auth.json` and nothing else of the owner's,
    /// which a check writes its own configuration into. A run on the owner's saves a trust
    /// table for its folder in their `config.toml` and its threads among theirs.
    fn codex(owner: &Path) -> Self {
        Self {
            folders: Vec::new(),
            vars: vec![("CODEX_HOME", "codex".into())],
            seeds: vec![Seed::Copy {
                from: owner.join("auth.json"),
                to: "codex/auth.json".into(),
                login: true,
            }],
            watched: vec![owner.join("auth.json"), owner.join("config.toml")],
            records: Vec::new(),
            described: owner.join("auth.json").display().to_string(),
        }
    }

    /// Pi: `PI_CODING_AGENT_DIR`, holding the entries of `auth.json` that the model of the run
    /// needs (`providers`) and that last `left`, and nothing else of the owner's. Pi writes its
    /// sessions, its settings and its MCP logins there.
    fn pi(owner: &Path, providers: Option<Vec<String>>, left: Duration) -> Self {
        Self {
            folders: Vec::new(),
            vars: vec![("PI_CODING_AGENT_DIR", "pi/agent".into())],
            seeds: vec![Seed::PiLogin {
                from: owner.join("auth.json"),
                to: "pi/agent/auth.json".into(),
                providers,
                left,
            }],
            watched: ["auth.json", "settings.json", "models.json", "mcp-auth.json"]
                .map(|name| owner.join(name))
                .into(),
            records: vec![owner.join("sessions")],
            described: owner.join("auth.json").display().to_string(),
        }
    }

    /// Devin: the four folders of the XDG layout, where it keeps its login and config (and its
    /// sessions, logs and cache); the login (`credentials.toml`, an API key) and `config.json`
    /// are copied. Whatever else the owner keeps in the config and data folders is reached
    /// through a link, for the other programs of the rig that read it there.
    #[cfg(unix)]
    fn devin(config: &Path, data: &Path) -> Self {
        Self {
            folders: ["config", "data", "state", "cache"]
                .map(PathBuf::from)
                .into(),
            vars: vec![
                ("XDG_CONFIG_HOME", "config".into()),
                ("XDG_DATA_HOME", "data".into()),
                ("XDG_STATE_HOME", "state".into()),
                ("XDG_CACHE_HOME", "cache".into()),
            ],
            seeds: vec![
                Seed::Copy {
                    from: data.join("devin").join("credentials.toml"),
                    to: "data/devin/credentials.toml".into(),
                    login: true,
                },
                Seed::Copy {
                    from: config.join("devin").join("config.json"),
                    to: "config/devin/config.json".into(),
                    login: false,
                },
                Seed::Reach {
                    from: config.to_path_buf(),
                    to: "config".into(),
                    except: "devin",
                },
                Seed::Reach {
                    from: data.to_path_buf(),
                    to: "data".into(),
                    except: "devin",
                },
            ],
            watched: vec![
                data.join("devin").join("credentials.toml"),
                config.join("devin").join("config.json"),
                data.join("devin")
                    .join("cli")
                    .join("trusted_workspaces.json"),
            ],
            records: Vec::new(),
            described: data
                .join("devin")
                .join("credentials.toml")
                .display()
                .to_string(),
        }
    }
}

/// A file of the run's home that holds a login, as it was copied.
struct Copied {
    file: PathBuf,
    was: Vec<u8>,
}

impl Copied {
    /// Whether the file is as it was copied: the same bytes, or the same JSON (a harness may
    /// write a login again, with nothing in it changed).
    fn unchanged(&self) -> bool {
        let Ok(now) = fs::read(&self.file) else {
            return false;
        };
        if now == self.was {
            return true;
        }
        match (
            serde_json::from_slice::<Value>(&now),
            serde_json::from_slice::<Value>(&self.was),
        ) {
            (Ok(now), Ok(was)) => now == was,
            _ => false,
        }
    }
}

/// The home of a run, once it is made.
struct Made {
    /// The folder, removed when this is dropped: with the run, however it ends.
    folder: TempDir,
    /// The logins in it, as they were copied.
    copied: Vec<Copied>,
    /// The owner's setup when it was made.
    owner: Seen,
}

/// The home a harness's windows run on for a run: made when the rig first asks for it, with a
/// copy of the owner's login, and removed when the last holder of this is dropped.
pub struct Isolation {
    harness: Harness,
    /// The owner's home, which a run says its paths below as `~`.
    owner_home: PathBuf,
    plan: Plan,
    /// Where the home is made.
    parent: PathBuf,
    made: Mutex<Option<Made>>,
}

impl Isolation {
    /// The home of `harness` for a live suite ([`RUN`]), copying the login of the owner whose
    /// home is `home` (Codex's: its own folder, `CODEX_HOME`); none for Claude Code, whose config
    /// folder is [`super::login`]'s, for OpenCode (the owner's), and where the platform's layout
    /// is not known (Devin's on Windows).
    pub fn of_machine(harness: Harness, home: &Path) -> Result<Option<Self>> {
        Self::of_machine_lasting(harness, home, RUN)
    }

    /// [`Isolation::of_machine`], for a run that takes `run` (an eval states its own length): a
    /// login that would end before it and [`MARGIN`] is refused when the home is made.
    pub fn of_machine_lasting(
        harness: Harness,
        home: &Path,
        run: Duration,
    ) -> Result<Option<Self>> {
        let machine = |name: &str| own_var(name).filter(|value| !value.is_empty());
        // Codex's home is its own folder; the owner's home, which a run says its paths below as
        // `~`, is the user's.
        let user = super::machine::home().unwrap_or_else(|_| home.to_path_buf());
        Ok(match harness {
            Harness::Codex => Some(Self::codex(&user, home)),
            Harness::Pi => {
                let agent = machine("PI_CODING_AGENT_DIR")
                    .map_or_else(|| home.join(".pi").join("agent"), PathBuf::from);
                let model = agents::asked()?.of(Harness::Pi).name.clone();
                Some(Self::pi(home, &agent, &model, run))
            }
            #[cfg(unix)]
            Harness::Devin => {
                let base = |variable: &str, below: &[&str]| {
                    machine(variable).map_or_else(
                        || {
                            below
                                .iter()
                                .fold(home.to_path_buf(), |path, part| path.join(part))
                        },
                        PathBuf::from,
                    )
                };
                Some(Self::devin(
                    home,
                    &base("XDG_CONFIG_HOME", &[".config"]),
                    &base("XDG_DATA_HOME", &[".local", "share"]),
                ))
            }
            _ => None,
        })
    }

    /// Codex's home, copying the login in `codex_home`, the owner's `CODEX_HOME` (in the home
    /// `home`).
    pub fn codex(home: &Path, codex_home: &Path) -> Self {
        Self::new(Harness::Codex, home, Plan::codex(codex_home))
    }

    /// Pi's home, copying the login in `agent`, the owner's Pi folder (in the home `home`), that
    /// the Pi model `model` needs, if it lasts a run of `run` and [`MARGIN`].
    pub fn pi(home: &Path, agent: &Path, model: &str, run: Duration) -> Self {
        let providers = pi::provider_of(model).map(|provider| vec![provider]);
        Self::new(Harness::Pi, home, Plan::pi(agent, providers, run + MARGIN))
    }

    /// Devin's home, copying the login in the owner's data folder `data` and the config in `config`
    /// (the XDG folders of the owner whose home is `home`).
    #[cfg(unix)]
    pub fn devin(home: &Path, config: &Path, data: &Path) -> Self {
        Self::new(Harness::Devin, home, Plan::devin(config, data))
    }

    fn new(harness: Harness, owner_home: &Path, plan: Plan) -> Self {
        Self {
            harness,
            owner_home: owner_home.to_path_buf(),
            plan,
            parent: std::env::temp_dir(),
            made: Mutex::new(None),
        }
    }

    /// The home, made as the clock says `now` is (milliseconds since the epoch): its path.
    fn made_at(&self, now: i64) -> Result<PathBuf> {
        let mut made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(made) = made.as_ref() {
            return Ok(made.folder.path().to_path_buf());
        }
        // Nothing is made before the login is known to be one that can be used.
        let seeds = self
            .plan
            .seeds
            .iter()
            .map(|seed| self.read(seed, now))
            .collect::<Result<Vec<_>>>()?;
        sweep(&self.parent, PREFIX);
        let folder = tempfile::Builder::new()
            .prefix(&format!("{PREFIX}{}-", std::process::id()))
            .tempdir_in(&self.parent)
            .map_err(|source| Error::File {
                action: "make a folder in",
                path: self.parent.clone(),
                source,
            })?;
        private_folder(folder.path())?;
        for relative in &self.plan.folders {
            files::make_dir(&folder.path().join(relative))?;
        }
        let mut copied = Vec::new();
        for seed in seeds.into_iter().flatten() {
            if let Some(login) = seed.place(folder.path())? {
                copied.push(login);
            }
        }
        let path = folder.path().to_path_buf();
        *made = Some(Made {
            folder,
            copied,
            owner: Seen::look(&self.plan.watched, &self.plan.records),
        });
        Ok(path)
    }

    /// What `seed` puts in the home, read from the owner's: none where there is nothing to put.
    fn read(&self, seed: &Seed, now: i64) -> Result<Option<Placed>> {
        match seed {
            Seed::Copy { from, to, login } => match fs::read(from) {
                Ok(bytes) => Ok(Some(Placed::File {
                    to: to.clone(),
                    bytes,
                    login: *login,
                })),
                Err(_) if !login => Ok(None),
                Err(failed) => Err(Error::Daemon(format!(
                    "{}'s login is not in {} ({failed}): {}",
                    self.harness.name(),
                    from.display(),
                    self.harness.log_in()
                ))),
            },
            Seed::PiLogin {
                from,
                to,
                providers,
                left,
            } => {
                let text = files::read_string(from).map_err(|failed| {
                    Error::Daemon(format!(
                        "Pi's login is not in {} ({failed}): {}",
                        from.display(),
                        self.harness.log_in()
                    ))
                })?;
                let entries = pi::login(
                    &text,
                    providers.as_deref(),
                    now,
                    *left,
                    &shown(from, &self.owner_home),
                )?;
                Ok(Some(Placed::File {
                    to: to.clone(),
                    bytes: entries.to_string().into_bytes(),
                    login: true,
                }))
            }
            #[cfg(unix)]
            Seed::Reach { from, to, except } => Ok(Some(Placed::Links {
                from: from.clone(),
                to: to.clone(),
                except,
            })),
        }
    }

    /// The file `name` below the folder the variable `variable` names (`CODEX_HOME`,
    /// `XDG_CONFIG_HOME`: where the harness reads what it is configured by), made with the
    /// folders it is in and the home if it is not, holding `text`: how a case that configures
    /// the harness does it, in the run's home and never in the owner's.
    pub fn write(&self, variable: &str, name: &str, text: &str) -> Result {
        let home = self.made_at(jiff::Timestamp::now().as_millisecond())?;
        let below = self
            .plan
            .vars
            .iter()
            .find(|(held, _)| *held == variable)
            .map(|(_, below)| below)
            .ok_or_else(|| {
                Error::Daemon(format!(
                    "{}'s windows run on no home for {variable}",
                    self.harness.name()
                ))
            })?;
        files::write(&home.join(below).join(name), text)
    }

    /// What a run that left the owner's setup as it was says it checked: the files, each with the
    /// first twelve digits of its hash; none where the home was never made (no run used it).
    fn evidence(&self) -> Option<String> {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        made.as_ref()?;
        let now = Seen::look(&self.plan.watched, &self.plan.records);
        Some(format!(
            "the owner's {} setup is as it was: {}; the copy of the login in the run's home is as it was copied",
            proper(self.harness),
            now.summary(&self.owner_home)
        ))
    }

    /// What the run did to the owner's setup, and what became of the copies of the login.
    fn findings(&self) -> (Vec<String>, Vec<String>) {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(made) = made.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let now = Seen::look(&self.plan.watched, &self.plan.records);
        let touched = made.owner.changes_to(&now, &self.owner_home);
        let refreshed = made
            .copied
            .iter()
            .filter(|login| !login.unchanged())
            .map(|login| {
                format!(
                    "the run's {} wrote a different login in the home's {}",
                    proper(self.harness),
                    login
                        .file
                        .strip_prefix(made.folder.path())
                        .unwrap_or(&login.file)
                        .display()
                )
            })
            .collect();
        (touched, refreshed)
    }
}

/// What a seed puts in the home.
enum Placed {
    File {
        to: PathBuf,
        bytes: Vec<u8>,
        login: bool,
    },
    #[cfg(unix)]
    Links {
        from: PathBuf,
        to: PathBuf,
        except: &'static str,
    },
}

impl Placed {
    /// Puts it in `home`; the login it is, if it is one.
    fn place(self, home: &Path) -> Result<Option<Copied>> {
        match self {
            Self::File { to, bytes, login } => {
                let file = home.join(to);
                if let Some(folder) = file.parent() {
                    files::make_dir(folder)?;
                }
                write_bytes(&file, &bytes)?;
                Ok(login.then_some(Copied { file, was: bytes }))
            }
            #[cfg(unix)]
            Self::Links { from, to, except } => {
                link_each(&from, &home.join(to), except)?;
                Ok(None)
            }
        }
    }
}

/// `bytes` in `file`, readable by its owner alone.
fn write_bytes(file: &Path, bytes: &[u8]) -> Result {
    match std::str::from_utf8(bytes) {
        Ok(text) => write_private(file, text),
        Err(_) => {
            // A login is text; a file that is not is copied as it is.
            let mut made = fs::File::create(file).map_err(Error::file("write", file))?;
            made.write_all(bytes).map_err(Error::file("write", file))
        }
    }
}

/// A link in `into` to each entry of `from` but `except`: the program that reads the folder
/// where the variables moved it finds what it had.
#[cfg(unix)]
fn link_each(from: &Path, into: &Path, except: &str) -> Result {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if entry.file_name() == except {
            continue;
        }
        let link = into.join(entry.file_name());
        std::os::unix::fs::symlink(entry.path(), &link).map_err(Error::file("link", &link))?;
    }
    Ok(())
}

impl fmt::Debug for Isolation {
    /// Which harness, where it is copied from and where, never what it holds.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner);
        formatter
            .debug_struct("Isolation")
            .field("harness", &self.harness)
            .field("from", &self.plan.described)
            .field("home", &made.as_ref().map(|made| made.folder.path()))
            .finish_non_exhaustive()
    }
}

impl Isolation {
    /// The variables of the harness's windows, below the home `home`.
    fn vars_in(&self, home: &Path) -> Vec<(String, String)> {
        self.plan
            .vars
            .iter()
            .map(|(name, below)| {
                (
                    (*name).to_owned(),
                    home.join(below).to_string_lossy().into_owned(),
                )
            })
            .collect()
    }
}

impl HarnessHome for Isolation {
    fn vars(&self) -> Result<Vec<(String, String)>> {
        let home = self.made_at(jiff::Timestamp::now().as_millisecond())?;
        Ok(self.vars_in(&home))
    }

    fn described(&self) -> String {
        self.plan.described.clone()
    }

    fn finish(&self) -> Result {
        let (touched, refreshed) = self.findings();
        if touched.is_empty() && refreshed.is_empty() {
            if let Some(evidence) = self.evidence() {
                let _ = writeln!(std::io::stderr(), "{evidence}");
            }
            return Ok(());
        }
        let said = said(self.harness, &touched, &refreshed);
        // The error is the case's to fail with; the banner is for whoever reads the output.
        let _ = writeln!(std::io::stderr(), "{said}");
        Err(Error::Daemon(said))
    }
}

/// The harness as its own words spell it.
const fn proper(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "Claude Code",
        Harness::OpenCode => "OpenCode",
        Harness::Pi => "Pi",
        Harness::Devin => "Devin",
        Harness::Codex => "Codex",
    }
}

/// What `finish` says when the run touched the owner's setup (`touched`) or its copy of the
/// login was written again (`refreshed`): loud, and with what to do. Never what a login holds.
fn said(harness: Harness, touched: &[String], refreshed: &[String]) -> String {
    let (name, bar) = (proper(harness), "!".repeat(78));
    let mut said = format!("\n{bar}\n");
    if !touched.is_empty() {
        said.push_str(&format!(
            "THE RUN CHANGED THE OWNER'S {} SETUP, which a live run on its own login is never to touch \
             (or the owner's own {name} did, while it ran):\n",
            name.to_uppercase()
        ));
        for change in touched {
            said.push_str(&format!("  - {change}\n"));
        }
    }
    if !refreshed.is_empty() {
        said.push_str(&format!(
            "THE RUN'S {} WROTE ITS COPY OF THE LOGIN AGAIN. A refresh token is used once, so the login \
             this machine keeps may now be stale, and {name} may ask the owner to log in again:\n",
            name.to_uppercase()
        ));
        for change in refreshed {
            said.push_str(&format!("  - {change}\n"));
        }
    }
    said.push_str(&bar);
    said
}
