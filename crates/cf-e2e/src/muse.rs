//! Muse Code as a case meets it from the outside: the machine's own install, which the
//! rig's windows run on Muse's built-in fake model (`--provider echo`: offline, no
//! login, no quota), and the session logs Muse writes, read as a case reads them.
//!
//! The rig puts a launcher of its own on its windows' `PATH` (`muse`) and beside it the
//! `.muse-version` of the machine's install and a `muse-bin-<version>` that runs the
//! machine's binary with `--provider echo`, as the product runs the binary a launcher
//! names. Echo refuses a model and an effort, so the stand-in takes them out of what it
//! runs, and writes down every command line it was given whole (`argv`), for a case to
//! hold the launch to. On a Mac it `exec`s Muse, so the window's process is Muse's, as the
//! product's own window's is. On Windows, which has no `exec`, it is this crate's
//! `muse-stand-in` program as `muse-bin-<version>.exe`, told what to run by the file
//! beside it ([`StandIn`]), and Muse is its child.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::process::own_var;
use crate::{files, Error, Result};

/// How the rig's Muse windows run: on Muse's fake model, each turn this many
/// milliseconds long, or live, on the model each is launched on, the person's login
/// found from their home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Echo { turn_ms: u64 },
    Live { real_home: PathBuf },
}

/// Gives a live rig's home what its Muse windows read their login by: settings of their
/// own, and a copy of the person's login (`auth.json`: on a Mac its metadata, the secret
/// being in the keychain, the one item every Muse of the machine reads; on Windows all of
/// it, `credential_backend: file`). The person's file is read, never written.
pub fn seed_live_home(rig_home: &Path, real_home: &Path) -> Result {
    let config = rig_home.join(".config").join("muse");
    files::write(&config.join("settings.json"), "{\"schema_version\":1}\n")?;
    let auth = real_home.join(".config").join("muse").join("auth.json");
    let text = files::read_string(&auth)?;
    files::write(&config.join("auth.json"), login_copy(&text)?)
}

/// The person's login as a live rig's home is given it: `text` as JSON, with every key whose
/// name has `refresh` in it taken out, at any depth. Muse's own on Windows holds an access token
/// and an API key and no refresh token, so nothing is taken out of it today; a Muse that keeps one
/// later is given a login that cannot use up the person's (the rule of T-1067 for Claude Code and
/// Pi). A file that is no JSON object cannot be shown free of one and is not copied.
pub fn login_copy(text: &str) -> Result<String> {
    let said = |why: String| {
        Error::Daemon(format!(
            "Muse's login is {why}: it cannot be shown free of a refresh token, so it is not copied"
        ))
    };
    let mut login: Value =
        serde_json::from_str(text).map_err(|failed| said(format!("no JSON ({failed})")))?;
    if !login.is_object() {
        return Err(said("no JSON object".to_owned()));
    }
    without_refresh(&mut login);
    let mut copy = serde_json::to_string_pretty(&login)
        .map_err(|failed| said(format!("not written again ({failed})")))?;
    copy.push('\n');
    Ok(copy)
}

/// `value` without the keys that name a refresh token, at any depth.
fn without_refresh(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !key.to_ascii_lowercase().contains("refresh"));
            map.values_mut().for_each(without_refresh);
        }
        Value::Array(items) => items.iter_mut().for_each(without_refresh),
        _ => {}
    }
}

/// The home of whoever runs the tests, where their Muse login is found: the profile on
/// Windows, `HOME` elsewhere.
pub fn real_home() -> std::result::Result<PathBuf, String> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    own_var(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("no {name} to find the Muse login in"))
}

/// The machine's own Muse: its binary and the version line naming it, and on Windows the
/// program the rig runs it through ([`Installed::through`]).
#[derive(Debug, Clone)]
pub struct Installed {
    pub binary: PathBuf,
    pub version: String,
    /// The rig's `muse-stand-in`, which a case names (`CARGO_BIN_EXE_muse-stand-in`):
    /// what the rig's Muse is on Windows.
    pub stand_in: Option<PathBuf>,
}

impl Installed {
    /// This install, the rig's Muse on Windows the program `stand_in`.
    #[must_use]
    pub fn through(mut self, stand_in: &str) -> Self {
        self.stand_in = Some(PathBuf::from(stand_in));
        self
    }
}

