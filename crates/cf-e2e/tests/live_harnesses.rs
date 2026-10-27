//! Live: what the Admin page does with the machine's own harness CLIs, asked of the native
//! daemon over its API as the page asks it. It is opt-in (`cargo xtask live harnesses`,
//! `npm run live:harnesses`): it needs each harness named installed, so it is ignored unless
//! asked for and is no part of `check` or of CI. It spends no model quota, and logs nothing in.
//!
//! - **The look** (`POST /api/harnesses/check`, which runs each CLI's `--version` to its end,
//!   as `execFile` does): each harness named has a row that says it is installed, where its
//!   program is, and the version the program says when the run asks it itself.
//! - **The update** (`POST /api/harnesses/update`, which runs the CLI's own updater to its
//!   end): only where it is asked for, with `--update pi`, and only of a Pi that its own
//!   installer made, which `pi update` brings to its latest release: the daemon is given
//!   that Pi's folder first on its `PATH`, the row it shows must be that program, and the
//!   update must come to `updated` or `unchanged`, never `failed`, with the version it says
//!   after the one the program says when the run asks it again. On Windows this is a `.cmd`
//!   that starts Node and an installer, the longest tree this crate starts.
//!
//! Every program the daemon starts for this runs in a job of its own (cf-process); this is
//! what shows, on the real CLIs, that none of them needs to leave one (T-353).
//!
//! ```text
//! cargo xtask live harnesses                    the look, at every harness
//! cargo xtask live harnesses codex pi           at some
//! cargo xtask live harnesses pi --update pi     and the update of a managed Pi
//! cargo xtask windows --host zeewin -- cargo xtask live harnesses --update pi
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::http::{origin, Http};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::process::{own_var, Run};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// What the daemon is given of the machine, besides the home of its own: where the
/// programs are and what they read their settings by.
const MACHINE: [&str; 21] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "USER",
    "LOGNAME",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CommonProgramFiles",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "CODEX_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

/// How long a look has to be answered, and an update (the daemon gives one ten minutes).
const LOOKS: Duration = Duration::from_secs(120);
const UPDATES: Duration = Duration::from_secs(660);

/// A daemon on a home of its own, which sees the machine's programs and logins.
struct Admin {
    daemon: Daemon,
    _home: Home,
    url: String,
    token: String,
    http: Http,
}

impl Admin {
    /// Starts the daemon, with `first` first on its `PATH` where there is one.
    fn start(first: Option<&Path>, patience: Duration) -> Result<Self> {
        let home = Home::new()?;
        let mut vars: Vec<(String, String)> = home
            .vars()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_string_lossy().into_owned()))
            .collect();
        for name in MACHINE {
            if let Some(value) = own_var(name) {
                vars.retain(|(known, _)| known != name);
                vars.push((name.to_owned(), value));
            }
        }
        if let Some(folder) = first {
            let path = std::env::join_paths(
                std::iter::once(folder.to_path_buf())
                    .chain(std::env::split_paths(&own_var("PATH").unwrap_or_default())),
            )
            .map_err(|failed| {
                Error::Daemon(format!("a PATH with {}: {failed}", folder.display()))
            })?;
            vars.retain(|(known, _)| known != "PATH");
            vars.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
        }
        let mut daemon = home.daemon_with(vars)?;
        let handle = daemon.handle(Duration::from_secs(60))?;
        Ok(Self {
            url: handle["url"].as_str().unwrap_or_default().to_owned(),
            token: handle["token"].as_str().unwrap_or_default().to_owned(),
            http: Http::waiting(patience),
            daemon,
            _home: home,
        })
    }

    /// What the page's route `path` answers to `body`, as JSON.
    fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{path}", origin(&self.url));
        let reply = self
            .http
            .send("POST", &url, Some(&self.token), Some(body))?;
        if reply.status != 200 {
            return Err(Error::Daemon(format!(
                "POST {path}: {} {}",
                reply.status, reply.text
            )));
        }
        reply.json()
    }

    fn stop(mut self) -> Result {
        self.daemon.stop()
    }
}

/// The harnesses the command line names, or all five.
fn named() -> Result<Vec<Harness>> {
    let words = live::list("harness");
    if words.is_empty() {
        return Ok(Harness::ALL.to_vec());
    }
    words
        .iter()
        .map(|word| {
            Harness::from_name(word)
                .ok_or_else(|| Error::Daemon(format!("no harness is called {word}")))
        })
        .collect()
}

