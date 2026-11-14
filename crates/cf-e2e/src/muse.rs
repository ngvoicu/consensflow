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
    crate::door::write(config.join("settings.json"), "{\"schema_version\":1}\n")?;
    let auth = real_home.join(".config").join("muse").join("auth.json");
    let text = files::read_string(&auth)?;
    crate::door::write(config.join("auth.json"), login_copy(&text)?)
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

/// The Muse of whoever runs the tests, found as the product's launch finds it
/// ([`installed_on`]), or why there is none to run. Muse ships for a Mac and Windows
/// alone, and CI's machines have none.
pub fn installed() -> std::result::Result<Installed, String> {
    if !cfg!(any(target_os = "macos", windows)) {
        return Err("Muse Code's rig cases run on a Mac or on Windows with Muse installed: Muse ships for no other".to_owned());
    }
    installed_on(
        &own_var("PATH").unwrap_or_default(),
        installer_place().as_deref(),
    )
}

/// Where Muse's installer puts its launcher for whoever runs the tests: `~/.local/bin` on
/// a Mac (`install.sh`), `%LOCALAPPDATA%\Programs\muse` on Windows (`install.ps1`).
fn installer_place() -> Option<PathBuf> {
    if cfg!(windows) {
        own_var("LOCALAPPDATA").map(|local| Path::new(&local).join("Programs").join("muse"))
    } else {
        own_var("HOME").map(|home| Path::new(&home).join(".local").join("bin"))
    }
}