/// The Muse installed for whoever runs the tests where its installer puts it, or why
/// there is none to run: `~/.local/bin` on a Mac (`install.sh`), and
/// `%LOCALAPPDATA%\Programs\muse` on Windows (`install.ps1`). Muse ships for those two
/// alone, and CI's machines have none.
pub fn installed() -> std::result::Result<Installed, String> {
    let (bin, exe) = if cfg!(windows) {
        let local = own_var("LOCALAPPDATA").ok_or("no LOCALAPPDATA to find Muse Code in")?;
        (Path::new(&local).join("Programs").join("muse"), ".exe")
    } else if cfg!(target_os = "macos") {
        let home = own_var("HOME").ok_or("no home to find Muse Code in")?;
        (Path::new(&home).join(".local").join("bin"), "")
    } else {
        return Err("Muse Code's rig cases run on a Mac or on Windows with Muse installed: Muse ships for no other".to_owned());
    };
    let version = fs::read_to_string(bin.join(".muse-version"))
        .map(|line| line.trim().to_owned())
        .map_err(|_| {
            format!(
                "Muse Code is not installed on this machine (no {})",
                bin.join(".muse-version").display()
            )
        })?;
    let binary = bin.join(format!("muse-bin-{version}{exe}"));
    if !binary.is_file() {
        return Err(format!(
            "Muse Code's binary {} is not on this machine",
            binary.display()
        ));
    }
    Ok(Installed {
        binary,
        version,
        stand_in: None,
    })
}

/// What the rig's Muse on Windows runs, in the file beside it of its name and `.json`:
/// the machine's binary, where it writes down each command line, and how it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandIn {
    pub binary: PathBuf,
    pub argv: PathBuf,
    pub run: Run,
    /// The home of the rig whose window this is: where a live run's Muse keeps its settings,
    /// its login's copy and its sessions ([`StandIn::env`]).
    pub rig_home: PathBuf,
}

impl StandIn {
    /// What the program running reads beside itself.
    pub fn beside_this_program() -> std::result::Result<Self, String> {
        let me = std::env::current_exe().map_err(|error| error.to_string())?;
        let file = Self::file_of(&me);
        let text =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        Self::from_json(&text).ok_or_else(|| format!("{} says no stand-in", file.display()))
    }

    /// The file beside `program` that says what it runs.
    fn file_of(program: &Path) -> PathBuf {
        program.with_extension("json")
    }

    fn to_json(&self) -> Value {
        let run = match &self.run {
            Run::Echo { turn_ms } => json!({ "echo": turn_ms }),
            Run::Live { real_home } => json!({ "live": real_home }),
        };
        json!({ "binary": self.binary, "argv": self.argv, "run": run, "rig": self.rig_home })
    }

    fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let run = &value["run"];
        let run = match (run["echo"].as_u64(), run["live"].as_str()) {
            (Some(turn_ms), _) => Run::Echo { turn_ms },
            (None, Some(home)) => Run::Live {
                real_home: PathBuf::from(home),
            },
            (None, None) => return None,
        };
        Some(Self {
            binary: PathBuf::from(value["binary"].as_str()?),
            argv: PathBuf::from(value["argv"].as_str()?),
            run,
            rig_home: PathBuf::from(value["rig"].as_str()?),
        })
    }

    /// Writes down the command line `words`, as the Mac's stand-in writes it.
    pub fn note(&self, words: &[String]) -> std::io::Result<()> {
        use std::io::Write as _;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.argv)?
            .write_all(format!("{}\u{1e}", words.join("\u{1f}")).as_bytes())
    }

    /// Writes down the window's process, this program's, and Muse's, its child's.
    pub fn note_pids(&self, window: u32, muse: u32) -> std::io::Result<()> {
        use std::io::Write as _;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pids_beside(&self.argv))?
            .write_all(format!("{window} {muse}\n").as_bytes())
    }

    /// What Muse is run with for the command line `words`: on echo, its turns the rig's
    /// length (`CF_TEST_MUSE_DELAY_MS` where a case sets it) and no model or effort,
    /// which echo refuses; live, the words as they are.
    pub fn args(&self, words: &[String]) -> Vec<String> {
        let Run::Echo { turn_ms } = self.run else {
            return words.to_vec();
        };
        let delay = own_var("CF_TEST_MUSE_DELAY_MS").unwrap_or_else(|| turn_ms.to_string());
        let mut args = vec![
            "--provider".to_owned(),
            "echo".to_owned(),
            "--echo-delay-ms".to_owned(),
            delay,
        ];
        let mut rest = words.iter();
        while let Some(word) = rest.next() {
            if word == "--model" || word == "--reasoning-effort" {
                rest.next();
            } else {
                args.push(word.clone());
            }
        }
        args
    }

    /// What Muse is given besides the window's environment: live, the rig's own config, data
    /// and cache folders (T-1084), with the copy of the person's login and the settings
    /// [`seed_live_home`] made in the first. Muse on Windows honours these and keeps its login
    /// in that file (`credential_backend: file`); the person's profile and home are named
    /// nowhere, so what the window finds of the person's own Muse is nothing.
    pub fn env(&self) -> Vec<(String, String)> {
        match &self.run {
            Run::Echo { .. } => Vec::new(),
            Run::Live { .. } => {
                let at = |path: PathBuf| path.to_string_lossy().into_owned();
                vec![
                    (
                        "XDG_CONFIG_HOME".to_owned(),
                        at(self.rig_home.join(".config")),
                    ),
                    (
                        "XDG_DATA_HOME".to_owned(),
                        at(self.rig_home.join(".local").join("share")),
                    ),
                    (
                        "XDG_CACHE_HOME".to_owned(),
                        at(self.rig_home.join(".cache")),
                    ),
                ]
            }
        }
    }
}