/// The first line a program says to `--version`, or why it said none.
fn says(program: &Path) -> String {
    Run::new(program)
        .arg("--version")
        .inheriting_env()
        .run()
        .ok()
        .filter(|ran| ran.code == Some(0))
        .and_then(|ran| ran.stdout.lines().next().map(|line| line.trim().to_owned()))
        .unwrap_or_else(|| "none".to_owned())
}

/// The row of `harness` in the daemon's answer.
fn row(answer: &Value, harness: Harness) -> Option<&Value> {
    answer["harnesses"]
        .as_array()?
        .iter()
        .find(|row| row["id"] == harness.name())
}

/// The look: a row for each harness, held to what its program says itself.
fn look(admin: &Admin, harnesses: &[Harness], log: &Log) -> Result<Vec<Check>> {
    let answer = admin.post("/api/harnesses/check", &json!({ "refresh": true }))?;
    let mut checks = Vec::new();
    for &harness in harnesses {
        let found = log.found(harness)?;
        let expected = found.version();
        let shown = row(&answer, harness).cloned().unwrap_or(Value::Null);
        let version = shown["version"]["value"].as_str().unwrap_or_default();
        let held = shown["installed"] == true
            && shown["version"]["state"] == "checked"
            && !version.is_empty()
            && expected.contains(version);
        let detail = format!(
            "installed {}, version {} ({}), program {}; the program says {expected:?}",
            shown["installed"],
            shown["version"]["state"],
            if version.is_empty() { "none" } else { version },
            shown["path"]
        );
        checks.push(Check::new(harness.name(), "the look", held, detail));
    }
    Ok(checks)
}

/// The folder Pi's own installer keeps its launcher in, in the user's home.
fn managed_pi() -> Option<PathBuf> {
    let home = own_var("USERPROFILE").or_else(|| own_var("HOME"))?;
    let folder = Path::new(&home).join(".pi").join("agent").join("bin");
    let launcher = folder.join(if cfg!(windows) { "pi.cmd" } else { "pi" });
    launcher.is_file().then_some(folder)
}

/// The update of a Pi its own installer made: it must come to `updated` or `unchanged`.
fn update_pi(log: &Log) -> Result<Vec<Check>> {
    let Some(folder) = managed_pi() else {
        return Ok(vec![Check::failed(
            "pi",
            "the update",
            "this machine has no Pi made by Pi's own installer (<home>/.pi/agent/bin)",
        )]);
    };
    let launcher = folder.join(if cfg!(windows) { "pi.cmd" } else { "pi" });
    let admin = Admin::start(Some(&folder), UPDATES)?;
    let before = says(&launcher);
    log.say(format!("pi (managed) is {} ({before})", launcher.display()));
    let seen = admin.post(
        "/api/harnesses/check",
        &json!({ "id": "pi", "refresh": true }),
    )?;
    let shown = row(&seen, Harness::Pi).cloned().unwrap_or(Value::Null);
    let first = shown["path"]
        .as_str()
        .is_some_and(|path| Path::new(path).starts_with(&folder));
    let updated = admin.post("/api/harnesses/update", &json!({ "id": "pi" }))?;
    let result = &updated["result"];
    let after = says(&launcher);
    let state = result["state"].as_str().unwrap_or("none").to_owned();
    let said_after = result["after"].as_str().unwrap_or_default();
    let held = first
        && matches!(state.as_str(), "updated" | "unchanged")
        && result["reason"].is_null()
        && !said_after.is_empty()
        && after.contains(said_after);
    let detail = format!(
        "the row's program was the managed one: {first}; state {state}, {} -> {}, the program \
         says {before:?} before and {after:?} after; command {}; reason {}",
        result["before"], result["after"], result["command"], result["reason"]
    );
    let checks = vec![Check::new("pi", "the update", held, detail)];
    if !held {
        log.block(
            "what the update wrote",
            result["output"].as_str().unwrap_or_default(),
        );
    }
    admin.stop()?;
    Ok(checks)
}

#[test]
#[ignore = "opt-in: runs the machine's harness CLIs, and `--update pi` updates its managed Pi; cargo xtask live harnesses (npm run live:harnesses)"]
fn the_admin_page_looks_at_each_harness_as_its_program_says_and_updates_a_managed_pi() -> Result {
    let log = Log::begin("live harnesses")?;
    let harnesses = named()?;
    let mut checks = Vec::new();
    let admin = Admin::start(None, LOOKS)?;
    checks.extend(look(&admin, &harnesses, &log)?);
    admin.stop()?;
    if live::list("update").iter().any(|word| word == "pi") {
        checks.extend(update_pi(&log)?);
    }
    report::conclude(&log, "what each check came to", &checks)
}
