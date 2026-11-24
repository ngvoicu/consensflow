//! Live: what the Admin page does with the machine's own harness CLIs, asked of the native
//! daemon over its API as the page asks it. It is opt-in (`cargo xtask live harnesses`,
//! `npm run live:harnesses`): it needs each harness named installed, so it is ignored unless
//! asked for and is no part of `check` or of CI. It spends no model quota, and logs nothing in.
//!
//! - **The look** (`POST /api/harnesses/check`, which runs each CLI's `--version` to its end,
//!   as `execFile` does): each harness named has a row that says it is installed, where its
//!   program is, and the version the program says when the run asks it itself.
//!
//! Every program the daemon starts for this runs in a job of its own (cf-process); this is
//! what shows, on the real CLIs, that none of them needs to leave one (T-353).
//!
//! ```text
//! cargo xtask live harnesses                    the look, at every harness
//! cargo xtask live harnesses codex pi           at some
//! cargo xtask windows --host zeewin -- cargo xtask live harnesses
//! ```

use std::time::Duration;

use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::http::{origin, Http};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::process::own_var;
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

/// How long a look has to be answered.
const LOOKS: Duration = Duration::from_secs(120);

/// A daemon on a home of its own, which sees the machine's programs and logins.
struct Admin {
    daemon: Daemon,
    _home: Home,
    url: String,
    token: String,
    http: Http,
}

impl Admin {
    /// Starts the daemon, which sees the machine's programs.
    fn start() -> Result<Self> {
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
        let mut daemon = home.daemon_with(vars)?;
        let handle = daemon.handle(Duration::from_secs(60))?;
        Ok(Self {
            url: handle["url"].as_str().unwrap_or_default().to_owned(),
            token: handle["token"].as_str().unwrap_or_default().to_owned(),
            http: Http::waiting(LOOKS),
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

/// The harnesses the command line names, or every one.
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

#[test]
#[ignore = "opt-in: runs the machine's harness CLIs; cargo xtask live harnesses (npm run live:harnesses)"]
fn the_admin_page_looks_at_each_harness_as_its_program_says() -> Result {
    let log = Log::begin("live harnesses")?;
    let harnesses = named()?;
    let mut checks = Vec::new();
    let admin = Admin::start()?;
    checks.extend(look(&admin, &harnesses, &log)?);
    admin.stop()?;
    report::conclude(&log, "what each check came to", &checks)
}
