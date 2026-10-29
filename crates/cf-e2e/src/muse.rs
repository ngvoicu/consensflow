//! Muse Code as a case meets it from the outside: the machine's own install, which the
//! rig's windows run on Muse's built-in fake model (`--provider echo`: offline, no
//! login, no quota), and the session logs Muse writes, read as a case reads them.
//!
//! The rig puts a launcher of its own on its windows' `PATH` (`muse`) and beside it the
//! `.muse-version` of the machine's install and a `muse-bin-<version>` that runs the
//! machine's binary with `--provider echo`, as the product runs the binary a launcher
//! names. Echo refuses a model and an effort, so the stand-in takes them out of what it
//! runs, and writes down every command line it was given whole (`argv`), for a case to
//! hold the launch to. It `exec`s Muse, so the window's process is Muse's, as the
//! product's own window's is.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::process::own_var;
use crate::{files, Result};

/// How the rig's Muse windows run: on Muse's fake model, each turn this many
/// milliseconds long, or live, on the model each is launched on, the person's login
/// found from their home.
#[derive(Debug, Clone)]
pub enum Run {
    Echo { turn_ms: u64 },
    Live { real_home: PathBuf },
}

/// Gives a live rig's home what its Muse windows read their login by: settings of their
/// own, and a copy of the metadata of the person's login (`auth.json`; the secret stays
/// in the keychain, the one item every Muse of the machine reads). The person's file is
/// read, never written.
pub fn seed_live_home(rig_home: &Path, real_home: &Path) -> Result {
    let config = rig_home.join(".config").join("muse");
    files::write(&config.join("settings.json"), "{\"schema_version\":1}\n")?;
    let auth = real_home.join(".config").join("muse").join("auth.json");
    files::write(&config.join("auth.json"), files::read(&auth)?)
}

/// The machine's own Muse: its binary and the version line naming it.
#[derive(Debug, Clone)]
pub struct Installed {
    pub binary: PathBuf,
    pub version: String,
}

/// The Muse of whoever runs the tests, found as the product's launch finds it
/// ([`installed_on`]), or why there is none to run: Muse's rig cases run on a Mac with
/// Muse Code installed. The Windows machines have none yet (MUS-20), nor has CI.
pub fn installed() -> std::result::Result<Installed, String> {
    if !cfg!(target_os = "macos") {
        return Err("Muse Code's rig cases run on a Mac with Muse installed; this machine is no Mac (the Windows machines have no Muse yet, MUS-20)".to_owned());
    }
    installed_on(
        &own_var("PATH").unwrap_or_default(),
        own_var("HOME").as_deref(),
    )
}

/// [`installed`], on the `PATH` `path` and the home `home`: the first `muse` on the `PATH`,
/// as the product's launch takes it (`cf_harness::muse::launch`), its link followed to the
/// launcher it names, and the `.muse-version` beside that launcher and the
/// `muse-bin-<version>` it names. The installer's place in the home (`~/.local/bin`) is
/// read only where the `PATH` has no `muse`. So a run is pointed at another Muse (one
/// downloaded into a scratch folder, T-905) by its `PATH`, and the person's home and Muse
/// are left as they are.
fn installed_on(path: &str, home: Option<&str>) -> std::result::Result<Installed, String> {
    let launcher = match crate::live::machine::find("muse", path, None) {
        Some(found) => found,
        None => Path::new(home.ok_or("no muse on the PATH, and no home to find Muse Code in")?)
            .join(".local")
            .join("bin")
            .join("muse"),
    };
    let real = fs::canonicalize(&launcher).unwrap_or_else(|_| launcher.clone());
    let bin = real.parent().unwrap_or(Path::new("."));
    let version = fs::read_to_string(bin.join(".muse-version"))
        .map(|line| line.trim().to_owned())
        .map_err(|_| {
            format!(
                "Muse Code is not installed on this machine (no .muse-version beside {})",
                launcher.display()
            )
        })?;
    let binary = bin.join(format!("muse-bin-{version}"));
    if !binary.is_file() {
        return Err(format!(
            "Muse Code's binary {} is not on this machine",
            binary.display()
        ));
    }
    Ok(Installed { binary, version })
}