/// Writes the rig's Muse on Windows in `dir`: its launcher (`muse.cmd`, which the product
/// finds and never runs), the version line, and `install`'s stand-in as the binary they
/// name, told by the file beside it to run `install`'s binary as `run` says.
fn write_windows(
    dir: &Path,
    install: &Installed,
    run: Run,
    argv: &Path,
    rig_home: &Path,
) -> Result {
    let program = install.stand_in.as_deref().ok_or_else(|| {
        Error::Daemon("the rig's Muse on Windows is the muse-stand-in program, which the case names (Installed::through)".to_owned())
    })?;
    let pinned = dir.join(format!("muse-bin-{}.exe", install.version));
    files::make_dir(dir)?;
    fs::copy(program, &pinned).map_err(Error::file("copy", &pinned))?;
    files::write(&dir.join(".muse-version"), format!("{}\n", install.version))?;
    files::write(
        &dir.join("muse.cmd"),
        format!("@\"{}\" %*\r\n", pinned.display()),
    )?;
    let stand_in = StandIn {
        binary: install.binary.clone(),
        argv: argv.to_owned(),
        run,
        rig_home: rig_home.to_owned(),
    };
    files::write(&StandIn::file_of(&pinned), stand_in.to_json().to_string())
}

/// Writes the rig's Muse in `dir`: its launcher, the version line, and the binary that
/// runs `install`'s on echo, each turn `delay_ms` long, writing each command line it
/// is given to `argv` (its words split by a unit separator, each line ended by a record
/// separator: a first message has lines of its own).
pub fn write_stand_in(dir: &Path, install: &Installed, delay_ms: u64, argv: &Path) -> Result {
    if cfg!(windows) {
        let run = Run::Echo { turn_ms: delay_ms };
        // An echo run keeps nothing of Muse's in the rig's home: the rig's, beside the log, is
        // named all the same.
        return write_windows(dir, install, run, argv, &argv.with_file_name("home"));
    }
    let pinned = dir.join(format!("muse-bin-{}", install.version));
    files::write(&dir.join(".muse-version"), format!("{}\n", install.version))?;
    files::write_executable(
        &dir.join("muse"),
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", pinned.display()),
    )?;
    let script = format!(
        r#"#!/bin/bash
# The rig's Muse: the machine's, on its fake model. Echo takes no model or effort.
( IFS=$'\x1f'; printf '%s\x1e' "$*" ) >> '{argv}'
printf '%s %s\n' $$ $$ >> '{pids}'
words=()
while [ $# -gt 0 ]; do
  case "$1" in
    --model|--reasoning-effort) shift 2 ;;
    *) words+=("$1"); shift ;;
  esac
done
exec '{binary}' --provider echo --echo-delay-ms "${{CF_TEST_MUSE_DELAY_MS:-{delay_ms}}}" "${{words[@]}}"
"#,
        argv = argv.display(),
        pids = pids_beside(argv).display(),
        binary = install.binary.display(),
    );
    files::write_executable(&pinned, script)
}

