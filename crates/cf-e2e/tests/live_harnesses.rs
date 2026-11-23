//! Live: what the Admin page does with the machine's own harness CLIs, asked of the native
//! daemon over its API as the page asks it. It is opt-in (`cargo xtask live harnesses`,
//! `npm run live:harnesses`): it needs each harness named installed, so it is ignored unless
//! asked for and is no part of `check` or of CI. It spends no model quota, and logs nothing in.
//!
//! - **The look** (`POST /api/harnesses/check`, which runs each CLI's `--version` to its end,
//!   as `execFile` does): each harness named has a row that says it is installed, where its
//!   program is, and the version the program says when the run asks it itself.
//! - **What the look does not ask** (K3, `live-kit-isolation-doors.md` section 2): before the
//!   daemon is asked about a harness, the kit resolves what the daemon will find on the search
//!   path it gave it. Muse is never looked at: the daemon's look would start it outside its file
//!   sandbox, so it is **unavailable**. Another harness is asked only where it resolves to the
//!   program the kit found installed; anything else is unavailable, with what it resolved to. An
//!   unavailable look fails nothing, is counted and printed, and a release names it as not checked.
//!   The daemon has every harness's update switches, and the launcher guard is taken around each
//!   harness looked at.
//! - **No update.** Admin's Update of a managed Pi is no part of the kit: it ran Pi's own
//!   installer on a copy of the owner's, the one live run where the Update button ran to its end,
//!   and a run asked for it (`--update`) is refused before anything starts.
//!
//! Every program the daemon starts for this runs in a job of its own (cf-process); this is
//! what shows, on the real CLIs, that none of them needs to leave one (T-353).
//!
//! ```text
//! cargo xtask live harnesses                    the look, at every harness
//! cargo xtask live harnesses codex pi           at some
//! ```

use std::path::Path;
use std::time::Duration;

use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::http::{origin, Http};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, launcher, Harness, Log};
use cf_e2e::process::own_var;
use cf_e2e::rig::HarnessHome;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// What the daemon is given of the machine, besides the home of its own: where the programs are
/// and what describes the machine. Never a variable that says where a login or a setting is
/// (`HOME`, `USERPROFILE`, `APPDATA`, a harness's own home, the XDG folders): the daemon's home
/// is the kit's, so that what the daemon reads of a harness's setting (Claude's release source in
/// `~/.claude/settings.json`) is read in the kit's home and never the owner's (T-1957).
const MACHINE: [&str; 11] = [
    "PATH",
    "USER",
    "LOGNAME",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CommonProgramFiles",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "SystemRoot",
];

/// The daemon's environment: the home's own (`home`), then the machine's `MACHINE` variables as
/// `lookup` has them, with `first` first on its `PATH` where there is one.
fn daemon_vars(
    home: Vec<(String, String)>,
    lookup: &dyn Fn(&str) -> Option<String>,
    first: Option<&Path>,
) -> Result<Vec<(String, String)>> {
    let mut vars = home;
    for name in MACHINE {
        if let Some(value) = lookup(name) {
            vars.retain(|(known, _)| known != name);
            vars.push((name.to_owned(), value));
        }
    }
    // No harness the daemon starts for its look updates or installs anything.
    for (name, value) in launcher::updates_off() {
        vars.retain(|(known, _)| known != name);
        vars.push((name.to_owned(), value.to_owned()));
    }
    if let Some(folder) = first {
        let path = std::env::join_paths(
            std::iter::once(folder.to_path_buf())
                .chain(std::env::split_paths(&lookup("PATH").unwrap_or_default())),
        )
        .map_err(|failed| Error::Daemon(format!("a PATH with {}: {failed}", folder.display())))?;
        vars.retain(|(known, _)| known != "PATH");
        vars.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
    }
    Ok(vars)
}

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
    /// Starts the daemon, with `first` first on its `PATH` where there is one.
    fn start(first: Option<&Path>, patience: Duration) -> Result<Self> {
        Self::start_in(Home::new()?, first, patience)
    }

    /// [`Admin::start`] on `home`, which the case has already put what it needs in.
    fn start_in(home: Home, first: Option<&Path>, patience: Duration) -> Result<Self> {
        let vars = daemon_vars(
            home.vars()
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_string_lossy().into_owned()))
                .collect(),
            &own_var,
            first,
        )?;
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