/// Writes the rig's Muse in `dir`: its launcher, the version line, and the binary that
/// runs `install`'s on echo, each turn `delay_ms` long, writing each command line it
/// is given to `argv` (its words split by a unit separator, each line ended by a record
/// separator: a first message has lines of its own).
pub fn write_stand_in(dir: &Path, install: &Installed, delay_ms: u64, argv: &Path) -> Result {
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
export HOME='{real}' XDG_CONFIG_HOME='{rig}/.config' XDG_DATA_HOME='{rig}/.local/share' XDG_CACHE_HOME='{rig}/.cache'
exec '{binary}' "$@"
"#,
        argv = argv.display(),
        real = real_home.display(),
        rig = rig_home.display(),
        binary = install.binary.display(),
    );
    files::write_executable(&pinned, script)
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
    crate::process::Run::new(&install.binary)
        .args(["usage", "--json"])
        .var("HOME", real_home)
        .var("XDG_CONFIG_HOME", config_home)
        .var("XDG_DATA_HOME", config_home.join("data"))
        .var("XDG_CACHE_HOME", config_home.join("cache"))
        .var("MUSE_NO_AUTO_UPDATE", "1")
        .var("MUSE_LOGIN", "0")
        .var("PATH", "/usr/bin:/bin")
        .closed_input()
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
    #[cfg(unix)]
    use std::path::Path;

    use serde_json::json;

    #[cfg(unix)]
    use super::installed_on;
    use super::used_percent;
    #[cfg(unix)]
    use crate::files;

    /// A Muse install in `bin`: its launcher, the version line, and the binary it names.
    #[cfg(unix)]
    fn install(bin: &Path, version: &str) {
        files::write_executable(&bin.join("muse"), "#!/bin/sh\n").unwrap();
        files::write(&bin.join(".muse-version"), format!("{version}\n")).unwrap();
        files::write_executable(&bin.join(format!("muse-bin-{version}")), "#!/bin/sh\n").unwrap();
    }

    /// The tester's T-905: a Muse downloaded into a scratch folder is run by putting that
    /// folder first on the `PATH`, as the product's launch finds Muse, and never by moving
    /// the home, which would put the person's own Muse at risk. (A Mac's: the kit runs Muse
    /// on no other machine, and a Windows `PATH` takes a launcher by its extension.)
    #[cfg(unix)]
    #[test]
    fn the_kit_runs_the_first_muse_on_the_path_and_the_home_s_only_where_the_path_has_none() {
        let home = tempfile::tempdir().unwrap();
        install(&home.path().join(".local").join("bin"), "1.4.13-R6996.1");
        let scratch = tempfile::tempdir().unwrap();
        install(scratch.path(), "1.4.14-R7309.1");
        let empty = tempfile::tempdir().unwrap();
        let home_of = home.path().to_str();
        let path = std::env::join_paths([empty.path(), scratch.path()]).unwrap();
        let found = installed_on(path.to_str().unwrap(), home_of).unwrap();
        assert_eq!(found.version, "1.4.14-R7309.1");
        assert_eq!(
            found.binary,
            std::fs::canonicalize(scratch.path())
                .unwrap()
                .join("muse-bin-1.4.14-R7309.1")
        );
        // A `PATH` with no `muse`: the installer's place in the home.
        let found = installed_on(empty.path().to_str().unwrap(), home_of).unwrap();
        assert_eq!(found.version, "1.4.13-R6996.1");
    }

    /// A `muse` on the `PATH` that is a link (Homebrew's, the person's own) is read where it
    /// leads, as the product reads the launcher's `.muse-version`.
    #[cfg(unix)]
    #[test]
    fn a_muse_on_the_path_that_is_a_link_is_read_beside_the_launcher_it_names() {
        let real = tempfile::tempdir().unwrap();
        install(real.path(), "1.4.14-R7309.1");
        let linked = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(real.path().join("muse"), linked.path().join("muse")).unwrap();
        let found = installed_on(linked.path().to_str().unwrap(), None).unwrap();
        assert_eq!(found.version, "1.4.14-R7309.1");
        assert!(found
            .binary
            .starts_with(std::fs::canonicalize(real.path()).unwrap()));
    }

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
}
