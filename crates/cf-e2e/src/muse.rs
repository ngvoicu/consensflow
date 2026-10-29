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
/// own, and on a Mac a copy of the metadata of the person's login (`auth.json`; the
/// secret stays in the keychain, the one item every Muse of the machine reads). On
/// Windows the login is all in that file (`credential_backend: file`), which a live
/// window reads where it is ([`StandIn::env`]): a copy would refresh apart from the
/// person's. The person's file is read, never written.
pub fn seed_live_home(rig_home: &Path, real_home: &Path) -> Result {
    let config = rig_home.join(".config").join("muse");
    files::write(&config.join("settings.json"), "{\"schema_version\":1}\n")?;
    if cfg!(windows) {
        return Ok(());
    }
    let auth = real_home.join(".config").join("muse").join("auth.json");
    files::write(&config.join("auth.json"), files::read(&auth)?)
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
        json!({ "binary": self.binary, "argv": self.argv, "run": run })
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

    /// What Muse is given besides the window's environment: live, the person's profile
    /// and config folder, where Muse on Windows keeps its login in a file
    /// (`auth.json`, `credential_backend: file`), which is used where it is and never
    /// copied (a copy's refresh would leave the person's stale); its sessions stay in
    /// the rig's home, where the daemon reads them.
    pub fn env(&self) -> Vec<(String, String)> {
        match &self.run {
            Run::Echo { .. } => Vec::new(),
            Run::Live { real_home } => {
                let real = real_home.to_string_lossy().into_owned();
                let config = real_home.join(".config").to_string_lossy().into_owned();
                vec![
                    ("USERPROFILE".to_owned(), real.clone()),
                    ("HOME".to_owned(), real),
                    ("XDG_CONFIG_HOME".to_owned(), config),
                ]
            }
        }
    }
}

/// Writes the rig's Muse on Windows in `dir`: its launcher (`muse.cmd`, which the product
/// finds and never runs), the version line, and `install`'s stand-in as the binary they
/// name, told by the file beside it to run `install`'s binary as `run` says.
fn write_windows(dir: &Path, install: &Installed, run: Run, argv: &Path) -> Result {
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
        return write_windows(dir, install, run, argv);
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
/// given, writing each command line to `argv` as [`write_stand_in`]'s does. Muse keeps
/// its login's secret in the system's keychain, which it finds by the person's home
/// (`real_home`: a throwaway home opens its onboarding, T-705); its settings, the copy of
/// the login's metadata, and its sessions are in the rig's home (`rig_home`), where the
/// daemon reads them, so nothing of the person's Muse folders is read or written.
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
        return write_windows(dir, install, run, argv);
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
    let run = crate::process::Run::new(&install.binary)
        .args(["usage", "--json"])
        .var("HOME", real_home)
        .var("XDG_DATA_HOME", config_home.join("data"))
        .var("XDG_CACHE_HOME", config_home.join("cache"))
        .var("MUSE_NO_AUTO_UPDATE", "1")
        .var("MUSE_LOGIN", "0");
    // On Windows the login is the person's config folder's, read where it is.
    let run = if cfg!(windows) {
        run.var("USERPROFILE", real_home)
            .var("XDG_CONFIG_HOME", real_home.join(".config"))
    } else {
        run.var("XDG_CONFIG_HOME", config_home)
            .var("PATH", "/usr/bin:/bin")
    };
    run.closed_input()
        .limit(std::time::Duration::from_secs(60))
        .run()?
        .json()
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
    /// line as it came; live, as it came, in the person's profile and config folder, where
    /// Muse keeps its login on Windows. What it runs is read back as it was written.
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
        };
        assert_eq!(live.args(&words), words);
        let config = std::path::Path::new("C:/Users/AGV").join(".config");
        assert_eq!(
            live.env(),
            [
                ("USERPROFILE".to_owned(), "C:/Users/AGV".to_owned()),
                ("HOME".to_owned(), "C:/Users/AGV".to_owned()),
                (
                    "XDG_CONFIG_HOME".to_owned(),
                    config.to_string_lossy().into_owned()
                ),
            ]
        );
        assert_eq!(StandIn::from_json(&live.to_json().to_string()), Some(live));
        assert_eq!(StandIn::from_json("{}"), None);
    }
}