/// [`installed`], on the `PATH` `path` and the installer's folder `place`: the first `muse`
/// on the `PATH` (on Windows its batch launcher, `muse.cmd`), as the product's launch takes
/// it (`cf_harness::muse::launch`), its link followed to the launcher it names, and the
/// `.muse-version` beside that launcher and the `muse-bin-<version>` (`.exe` on Windows)
/// it names. The installer's folder is read only where the `PATH` has no `muse`. So a run
/// is pointed at another Muse (one downloaded into a scratch folder, T-905) by its `PATH`,
/// and the person's home and Muse are left as they are.
pub(crate) fn installed_on(
    path: &str,
    place: Option<&Path>,
) -> std::result::Result<Installed, String> {
    let launcher = match crate::live::machine::find("muse", path, Some(".CMD;.EXE")) {
        Some(found) => found,
        None => place
            .ok_or("no muse on the PATH, and no folder of its installer to find Muse Code in")?
            .join(if cfg!(windows) { "muse.cmd" } else { "muse" }),
    };
    let real = files::real_path(&launcher);
    let bin = real.parent().unwrap_or(Path::new("."));
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let version = fs::read_to_string(bin.join(".muse-version"))
        .map(|line| line.trim().to_owned())
        .map_err(|_| {
            format!(
                "Muse Code is not installed on this machine (no .muse-version beside {})",
                launcher.display()
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

    /// Whether `words` are the daemon's read of the models (`muse serve`) on an echo run, which
    /// the stand-in answers itself ([`serve_list`]); a live run reads the real Muse's.
    pub fn lists_models(&self, words: &[String]) -> bool {
        matches!(self.run, Run::Echo { .. }) && words.first().map(String::as_str) == Some("serve")
    }

    /// Writes down the command line `words`, as the Mac's stand-in writes it.
    pub fn note(&self, words: &[String]) -> std::io::Result<()> {
        use std::io::Write as _;
        crate::door::append(&self.argv)?
            .write_all(format!("{}\u{1e}", words.join("\u{1f}")).as_bytes())
    }

    /// Writes down the window's process, this program's, and Muse's, its child's.
    pub fn note_pids(&self, window: u32, muse: u32) -> std::io::Result<()> {
        use std::io::Write as _;
        crate::door::append(pids_beside(&self.argv))?
            .write_all(format!("{window} {muse}\n").as_bytes())
    }

    /// What Muse is run with for the command line `words`: on echo, its turns the rig's
    /// length (`CF_TEST_MUSE_DELAY_MS` where a case sets it), its reminder roster empty
    /// (Muse 1.4.15's blocking end-of-turn reminders run on the echo provider too, never
    /// call `submit_reminder_decision`, and add three more echo turns to each turn), and
    /// no model or effort, which echo refuses; live, the words as they are.
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
            "-c".to_owned(),
            r#"run.reminder_roster={"agents":[]}"#.to_owned(),
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
    crate::door::make_dir(dir)?;
    crate::door::copy_keeping_mode(program, &pinned)?;
    crate::door::write(dir.join(".muse-version"), format!("{}\n", install.version))?;
    crate::door::write(
        dir.join("muse.cmd"),
        format!("@\"{}\" %*\r\n", pinned.display()),
    )?;
    let stand_in = StandIn {
        binary: install.binary.clone(),
        argv: argv.to_owned(),
        run,
        rig_home: rig_home.to_owned(),
    };
    crate::door::write(StandIn::file_of(&pinned), stand_in.to_json().to_string())
}

/// The models the rig's echo Muse lists when the daemon reads them as a dialog opens (A4-28,
/// T-1329). Muse's own list is its account's, fetched with a login the rig's home has not: a
/// fresh Muse lists none (`source: bundledCatalog`, `models: []`, 0.4 s measured on Muse
/// 1.4.14), and the daemon refuses a pick on a harness that lists none. The stand-in lists the
/// model the rig's cases seat, as the other stand-ins list theirs.
pub fn echo_list() -> Value {
    json!({
        "providerId": "meta",
        "profileId": "stand-in",
        "source": "providerCatalog",
        "models": [{
            "modelId": "muse-spark-1.3",
            "displayLabel": "Muse Spark 1.3",
            "variants": ["minimal", "low", "medium", "high"],
        }],
    })
}

/// The reply of the rig's echo Muse to one line of `muse serve` (JSON-RPC over stdio, the
/// daemon's read of the models): `initialize` and `model/list` are answered, a notification
/// and anything else are not.
pub fn serve_reply(line: &str) -> Option<String> {
    let request: Value = serde_json::from_str(line).ok()?;
    let id = request.get("id")?.clone();
    let result = match request.get("method")?.as_str()? {
        "initialize" => json!({ "serverInfo": { "name": "muse", "version": "stand-in" } }),
        "model/list" => echo_list(),
        _ => return None,
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

/// `muse serve` as the rig's echo Muse runs it, which a list is read from: each request of the
/// standard input answered ([`serve_reply`]) until the input ends.
pub fn serve_list() -> std::io::Result<()> {
    use std::io::{BufRead as _, Write as _};
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        if let Some(reply) = serve_reply(&line?) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// The part of the rig's Muse script that answers the daemon's read of the models
/// ([`echo_list`]): `muse serve` is the host a list is read from, and no window. Before the
/// command line is written down, so that no case that counts the windows' command lines counts
/// a read.
fn serve_list_script() -> String {
    format!(
        r#"if [ "$1" = serve ]; then
  models='{models}'
  while IFS= read -r line; do
    id=${{line#*\"id\":}}; id=${{id%%[,\}}]*}}
    case "$line" in
      *'"method":"initialize"'*) printf '{{"id":%s,"jsonrpc":"2.0","result":{{"serverInfo":{{"name":"muse","version":"stand-in"}}}}}}\n' "$id" ;;
      *'"method":"model/list"'*) printf '{{"id":%s,"jsonrpc":"2.0","result":%s}}\n' "$id" "$models" ;;
    esac
  done
  exit 0
fi"#,
        models = echo_list()
    )
}

/// Writes the rig's Muse in `dir`: its launcher, the version line, and the binary that
/// runs `install`'s on echo, each turn `delay_ms` long, writing each command line it
/// is given to `argv` (its words split by a unit separator, each line ended by a record
/// separator: a first message has lines of its own). Its settings and sessions are in the
/// rig's home (`rig_home`, by its real path), as the live one's are
/// ([`write_live_stand_in`]), whatever folders a harness's home gives the rig's windows.
pub fn write_stand_in(
    dir: &Path,
    install: &Installed,
    delay_ms: u64,
    rig_home: &Path,
    argv: &Path,
) -> Result {
    if cfg!(windows) {
        let run = Run::Echo { turn_ms: delay_ms };
        // An echo run keeps nothing of Muse's in the rig's home, which is named all the same.
        return write_windows(dir, install, run, argv, rig_home);
    }
    let pinned = dir.join(format!("muse-bin-{}", install.version));
    crate::door::write(dir.join(".muse-version"), format!("{}\n", install.version))?;
    crate::door::write_executable(
        dir.join("muse"),
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", pinned.display()),
    )?;
    let script = format!(
        r#"#!/bin/bash
# The rig's Muse: the machine's, on its fake model. Echo takes no model or effort.
{serve}
( IFS=$'\x1f'; printf '%s\x1e' "$*" ) >> '{argv}'
export XDG_CONFIG_HOME='{rig}/.config' XDG_DATA_HOME='{rig}/.local/share' XDG_CACHE_HOME='{rig}/.cache'
printf '%s %s\n' $$ $$ >> '{pids}'
words=()
while [ $# -gt 0 ]; do
  case "$1" in
    --model|--reasoning-effort) shift 2 ;;
    *) words+=("$1"); shift ;;
  esac
done
{repaint}
exec '{binary}' --provider echo --echo-delay-ms "${{CF_TEST_MUSE_DELAY_MS:-{delay_ms}}}" -c 'run.reminder_roster={{"agents":[]}}' "${{words[@]}}"
"#,
        serve = serve_list_script(),
        argv = argv.display(),
        pids = pids_beside(argv).display(),
        rig = rig_home.display(),
        binary = install.binary.display(),
        repaint = REPAINT_AT_REST,
    );
    crate::door::write_executable(&pinned, script)
}

/// The part of the rig's Muse that repaints at rest when a case asks
/// (`CF_TEST_MUSE_REPAINT_MS=<ms>`): Muse 1.4.14, at rest after a command it ran in the
/// background was stopped, writes colour resets every 50 ms and shows the same (T-1081,
/// T-1139). Echo never works in the background, so it never gets there, and the window's
/// terminal is written the same bytes by a sibling of Muse's, which ends with it (it looks
/// for Muse's pid, this shell's, which `exec` keeps).
const REPAINT_AT_REST: &str = r#"if [ -n "${CF_TEST_MUSE_REPAINT_MS:-}" ]; then
  nap=$(awk -v ms="$CF_TEST_MUSE_REPAINT_MS" 'BEGIN { printf "%.3f", ms / 1000 }')
  ( while kill -0 $$ 2>/dev/null; do printf '\033[39m\033[49m\033[59m\033[0m'; /bin/sleep "$nap"; done ) &
fi"#;

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
    crate::door::write(dir.join(".muse-version"), format!("{}\n", install.version))?;
    crate::door::write_executable(
        dir.join("muse"),
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", pinned.display()),
    )?;
    let script = format!(
        r#"#!/bin/bash
# The rig's Muse, live: the machine's, on its real model, the person's keychain login.
( IFS=$'\x1f'; printf '%s\x1e' "$*" ) >> '{argv}'
printf '%s %s\n' $$ $$ >> '{pids}'
# Its sessions go where the daemon reads them: the data folder the window was given, which a
# harness's home of the run's own may have moved for the whole rig (Devin's), by its real
# path; the rig's own where it was given none, or the person's.
data="${{XDG_DATA_HOME:-}}"
case "$data" in ''|'{real}'|'{real}'/*) data='{rig}/.local/share' ;; esac
data="$(cd "$data" 2>/dev/null && pwd -P || printf '%s' "$data")"
case "$data" in '{real}'|'{real}'/*) data='{rig}/.local/share' ;; esac
export HOME='{real}' XDG_CONFIG_HOME='{rig}/.config' XDG_DATA_HOME="$data" XDG_CACHE_HOME='{rig}/.cache'
exec '{binary}' "$@"
"#,
        argv = argv.display(),
        pids = pids_beside(argv).display(),
        real = real_home.display(),
        rig = rig_home.display(),
        binary = install.binary.display(),
    );
    crate::door::write_executable(&pinned, script)
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

/// How each run of a SessionStart hook ended, in the order the log wrote them: `completed`
/// or `failed`, and what Muse says where it failed (`hook_run_terminal`). Muse 1.4.14 ends
/// a hook that printed more than 16,384 bytes as `failed`, with `output_too_large`, and
/// takes nothing of what it printed.
pub fn hook_runs(records: &[Value]) -> Vec<(String, Option<String>)> {
    records
        .iter()
        .filter(|record| kind(record) == "run/hook_run_terminal")
        .filter(|record| record["payload"]["event"]["event"] == "SessionStart")
        .map(|record| {
            let event = &record["payload"]["event"];
            (
                event["status"].as_str().unwrap_or_default().to_owned(),
                event["error"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

/// The context the SessionStart hooks gave the session, each hook's block by the number
/// Muse names it with (`hook:session_start:session:<n>`), in that order: a block a later
/// start wrote again (a resume) is its last text.
pub fn hook_blocks(records: &[Value]) -> Vec<(u32, String)> {
    let mut blocks: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
    for record in records {
        if kind(record) != "run/context_block_updated" {
            continue;
        }
        let event = &record["payload"]["event"];
        let number = event["id"]
            .as_str()
            .and_then(|id| id.strip_prefix("hook:session_start:session:"))
            .and_then(|number| number.parse().ok());
        if let (Some(number), Some(text)) = (number, event["text"].as_str()) {
            blocks.insert(number, text.to_owned());
        }
    }
    blocks.into_iter().collect()
}

/// The `order` each SessionStart block of the log was written with, by its number, in the
/// order the log wrote them (a block written again, as a resume does, again): Muse puts its
/// developer context blocks in a request by this number, ascending (the `order` of
/// `run_context_messages` in each `model_request_configured` is so, 85 to 4,294,967,295), and
/// gives a hook's block `900000 + <number>` by the hook's place among those that printed.
pub fn hook_block_orders(records: &[Value]) -> Vec<(u32, u64)> {
    records
        .iter()
        .filter(|record| kind(record) == "run/context_block_updated")
        .filter_map(|record| {
            let event = &record["payload"]["event"];
            let number = event["id"]
                .as_str()?
                .strip_prefix("hook:session_start:session:")?
                .parse()
                .ok()?;
            Some((number, event["order"].as_u64()?))
        })
        .collect()
}

/// The bytes of developer context each full model request of the log carried (the
/// `model_input_trace_recorded` of each: its `context` lane, which holds the blocks the
/// SessionStart hooks gave and Muse's own), in order: what reached the wire, not only what
/// the log says a hook gave.
pub fn request_context_bytes(records: &[Value]) -> Vec<u64> {
    records
        .iter()
        .filter(|record| kind(record) == "run/model_input_trace_recorded")
        .filter(|record| record["payload"]["event"]["scope"] == "full_request")
        .filter_map(|record| {
            record["payload"]["event"]["bounded"]["aggregates"]
                .as_array()?
                .iter()
                .find(|lane| lane["logical_lane"]["value"] == "context")?["byte_count"]
                .as_u64()
        })
        .collect()
}

/// The question tools each model request of the log offered, in order: the names among
/// its `toolset.active_tools` (`run/model_request_configured`) Muse asks the person with,
/// `request_user_input` and the `request_user_input_async` `--yolo` adds (1.4.14).
pub fn question_tools(records: &[Value]) -> Vec<Vec<String>> {
    records
        .iter()
        .filter(|record| kind(record) == "run/model_request_configured")
        .map(|record| {
            record["payload"]["event"]["toolset"]["active_tools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|tool| tool.starts_with("request_user_input"))
                .map(str::to_owned)
                .collect()
        })
        .collect()
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

/// The share of its window or of its week at which a live run never starts: a run may use
/// all of Muse's allowance (T-1123, the owner's decision of 2026-11-02), but every rig that runs the
/// machine's Muse asks first ([`guard_allowance`]), so no case starts on an allowance that
/// is used up (T-1076: `live close-early` had no such check, ran at 99% of the week, and
/// its task never finished; only `live muse-window` asked).
pub const STOP_AT_PERCENT: u64 = 100;

/// What `usage` (`muse usage --json`) says is left of the allowance, in words, or why a live
/// run spends none of it: its window or its week is used up ([`STOP_AT_PERCENT`]), with when
/// each resets, or no allowance at all.
pub fn allowance_left(usage: &Value) -> std::result::Result<String, String> {
    match used_percent(usage) {
        Some((window, weekly)) if window >= STOP_AT_PERCENT || weekly >= STOP_AT_PERCENT => {
            Err(format!(
                "the Muse allowance is at {window}% of its window and {weekly}% of its week, and a live run stops at {STOP_AT_PERCENT}% before it spends any (resets: {} and {})",
                usage["allowance"]["window"]["resetsAtMs"], usage["allowance"]["weekly"]["resetsAtMs"]
            ))
        }
        Some((window, weekly)) => Ok(format!("{window}% of the window, {weekly}% of the week")),
        None => Err(format!("Muse says no allowance: {usage}")),
    }
}

/// The allowance a live rig's Muse windows would spend, asked of `install` with the person's
/// login (found by `real_home`) and the settings in `config_home` before any window opens
/// ([`usage`], no model turn): what is left, in words, or the refusal that stops the run
/// ([`allowance_left`]). The rig asks it as it writes its live launcher
/// ([`crate::rig::Config::muse_live`]), so every case that runs Muse is guarded without
/// asking for it.
pub fn guard_allowance(
    install: &Installed,
    real_home: &Path,
    config_home: &Path,
) -> Result<String> {
    let usage = usage(install, real_home, config_home)?;
    allowance_left(&usage).map_err(Error::Daemon)
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
    use crate::rig::{Project, Rig};
    use crate::Result;

    /// The worker's agent, as the roster names it.
    pub const WORKER: &str = "muse-worker";

    /// The handle the Muse worker has on the staff: named by its model and effort as Muse
    /// runs them (spec harness-models), not by the stand-in's name.
    pub const WORKER_HANDLE: &str = "muse-spark-1-3-minimal";

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
        let mut agents =
            vec![json!({ "id": "chief", "kind": "claude-code", "model": "fake-chief" })];
        // A worker on a model that lists efforts and is given none is refused by the daemon.
        agents.extend(crate::live::agents::with_efforts(rig, &[worker], |_| {}));
        let roster = json!({ "schemaVersion": 1, "agents": agents });
        rig.stand_ins(&roster)?;
        let project = Project::open(rig, "chief", json!({}))?;
        let added = project.add_member(WORKER)?;
        if added["member"]["handle"] != WORKER_HANDLE {
            return Err(crate::Error::Daemon(format!(
                "the Muse worker is not @{WORKER_HANDLE}: {added}"
            )));
        }
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
        Ok(project.lane(WORKER_HANDLE)?["pane"].clone())
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
            Ok(begun && project.lane(WORKER_HANDLE)?["activity"]["state"] == "working")
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
    #[cfg(unix)]
    use super::serve_list_script;
    use super::{allowance_left, question_tools, used_percent, Run, StandIn};
    use super::{echo_list, serve_reply};
    #[cfg(unix)]
    use crate::files;

    #[test]
    fn the_echo_muse_answers_the_daemons_read_of_the_models_and_nothing_else() {
        let reply = |line: &str| {
            serve_reply(line).map(|text| serde_json::from_str::<serde_json::Value>(&text).unwrap())
        };
        let initialized =
            reply(r#"{"id":1,"jsonrpc":"2.0","method":"initialize","params":{}}"#).unwrap();
        assert_eq!(initialized["id"], 1);
        let listed =
            reply(r#"{"id":2,"jsonrpc":"2.0","method":"model/list","params":{}}"#).unwrap();
        assert_eq!(listed["id"], 2);
        assert_eq!(listed["result"], echo_list());
        assert_eq!(listed["result"]["source"], "providerCatalog");
        assert_eq!(listed["result"]["models"][0]["modelId"], "muse-spark-1.3");
        assert!(reply(r#"{"jsonrpc":"2.0","method":"initialized"}"#).is_none());
        assert!(reply("not json").is_none());
    }

    /// The Mac's script answers the same lines the same way, whatever order the keys come in.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods)] // A test runs the script it wrote, with bash.
    fn the_script_of_the_mac_answers_as_the_program_of_windows_does() {
        use std::io::Write as _;
        use std::process::{Command, Stdio};
        let script = format!(
            "set -- serve --provider meta --no-session-log\n{}\n",
            serve_list_script()
        );
        let mut bash = Command::new("/bin/bash")
            .args(["-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let requests = [
            r#"{"id":1,"jsonrpc":"2.0","method":"initialize","params":{"clientInfo":{"name":"consensflow"}}}"#,
            r#"{"jsonrpc":"2.0","method":"initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"model/list","params":{}}"#,
        ];
        let mut stdin = bash.stdin.take().unwrap();
        for line in requests {
            writeln!(stdin, "{line}").unwrap();
        }
        drop(stdin);
        let out = bash.wait_with_output().unwrap();
        let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(
            (lines[0]["id"].clone(), lines[1]["id"].clone()),
            (json!(1), json!(2))
        );
        assert_eq!(lines[1]["result"], echo_list());
    }

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
        let home = crate::door::made("cf-test-").unwrap();
        install(&home.path().join(".local").join("bin"), "1.4.13-R6996.1");
        let scratch = crate::door::made("cf-test-").unwrap();
        install(scratch.path(), "1.4.14-R7309.1");
        let empty = crate::door::made("cf-test-").unwrap();
        let place = home.path().join(".local").join("bin");
        let home_of = Some(place.as_path());
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
        let real = crate::door::made("cf-test-").unwrap();
        install(real.path(), "1.4.14-R7309.1");
        let linked = crate::door::made("cf-test-").unwrap();
        std::os::unix::fs::symlink(real.path().join("muse"), linked.path().join("muse")).unwrap();
        let found = installed_on(linked.path().to_str().unwrap(), None).unwrap();
        assert_eq!(found.version, "1.4.14-R7309.1");
        assert!(found
            .binary
            .starts_with(std::fs::canonicalize(real.path()).unwrap()));
    }

    /// T-1081: the rig's Muse on echo keeps its settings and sessions in the rig's home,
    /// whatever XDG folders the windows were given (a Devin home moves them all). The live one
    /// keeps its settings there too, and its sessions in the data folder the window was given,
    /// where the daemon reads them (T-1152); the rig links that folder's `muse` to its own
    /// (`give_muse_its_folders`), so both meet the folders the live kit's Muse does.
    #[cfg(unix)]
    #[test]
    fn the_rigs_muse_on_echo_keeps_its_folders_in_the_rigs_home_as_the_live_one_does() {
        let folder = crate::door::made("cf-test-").unwrap();
        let said = folder.path().join("said");
        let binary = folder.path().join("real-muse");
        files::write_executable(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s %s' \"$XDG_CONFIG_HOME\" \"$XDG_DATA_HOME\" > '{}'\n",
                said.display()
            ),
        )
        .unwrap();
        let install = super::Installed {
            binary,
            version: "1.4.14-R7309.1".to_owned(),
            stand_in: None,
        };
        let rig = folder.path().join("rig-home");
        let bin = folder.path().join("bin");
        files::make_dir(&bin).unwrap();
        let argv = folder.path().join("argv.log");
        let ran = |launcher: &Path| {
            let ran = crate::process::Run::new(launcher)
                .var("XDG_CONFIG_HOME", "/a-home/config")
                .var("XDG_DATA_HOME", "/a-home/data")
                .run()
                .unwrap();
            assert_eq!(ran.code, Some(0), "{ran}");
            std::fs::read_to_string(&said).unwrap()
        };
        let wanted = format!("{0}/.config {0}/.local/share", rig.display());
        super::write_stand_in(&bin, &install, 10, &rig, &argv).unwrap();
        assert_eq!(ran(&bin.join("muse")), wanted, "on echo");
        super::write_live_stand_in(&bin, &install, folder.path(), &rig, &argv).unwrap();
        let live = format!("{}/.config /a-home/data", rig.display());
        assert_eq!(ran(&bin.join("muse")), live, "live");
    }

    /// The rig's Muse, run as a window runs it, with a program of 2 s in Muse's place: long
    /// enough that a slow machine's repaints come many times (GitHub's macOS runner made 6 in
    /// 0.6 s at 30 ms, CI run 41925722174).
    #[cfg(unix)]
    fn the_rigs_muse_run(repaint_ms: Option<&str>) -> crate::process::Ran {
        let dir = crate::door::made("cf-test-").unwrap();
        let install = super::Installed {
            binary: dir.path().join("program"),
            version: "9.9.9".to_owned(),
            stand_in: None,
        };
        files::write_executable(&install.binary, "#!/bin/sh\nsleep 2\n").unwrap();
        super::write_stand_in(
            dir.path(),
            &install,
            1_000,
            &dir.path().join("rig-home"),
            &dir.path().join("argv.log"),
        )
        .unwrap();
        let mut run = crate::process::Run::new(dir.path().join("muse-bin-9.9.9"))
            .finding_programs()
            .limit(std::time::Duration::from_secs(20));
        if let Some(ms) = repaint_ms {
            run = run.var("CF_TEST_MUSE_REPAINT_MS", ms);
        }
        run.run().unwrap()
    }

    /// T-1139: Muse at rest after a stopped command writes colour resets every 50 ms and
    /// shows the same; echo never gets there, so a case that asks has the terminal written
    /// them (`CF_TEST_MUSE_REPAINT_MS`) for as long as Muse lives, and none otherwise.
    #[cfg(unix)]
    #[test]
    fn the_rigs_muse_repaints_at_rest_when_asked_for_as_long_as_it_lives_and_not_otherwise() {
        let ran = the_rigs_muse_run(Some("30"));
        let times = ran
            .stdout
            .matches("\x1b[39m\x1b[49m\x1b[59m\x1b[0m")
            .count();
        assert!(times >= 10, "{times} repaints in 2 s at 30 ms: {ran}");
        assert!(!ran.timed_out, "{ran}");
        assert!(!ran.held, "it went on after Muse was gone: {ran}");
        let ran = the_rigs_muse_run(None);
        assert!(ran.stdout.is_empty(), "{ran}");
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
                    r#"run.reminder_roster={"agents":[]}"#,
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

    /// Each request's question tools, as Muse 1.4.14 on echo wrote them (T-790's probe):
    /// both under `--yolo`, none with `--disable-request-user-input`.
    #[test]
    fn the_question_tools_of_each_request_are_read_from_its_toolset() {
        let request = |tools: &[&str]| {
            json!({ "payload": { "kind": "run", "event": {
                "kind": "model_request_configured",
                "toolset": { "source": "default", "mode": "all", "active_tools": tools },
            } } })
        };
        let log = [
            request(&[
                "read_file",
                "request_user_input",
                "request_user_input_async",
                "bash",
            ]),
            json!({ "payload": { "kind": "run", "event": { "kind": "started" } } }),
            request(&["read_file", "bash"]),
        ];
        assert_eq!(
            question_tools(&log),
            [
                vec![
                    "request_user_input".to_owned(),
                    "request_user_input_async".to_owned()
                ],
                Vec::new(),
            ]
        );
    }

    /// T-1076's guard at T-1123's share: a run may use all of the allowance, so it stops
    /// only where the window or the week is used up, and says when each resets; under
    /// that, and on a window nothing was spent of yet, it runs.
    #[test]
    fn a_live_run_stops_where_the_window_or_the_week_is_used_up_and_runs_under_it() {
        let at = |window: u64, weekly: u64| {
            json!({ "schemaVersion": 1, "allowance": {
                "window": { "usedPercent": window, "resetsAtMs": 1 },
                "weekly": { "usedPercent": weekly, "resetsAtMs": 2 },
            } })
        };
        let refused = allowance_left(&at(10, 100)).unwrap_err();
        assert!(refused.contains("100% of its week"), "{refused}");
        assert!(refused.contains("stops at 100%"), "{refused}");
        assert!(refused.contains("resets: 1 and 2"), "{refused}");
        assert!(allowance_left(&at(100, 0)).is_err(), "100% of the window");
        assert!(allowance_left(&at(0, 100)).is_err(), "100% of the week");
        assert_eq!(
            allowance_left(&at(50, 50)).unwrap(),
            "50% of the window, 50% of the week"
        );
        assert!(allowance_left(&at(99, 99)).is_ok(), "99% of each runs");
        let fresh =
            json!({ "schemaVersion": 1, "allowance": null, "reason": "server_sent_no_usage" });
        assert!(allowance_left(&fresh).is_ok());
        let none = json!({ "schemaVersion": 1, "allowance": null, "reason": "no_allowance" });
        assert!(allowance_left(&none).is_err());
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
        let person = crate::door::made("cf-test-").unwrap();
        let rig = crate::door::made("cf-test-").unwrap();
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