/// Writes the rig's Muse in `dir` for a live run: its launcher, the version line, and
/// the binary that runs `install`'s on the real model, with the model and effort it is
/// given, writing each command line to `argv` as [`write_stand_in`]'s does. On a Mac Muse
/// keeps its login's secret in the system's keychain, which it finds by the person's home
/// (`real_home`: a throwaway home opens its onboarding, T-705); on Windows its login is the
/// file in its config folder and the person's home is named nowhere ([`StandIn::env`]). Its
/// settings, the copy of the login ([`login_copy`]), and its sessions are in the rig's home
/// (`rig_home`), where the daemon reads them, so nothing of the person's Muse folders is
/// read or written.
pub fn write_live_stand_in(
    dir: &Path,
    install: &Installed,
    real_home: &Path,
    rig_home: &Path,
    argv: &Path,
) -> Result {
    if cfg!(windows) {
        let run = Run::Live {
            real_home: real_home.to_owned(),
        };
        return write_windows(dir, install, run, argv, rig_home);
    }
    let pinned = dir.join(format!("muse-bin-{}", install.version));
    files::write(&dir.join(".muse-version"), format!("{}\n", install.version))?;
    files::write_executable(
        &dir.join("muse"),
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", pinned.display()),
    )?;
    let script = format!(
        r#"#!/bin/bash
# The rig's Muse, live: the machine's, on its real model, the person's keychain login.
( IFS=$'\x1f'; printf '%s\x1e' "$*" ) >> '{argv}'
printf '%s %s\n' $$ $$ >> '{pids}'
export HOME='{real}' XDG_CONFIG_HOME='{rig}/.config' XDG_DATA_HOME='{rig}/.local/share' XDG_CACHE_HOME='{rig}/.cache'
exec '{binary}' "$@"
"#,
        argv = argv.display(),
        pids = pids_beside(argv).display(),
        real = real_home.display(),
        rig = rig_home.display(),
        binary = install.binary.display(),
    );
    files::write_executable(&pinned, script)
}

/// The file beside the rig's Muse's `argv` log in which each of its windows writes its
/// process and Muse's, a line each (one number twice on a Mac, where the stand-in becomes
/// Muse; Muse's own a child's on Windows).
pub fn pids_beside(argv: &Path) -> PathBuf {
    argv.with_file_name("muse-pids.log")
}

/// Each window's process and its Muse's, as the rig's Muse wrote them, oldest first.
pub fn pids(argv: &Path) -> Vec<(u32, u32)> {
    fs::read_to_string(pids_beside(argv))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (window, muse) = line.trim().split_once(' ')?;
            Some((window.parse().ok()?, muse.parse().ok()?))
        })
        .collect()
}

/// Every command line the rig's Muse was given, in order, as its words.
pub fn launches(argv: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(argv)
        .unwrap_or_default()
        .split('\u{1e}')
        .filter(|line| !line.is_empty())
        .map(|line| line.split('\u{1f}').map(str::to_owned).collect())
        .collect()
}

/// The sessions Muse keeps in the home `home` (its data folder there, `.local/share/muse`),
/// by id, each with its log, oldest first (a UUIDv7 sorts by when it began).
pub fn sessions(home: &Path) -> Vec<(String, PathBuf)> {
    let root = home
        .join(".local")
        .join("share")
        .join("muse")
        .join("sessions");
    let mut found = Vec::new();
    for year in dirs(&root) {
        for month in dirs(&year) {
            for day in dirs(&month) {
                for session in dirs(&day) {
                    let log = session.join("session.jsonl");
                    if log.is_file() {
                        let id = session
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        found.push((id, log));
                    }
                }
            }
        }
    }
    found.sort();
    found
}

fn dirs(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.file_name().is_some_and(|name| name != "subagent"))
        .collect();
    found.sort();
    found
}

/// A session log's records, its torn lines passed over.
pub fn records(log: &Path) -> Vec<Value> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// What a record is, as a case names it: its payload's kind, and a run event's kind
/// after a slash (`run/terminal`, `run/run_retracted`, `user_intent`).
pub fn kind(record: &Value) -> String {
    let payload = &record["payload"];
    let kind = payload["kind"].as_str().unwrap_or_default();
    match payload["event"]["kind"].as_str() {
        Some(event) if !kind.is_empty() => format!("{kind}/{event}"),
        _ => kind.to_owned(),
    }
}