/// What a look at one harness asks: that harness only, looked at again.
fn check_body(harness: Harness) -> Value {
    json!({ "id": harness.name(), "refresh": true })
}

/// What the daemon will start for its look at `harness`, on its search path `path`: the program
/// the kit found installed (`installed`), or why the look is unavailable. Muse is never looked
/// at: the daemon's look starts it outside its file sandbox.
fn vouched_for(harness: Harness, installed: &Path, path: &str) -> std::result::Result<(), String> {
    if harness == Harness::Muse {
        return Err("the look would run Muse outside its sandbox, so Muse is not looked at".into());
    }
    let extensions = own_var("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    let found = cf_e2e::process::first_on_path(harness.name(), path, &extensions);
    let real = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match found {
        Some(found) if real(&found) == real(installed) => Ok(()),
        Some(found) => Err(format!(
            "the daemon would start {} (it resolves to {}), which is not the {} the kit found installed ({})",
            found.display(),
            real(&found).display(),
            harness.name(),
            installed.display()
        )),
        None => Err(format!(
            "the daemon would find no {} on its search path",
            harness.name()
        )),
    }
}

/// The look: a row for each harness, held to what its program says itself; unavailable where the
/// kit cannot vouch for what the daemon would start ([`vouched_for`]), and then never asked. The
/// owner's launchers of a harness looked at are compared before and after its look.
fn look(admin: &Admin, harnesses: &[Harness], log: &Log, path: &str) -> Result<Vec<Check>> {
    let mut checks = Vec::new();
    for &harness in harnesses {
        let found = log.found(harness)?;
        if let Err(why) = vouched_for(harness, found.command(), path) {
            checks.push(Check::unavailable(harness.name(), "the look", why));
            continue;
        }
        let guard = launcher::Guard::new(harness, found.command(), found.home());
        guard.vars()?;
        // Only the harness asked for is looked at: an unscoped refresh would run every
        // harness's CLI of the machine, which this run reports nothing of.
        let answer = admin.post("/api/harnesses/check", &check_body(harness))?;
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
        if let Err(changed) = guard.finish() {
            checks.push(Check::failed(
                harness.name(),
                "the launchers",
                changed.to_string(),
            ));
        }
    }
    Ok(checks)
}

/// A run that asks for Admin's Update (`--update`), which the kit no longer runs: refused, with
/// nothing started.
fn no_update_asked(asked: &[String]) -> Result {
    if asked.is_empty() {
        return Ok(());
    }
    Err(Error::Daemon(format!(
        "--update {} is no part of the kit: Admin's Update of a managed Pi ran Pi's own installer,          and no live run starts it; the look runs without it",
        asked.join(",")
    )))
}

#[test]
#[ignore = "opt-in: runs the machine's harness CLIs; cargo xtask live harnesses (npm run live:harnesses)"]
fn the_admin_page_looks_at_each_harness_as_its_program_says() -> Result {
    no_update_asked(&live::list("update"))?;
    let log = Log::begin("live harnesses")?;
    let harnesses = named()?;
    let admin = Admin::start(None, LOOKS)?;
    let path = own_var("PATH").unwrap_or_default();
    let checks = look(&admin, &harnesses, &log, &path)?;
    admin.stop()?;
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A look at some harnesses asks for those and no other (an unscoped refresh runs every
    /// harness's CLI of the machine).
    #[test]
    fn a_look_asks_for_the_one_harness_it_reports() {
        for harness in Harness::ALL {
            let body = check_body(harness);
            assert_eq!(body["id"], harness.name());
            assert_eq!(body["refresh"], true);
        }
    }

    /// T-1997 1: a run that asks for the update is refused before anything starts.
    #[test]
    fn live_harnesses_refuses_an_update_and_starts_nothing_for_it() {
        assert!(no_update_asked(&[]).is_ok());
        let refused = no_update_asked(&["pi".to_owned()]).unwrap_err().to_string();
        assert!(refused.contains("no part of the kit"), "{refused}");
    }

    /// T-2000 2: the look asks the daemon of no harness it cannot vouch for. Muse is never looked
    /// at; another harness is asked only where the daemon's search path resolves it to the
    /// program the kit found installed.
    #[cfg(unix)]
    #[test]
    fn the_look_asks_the_daemon_of_no_harness_it_cannot_vouch_for() {
        let root = cf_e2e::door::made("look-").unwrap();
        let installed = root.path().join("installed");
        let other = root.path().join("other");
        for folder in [&installed, &other] {
            cf_e2e::door::make_dir(folder).unwrap();
            for name in ["pi", "muse", "codex"] {
                cf_e2e::door::write_executable(folder.join(name), "#!/bin/sh\n").unwrap();
            }
        }
        cf_e2e::door::write(installed.join(".muse-version"), "1.4.14\n").unwrap();
        let path = |folders: &[&Path]| {
            std::env::join_paths(folders)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        // Muse, with its version file or without, is not looked at.
        let muse = vouched_for(Harness::Muse, &installed.join("muse"), &path(&[&installed]));
        assert!(muse.unwrap_err().contains("outside its sandbox"));
        // A `pi` first on the daemon's path that is not the installed one is not asked.
        let pi = vouched_for(
            Harness::Pi,
            &installed.join("pi"),
            &path(&[&other, &installed]),
        );
        assert!(pi.unwrap_err().contains("not the pi the kit found"));
        assert!(vouched_for(Harness::Pi, &installed.join("pi"), &path(&[&installed])).is_ok());
        // A link to the installed program is the installed program.
        let linked = root.path().join("linked");
        cf_e2e::door::make_dir(&linked).unwrap();
        cf_e2e::door::symlink(installed.join("codex"), linked.join("codex")).unwrap();
        assert!(vouched_for(Harness::Codex, &installed.join("codex"), &path(&[&linked])).is_ok());
        assert!(vouched_for(Harness::Codex, &installed.join("codex"), "").is_err());
    }

    /// T-1957: the daemon and what it starts are on the kit's home. The owner's `HOME`,
    /// `USERPROFILE`, `APPDATA`, `CODEX_HOME` and the XDG folders never replace the home's own;
    /// the machine's `PATH` and description still pass.
    #[test]
    fn the_admin_daemon_is_given_the_kits_home_and_never_the_owners() {
        let owner = |name: &str| match name {
            "HOME" | "USERPROFILE" | "APPDATA" | "LOCALAPPDATA" | "CODEX_HOME"
            | "XDG_CONFIG_HOME" | "XDG_DATA_HOME" | "HOMEDRIVE" | "HOMEPATH" => {
                Some(format!("/the-owners/{name}"))
            }
            "PATH" => Some("/usr/bin".to_owned()),
            "OS" => Some("Windows_NT".to_owned()),
            _ => None,
        };
        let home = vec![
            ("HOME".to_owned(), "/kit/home".to_owned()),
            ("USERPROFILE".to_owned(), "/kit/home".to_owned()),
            ("HOMEDRIVE".to_owned(), "K:".to_owned()),
            ("HOMEPATH".to_owned(), "\\kit\\home".to_owned()),
            ("XDG_CONFIG_HOME".to_owned(), "/kit/home/.config".to_owned()),
        ];
        let vars = daemon_vars(home, &owner, Some(Path::new("/pi/bin"))).unwrap();
        let get = |name: &str| {
            vars.iter()
                .rev()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("HOME"), Some("/kit/home"));
        assert_eq!(get("USERPROFILE"), Some("/kit/home"));
        assert_eq!(get("XDG_CONFIG_HOME"), Some("/kit/home/.config"));
        for name in ["APPDATA", "LOCALAPPDATA", "CODEX_HOME", "XDG_DATA_HOME"] {
            assert_eq!(get(name), None, "{name} is the owner's");
        }
        // Both halves of the profile path are the kit's, and what a Windows child is given of
        // the machine on top of them (`Run`'s own merge) is never the owner's drive or path.
        assert_eq!(get("HOMEDRIVE"), Some("K:"));
        assert_eq!(get("HOMEPATH"), Some("\\kit\\home"));
        let child_vars: Vec<(std::ffi::OsString, std::ffi::OsString)> =
            vars.iter().map(|(k, v)| (k.into(), v.into())).collect();
        let merged = cf_e2e::process::required_from(&child_vars, true, true, &|name| {
            owner(name).map(Into::into)
        });
        for (name, value) in &merged {
            assert!(
                !value.to_string_lossy().contains("the-owners"),
                "{name} is the owner's after Run's merge"
            );
        }
        assert!(
            vars.iter().all(|(_, value)| !value.contains("the-owners")),
            "{vars:?}"
        );
        assert_eq!(get("OS"), Some("Windows_NT"));
        assert!(get("PATH")
            .is_some_and(|path| path.starts_with("/pi/bin") && path.contains("/usr/bin")));
    }
}