/// How each run of a log ended, in order: `completed`, `cancelled` or `failed`.
pub fn terminals(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .filter(|record| kind(record) == "run/terminal")
        .map(|record| {
            record["payload"]["event"]["terminal"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// The words of each message of the user's Muse took in, in order
/// (`runtime.user_intent.accepted`), and how each went in: a turn of its own, or a
/// turn at work (`active_run_delivery`), by its `materialized` record.
pub fn messages(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .filter(|record| record["payload_type"] == "runtime.user_intent.accepted")
        .map(|record| {
            record["payload"]["refill_blocks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .collect()
}

/// How each message of the user's went in, in order: the outcome of its
/// `runtime.user_intent.materialized` (`top_level_turn_started`, `active_run_delivery`).
pub fn deliveries(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .filter(|record| record["payload_type"] == "runtime.user_intent.materialized")
        .map(|record| {
            record["payload"]["outcome"]["kind"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// Whether the log has a record of `kind` ([`kind`]).
pub fn has(records: &[Value], wanted: &str) -> bool {
    records.iter().any(|record| kind(record) == wanted)
}

/// What `muse usage --json` says of the account's allowance now, asked of the machine's
/// Muse with the person's login (its secret in the keychain, found by `real_home`) and
/// the settings in `config_home`, a throwaway folder: no model turn.
pub fn usage(install: &Installed, real_home: &Path, config_home: &Path) -> Result<Value> {
    // By its real path: Muse takes a data folder reached through a link for unsafe (T-931).
    let config_home = &files::real_path(config_home);
    let mut run = crate::process::Run::new(&install.binary).args(["usage", "--json"]);
    for (name, value) in usage_vars(real_home, config_home, cfg!(windows)) {
        run = run.var(name, value);
    }
    run.closed_input()
        .limit(std::time::Duration::from_secs(60))
        .run()?
        .json()
}

/// The variables `muse usage --json` is run with: the settings and, on a Mac, the login's
/// metadata are in `config_home` (a throwaway folder seeded by [`seed_live_home`]), Muse's
/// secret in the keychain it finds by the person's home. On `windows` the login is the file in
/// `config_home` and there is no keychain, so the person's profile is named nowhere.
fn usage_vars(
    real_home: &Path,
    config_home: &Path,
    windows: bool,
) -> Vec<(&'static str, std::ffi::OsString)> {
    let mut vars: Vec<(&'static str, std::ffi::OsString)> = vec![
        ("XDG_DATA_HOME", config_home.join("data").into()),
        ("XDG_CACHE_HOME", config_home.join("cache").into()),
        ("MUSE_NO_AUTO_UPDATE", "1".into()),
        ("MUSE_LOGIN", "0".into()),
    ];
    if windows {
        let root = config_home.parent().unwrap_or(config_home);
        vars.push(("USERPROFILE", root.into()));
        vars.push(("HOME", root.into()));
        vars.push(("XDG_CONFIG_HOME", config_home.into()));
    } else {
        vars.push(("HOME", real_home.into()));
        vars.push(("XDG_CONFIG_HOME", config_home.into()));
        vars.push(("PATH", "/usr/bin:/bin".into()));
    }
    vars
}

/// How much of its current window and of its week an allowance has used, in percent:
/// none where `usage` says no allowance. A window the server sent no usage for yet
/// (`server_sent_no_usage`: "this can happen before your first use in a new window",
/// Muse 1.4.14) is one nothing was spent of.
pub fn used_percent(usage: &Value) -> Option<(u64, u64)> {
    let allowance = &usage["allowance"];
    if allowance.is_null() && usage["reason"] == "server_sent_no_usage" {
        return Some((0, 0));
    }
    Some((
        allowance["window"]["usedPercent"].as_u64()?,
        allowance["weekly"]["usedPercent"].as_u64()?,
    ))
}

/// The model a log says Muse moved its session to on its own, away from `launched`
/// (`runtime.model_reconfigure.completed`, whose `effective` is another model): what T-705
/// saw after a refusal, to a contributor model. None where it never moved.
pub fn moved_away(records: &[Value], launched: &str) -> Option<String> {
    records
        .iter()
        .filter(|record| record["payload_type"] == "runtime.model_reconfigure.completed")
        .filter_map(|record| record["payload"]["record"]["effective"]["model_id"].as_str())
        .find(|model| *model != launched)
        .map(str::to_owned)
}

/// The record kinds of a log in order, its run events after a slash, with what Muse
/// writes of its own bookkeeping (each model request's context blocks and traces, the
/// tasks of its reminders) left out: what a case says of a live turn's shape.
pub fn shape(records: &[Value]) -> Vec<String> {
    const NOISE: [&str; 6] = [
        "run/context_block_diagnostic",
        "run/model_input_trace_recorded",
        "run/resource_usage_sampled",
        "run/goal_usage_attribution",
        "run/task_stream_linked",
        "command_intake",
    ];
    records
        .iter()
        .filter(|record| record["stream"]["kind"] != "task")
        .map(kind)
        .filter(|kind| {
            !kind.is_empty() && !kind.starts_with("task/") && !NOISE.contains(&kind.as_str())
        })
        .collect()
}

/// A project of the rig's with a chief on the stand-in `claude` and a Muse worker (the
/// live tests' on the model the run names, the echo rig's on `muse-spark-1.3` at
/// `minimal`, which the rig's echo Muse takes out), and what its cases read of it.
pub mod members {
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::{kind, records};
    use crate::files;
    use crate::rig::{Project, Rig};
    use crate::Result;

    /// The worker's agent, as the roster names it.
    pub const WORKER: &str = "muse-worker";

    /// The worker of the echo rig's cases: the live tests' model and effort.
    pub fn echo_worker() -> Value {
        json!({ "kind": "muse", "model": "muse-spark-1.3", "effort": "minimal" })
    }

    /// The project of a case: the roster written, its worker `agent` (its kind, model
    /// and effort) under [`WORKER`], the chief's project opened, the Muse worker added.
    pub fn project<'a>(rig: &'a Rig, agent: &Value) -> Result<Project<'a>> {
        let mut worker = agent.clone();
        if let Some(fields) = worker.as_object_mut() {
            fields.insert("id".to_owned(), json!(WORKER));
            fields.insert("workTier".to_owned(), json!("standard"));
        }
        let roster = json!({
            "schemaVersion": 1,
            "agents": [{ "id": "chief", "kind": "claude-code", "model": "fake-chief" }, worker],
        });
        files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
        let project = Project::open(rig, "chief", json!({}))?;
        project.add_member(WORKER)?;
        Ok(project)
    }

    /// The handle of the Muse worker given T-1: its agent's name and words of its own.
    pub fn worker(project: &Project<'_>) -> Result<String> {
        Ok(project.task(1)?["assignee"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches('@')
            .to_owned())
    }

    /// The one Muse session of the rig, its id and its records now.
    pub fn the_session(rig: &Rig) -> (String, Vec<Value>) {
        let sessions = rig.muse_sessions();
        assert_eq!(sessions.len(), 1, "one session: {sessions:?}");
        let (id, log) = &sessions[0];
        (id.clone(), records(log))
    }

    /// The worker's window, as the board names its pane.
    pub fn pane_of(project: &Project<'_>) -> Result<Value> {
        Ok(project.lane(WORKER)?["pane"].clone())
    }

    /// Waits until the worker's window is in its turn: the board says it works, and its
    /// session's log has `runs` turns begun.
    pub fn in_its_turn(rig: &Rig, project: &Project<'_>, runs: usize, within: Duration) -> Result {
        rig.wait_for("the Muse worker to be in its turn", within, || {
            let begun = rig.muse_sessions().first().is_some_and(|(_, log)| {
                records(log)
                    .iter()
                    .filter(|record| kind(record) == "run/started")
                    .count()
                    >= runs
            });
            Ok(begun && project.lane(WORKER)?["activity"]["state"] == "working")
        })
    }

    /// The message of `participant`'s inbox whose words hold `words`, as it is now.
    pub fn message_with(
        project: &Project<'_>,
        participant: &str,
        words: &str,
    ) -> Result<Option<Value>> {
        Ok(project.inbox(participant)?.into_iter().find(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(words))
        }))
    }

    /// Whether the rig's Muse was last started with `muse resume` on `session`.
    pub fn last_resumed(rig: &Rig, session: &str) -> bool {
        rig.muse_launches()
            .last()
            .is_some_and(|argv| argv.windows(2).any(|pair| pair == ["resume", session]))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{used_percent, Run, StandIn};

    /// T-829's live run (Muse 1.4.14, 2026-10-28): a window of the subscription in which
    /// nothing was used yet answers no allowance with `server_sent_no_usage`, which is no
    /// account without one: nothing of it is spent, and the run goes on.
    #[test]
    fn a_window_the_server_sent_no_usage_for_yet_is_one_nothing_was_spent_of() {
        let fresh = json!({
            "schemaVersion": 1, "allowance": null, "reason": "server_sent_no_usage",
            "hint": "the server sent no usage data for this subscription; this can happen before your first use in a new window",
        });
        assert_eq!(used_percent(&fresh), Some((0, 0)));
        let used = json!({ "schemaVersion": 1, "allowance": {
            "window": { "usedPercent": 23, "resetsAtMs": 1 }, "weekly": { "usedPercent": 4, "resetsAtMs": 2 },
        } });
        assert_eq!(used_percent(&used), Some((23, 4)));
        let none = json!({ "schemaVersion": 1, "allowance": null, "reason": "no_allowance" });
        assert_eq!(used_percent(&none), None);
    }

    /// The rig's Muse on Windows runs the machine's binary on echo, each turn the rig's
    /// length, with no model or effort (echo refuses them) and the rest of its command
    /// line as it came; live, as it came, in the rig's own config, data and cache folders
    /// (T-1084: not the person's, where Muse on Windows keeps its login in a file).
    /// What it runs is read back as it was written.
    #[test]
    fn the_rig_s_muse_on_windows_runs_the_machine_s_on_echo_without_a_model_or_live_as_given() {
        let words: Vec<String> = [
            "-c",
            "hooks={}",
            "--model",
            "muse-spark-1.3",
            "--reasoning-effort",
            "minimal",
            "--yolo",
        ]
        .map(str::to_owned)
        .into();
        let echo = StandIn {
            binary: "C:/m/muse-bin-1.4.13-R6996.1.exe".into(),
            argv: "C:/rig/muse-argv.log".into(),
            run: Run::Echo { turn_ms: 4_000 },
            rig_home: "C:/rig/home".into(),
        };
        if crate::process::own_var("CF_TEST_MUSE_DELAY_MS").is_none() {
            assert_eq!(
                echo.args(&words),
                [
                    "--provider",
                    "echo",
                    "--echo-delay-ms",
                    "4000",
                    "-c",
                    "hooks={}",
                    "--yolo"
                ]
            );
        }
        assert!(echo.env().is_empty());
        assert_eq!(StandIn::from_json(&echo.to_json().to_string()), Some(echo));
        let live = StandIn {
            binary: "C:/m/muse-bin-1.4.13-R6996.1.exe".into(),
            argv: "C:/rig/muse-argv.log".into(),
            run: Run::Live {
                real_home: "C:/Users/AGV".into(),
            },
            rig_home: "C:/rig/home".into(),
        };
        assert_eq!(live.args(&words), words);
        let rig = std::path::Path::new("C:/rig/home");
        let at = |path: std::path::PathBuf| path.to_string_lossy().into_owned();
        assert_eq!(
            live.env(),
            [
                ("XDG_CONFIG_HOME".to_owned(), at(rig.join(".config"))),
                (
                    "XDG_DATA_HOME".to_owned(),
                    at(rig.join(".local").join("share"))
                ),
                ("XDG_CACHE_HOME".to_owned(), at(rig.join(".cache"))),
            ]
        );
        assert_eq!(StandIn::from_json(&live.to_json().to_string()), Some(live));
        assert_eq!(StandIn::from_json("{}"), None);
    }

    /// A live Muse on Windows is given none of the person's folders: not their profile, not
    /// their home, and a config, data and cache folder all in the rig's home.
    #[test]
    fn a_live_muse_on_windows_is_given_none_of_the_persons_folders() {
        let live = StandIn {
            binary: "C:/m/muse-bin-1.4.13-R6996.1.exe".into(),
            argv: "C:/rig/muse-argv.log".into(),
            run: Run::Live {
                real_home: "C:/Users/AGV".into(),
            },
            rig_home: "C:/rig/home".into(),
        };
        let env = live.env();
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]
        );
        for (name, value) in &env {
            assert!(value.starts_with("C:/rig/home"), "{name}: {value}");
            assert!(!value.contains("AGV"), "{name}: {value}");
        }
        // A stand-in that says no rig home says no stand-in: it would run on the person's.
        let mut json = live.to_json();
        json.as_object_mut().unwrap().remove("rig");
        assert_eq!(StandIn::from_json(&json.to_string()), None);
    }

    /// The copy of a Muse login holds no refresh token, whatever it is called and however
    /// deep: it cannot use up the person's. What the login is made of is kept.
    #[test]
    fn a_copy_of_muse_s_login_holds_no_refresh_token_at_any_depth() {
        let login = json!({
            "schema_version": 1,
            "refresh_token": "r-top-SECRET",
            "providers": { "meta": {
                "access_token": "a-SECRET", "api_key": "k-SECRET", "obtained_via": "oauth",
                "subs_active": true, "refreshToken": "r-camel-SECRET",
                "nested": { "Refresh_Token_Expires_At": 1, "keep": "this" },
            } },
        });
        let copy: serde_json::Value =
            serde_json::from_str(&super::login_copy(&login.to_string()).unwrap()).unwrap();
        assert!(!copy.to_string().contains("r-top-SECRET"), "{copy}");
        assert!(!copy.to_string().contains("r-camel-SECRET"), "{copy}");
        assert!(
            !copy.to_string().to_lowercase().contains("refresh"),
            "{copy}"
        );
        let meta = &copy["providers"]["meta"];
        assert_eq!(meta["access_token"], "a-SECRET");
        assert_eq!(meta["api_key"], "k-SECRET");
        assert_eq!(meta["obtained_via"], "oauth");
        assert_eq!(meta["subs_active"], true);
        assert_eq!(meta["nested"]["keep"], "this");
        assert_eq!(copy["schema_version"], 1);
        // A login with none is copied as it is, in value.
        let plain = json!({ "providers": { "meta": { "access_token": "a" } } });
        let same: serde_json::Value =
            serde_json::from_str(&super::login_copy(&plain.to_string()).unwrap()).unwrap();
        assert_eq!(same, plain);
    }

    /// A file that is no JSON object cannot be shown free of a refresh token, so it is no login
    /// to copy.
    #[test]
    fn a_login_that_is_not_a_json_object_is_not_copied() {
        for text in ["", "not json", "[1, 2]", "\"a string\""] {
            let said = super::login_copy(text).unwrap_err().to_string();
            assert!(said.contains("Muse"), "{text:?}: {said}");
        }
    }

    /// A live rig's home is given Muse's settings and a copy of the person's login without a
    /// refresh token, on every system (on Windows too, where the login is the file itself); the
    /// person's file is left as it was.
    #[test]
    fn a_live_rig_s_home_is_given_settings_and_the_login_copied_without_a_refresh_token() {
        let person = tempfile::tempdir().unwrap();
        let rig = tempfile::tempdir().unwrap();
        let theirs = person.path().join(".config").join("muse").join("auth.json");
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        let login = json!({ "providers": { "meta": { "access_token": "a-SECRET", "refresh_token": "r-SECRET" } } });
        std::fs::write(&theirs, login.to_string()).unwrap();
        super::seed_live_home(rig.path(), person.path()).unwrap();
        let ours = rig.path().join(".config").join("muse");
        let copy = std::fs::read_to_string(ours.join("auth.json")).unwrap();
        assert!(copy.contains("a-SECRET"), "{copy}");
        assert!(!copy.contains("r-SECRET"), "{copy}");
        assert_eq!(
            std::fs::read_to_string(ours.join("settings.json")).unwrap(),
            "{\"schema_version\":1}\n"
        );
        assert_eq!(std::fs::read_to_string(&theirs).unwrap(), login.to_string());
        // A person with no login stops the seed, naming the file.
        std::fs::remove_file(&theirs).unwrap();
        let said = super::seed_live_home(rig.path(), person.path())
            .unwrap_err()
            .to_string();
        assert!(said.contains("auth.json"), "{said}");
    }

    /// `muse usage --json` on Windows is run on the throwaway config folder that holds the copy of
    /// the login, with no variable naming the person's profile or home: Muse has no keychain
    /// there to find by it. On a Mac the person's home is the keychain's.
    #[test]
    fn muse_usage_is_run_on_the_copy_of_the_login_and_on_windows_names_none_of_the_persons_folders()
    {
        let person = std::path::Path::new("/Users/AGV");
        let config = std::path::Path::new("/tmp/throwaway/.config");
        let value = |vars: &[(&str, std::ffi::OsString)], name: &str| {
            vars.iter()
                .find(|(held, _)| *held == name)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        let windows = super::usage_vars(person, config, true);
        assert_eq!(
            value(&windows, "XDG_CONFIG_HOME").as_deref(),
            Some("/tmp/throwaway/.config")
        );
        assert_eq!(
            value(&windows, "USERPROFILE").as_deref(),
            Some("/tmp/throwaway")
        );
        assert_eq!(value(&windows, "HOME").as_deref(), Some("/tmp/throwaway"));
        for (name, value) in &windows {
            assert!(
                !value.to_string_lossy().contains("AGV"),
                "{name}: {value:?}"
            );
        }
        let mac = super::usage_vars(person, config, false);
        assert_eq!(
            value(&mac, "XDG_CONFIG_HOME").as_deref(),
            Some("/tmp/throwaway/.config")
        );
        assert_eq!(value(&mac, "HOME").as_deref(), Some("/Users/AGV"));
    }
}
