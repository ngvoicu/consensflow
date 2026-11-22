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
//!   installer made, which `pi update` brings to its latest release. The update runs on a **copy**
//!   of that installation made in the kit's own home (refused where a link is in it or a file
//!   names the owner's folder): the daemon is given the copy's folder first on its `PATH`, the
//!   row it shows must be that program, and the update must come to `updated` or `unchanged`,
//!   never `failed`, with the version it says after the one the program says when the run asks it
//!   again. The owner's launcher is read to be copied and never started. On Windows this is a
//!   `.cmd` that starts Node and an installer, the longest tree this crate starts.
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
use cf_e2e::live::{self, launcher, Harness, Log};
use cf_e2e::process::{own_var, Run};
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

/// The first line Pi's launcher `program` says to `--version`, or why it said none: told, as every
/// Pi the kit starts is, to install nothing ([`launcher::updates_off_of`]), and given the machine's
/// variables and the kit's home `home` by name, never the caller's environment.
fn says(program: &Path, home: &[(String, String)]) -> String {
    Run::new(program)
        .arg("--version")
        .machine_env()
        .vars(home.iter().cloned())
        .vars(launcher::updates_off_of(Harness::Pi).iter().copied())
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
    let mut checks = Vec::new();
    for &harness in harnesses {
        // Only the harness asked for is looked at: an unscoped refresh would run every
        // harness's CLI of the machine, which this run reports nothing of.
        let answer = admin.post(
            "/api/harnesses/check",
            &json!({ "id": harness.name(), "refresh": true }),
        )?;
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

/// The folder Pi's own installer keeps its launcher in, in the user's home: where a copy is made
/// from, never a place anything runs from.
fn managed_pi() -> Option<PathBuf> {
    let home = own_var("USERPROFILE").or_else(|| own_var("HOME"))?;
    let folder = Path::new(&home).join(".pi").join("agent").join("bin");
    let launcher = folder.join(if cfg!(windows) { "pi.cmd" } else { "pi" });
    launcher.is_file().then_some(folder)
}

/// How much a managed Pi's launcher folder may be: a launcher and what it starts, not a tree.
const COPY_LIMIT: u64 = 256 * 1024 * 1024;

/// Copies the files of the managed installation `from` into `to`, a folder of the run's own, so
/// that the update this run asks for lands in a copy and never in the owner's installation.
/// Refused, with nothing run, where a link is among them (it would lead out of the copy), where
/// it is too large to be a launcher, or where a file names the owner's folder (a launcher that
/// starts the program by its absolute path would run the owner's installation, not the copy).
fn copy_managed_pi(from: &Path, to: &Path) -> Result {
    let owner = from.to_string_lossy().into_owned();
    let owner_home = from
        .ancestors()
        .nth(3)
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut total = 0u64;
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((source, target)) = pending.pop() {
        cf_e2e::door::make_dir(&target)?;
        let entries = std::fs::read_dir(&source).map_err(|why| {
            Error::Daemon(format!("could not read {}: {why}", source.display()))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().map_err(|why| {
                Error::Daemon(format!("could not read {}: {why}", path.display()))
            })?;
            if kind.is_symlink() {
                return Err(Error::Daemon(format!(
                    "{} is a link: a copy of the managed Pi would lead out of the run's own folder; the update is not run",
                    path.display()
                )));
            }
            let to_path = target.join(entry.file_name());
            if kind.is_dir() {
                pending.push((path, to_path));
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|why| {
                Error::Daemon(format!("could not read {}: {why}", path.display()))
            })?;
            total += bytes.len() as u64;
            if total > COPY_LIMIT {
                return Err(Error::Daemon(format!(
                    "{} is more than a launcher: the update is not run",
                    from.display()
                )));
            }
            let text = String::from_utf8_lossy(&bytes);
            let names_owner = text.contains(&owner)
                || (!owner_home.is_empty() && text.contains(&owner_home));
            if names_owner {
                return Err(Error::Daemon(format!(
                    "{} names the owner's own folder: a copy of it would run the owner's installation, so the update is not run",
                    path.display()
                )));
            }
            // What the owner's file can run, the copy can run (a launcher is its mode).
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(&path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
            };
            #[cfg(not(unix))]
            let executable = false;
            if executable {
                cf_e2e::door::write_executable(&to_path, &bytes)?;
            } else {
                cf_e2e::door::write(&to_path, &bytes)?;
            }
        }
    }
    Ok(())
}

/// The update of a Pi its own installer made: it must come to `updated` or `unchanged`. Run on a
/// copy of that installation made in the kit's own home ([`copy_managed_pi`]), by the daemon of that
/// home and nothing of the owner's: the owner's launcher is not started, not even to read a version.
fn update_pi(log: &Log) -> Result<Vec<Check>> {
    let Some(source) = managed_pi() else {
        return Ok(vec![Check::failed(
            "pi",
            "the update",
            "this machine has no Pi made by Pi's own installer (<home>/.pi/agent/bin)",
        )]);
    };
    let home = Home::new()?;
    let folder = home.root().join("home/.pi/agent/bin");
    copy_managed_pi(&source, &folder)?;
    let launcher = folder.join(if cfg!(windows) { "pi.cmd" } else { "pi" });
    let kit_home: Vec<(String, String)> = home
        .vars()
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_string_lossy().into_owned()))
        .collect();
    let admin = Admin::start_in(home, Some(&folder), UPDATES)?;
    let before = says(&launcher, &kit_home);
    log.say(format!(
        "pi (managed, a copy in the run's own home) is {} ({before})",
        launcher.display()
    ));
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
    let after = says(&launcher, &kit_home);
    let state = result["state"].as_str().unwrap_or("none").to_owned();
    let said_after = result["after"].as_str().unwrap_or_default();
    let held = first
        && matches!(state.as_str(), "updated" | "unchanged")
        && result["reason"].is_null()
        && !said_after.is_empty()
        && after.contains(said_after);
    let detail = format!(
        "the row's program was the run's copy of the managed one: {first}; state {state}, {} -> {}, the program \
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

#[cfg(test)]
mod tests {
    use super::*;

    /// K3 review T-1978, 1: the update runs on a copy of the managed installation in the kit's own
    /// home, and a copy that would lead out of it, or run the owner's installation, is refused.
    #[cfg(unix)]
    #[test]
    fn the_managed_pi_is_copied_whole_and_a_link_or_a_launcher_naming_the_owner_is_refused() {
        let root = cf_e2e::door::made("managed-pi-").unwrap();
        let owner_bin = root.path().join("owner/.pi/agent/bin");
        cf_e2e::door::make_dir(&owner_bin).unwrap();
        cf_e2e::door::write_executable(owner_bin.join("pi"), "#!/bin/sh\nexec node \"$(dirname \"$0\")/pi.js\" \"$@\"\n").unwrap();
        cf_e2e::door::write(owner_bin.join("pi.js"), "console.log('1.0.0')").unwrap();
        let to = root.path().join("kit/home/.pi/agent/bin");
        copy_managed_pi(&owner_bin, &to).unwrap();
        assert!(to.join("pi.js").is_file());
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(to.join("pi")).unwrap().permissions().mode() & 0o111,
            0,
            "the launcher lost its mode"
        );
        // A launcher that starts the owner's program by its absolute path is not a copy.
        let naming = root.path().join("owner2/.pi/agent/bin");
        cf_e2e::door::make_dir(&naming).unwrap();
        cf_e2e::door::write_executable(
            naming.join("pi"),
            format!("#!/bin/sh\nexec {}/pi.js\n", naming.display()),
        )
        .unwrap();
        let refused = copy_managed_pi(&naming, &root.path().join("kit2/pi")).unwrap_err().to_string();
        assert!(refused.contains("names the owner's own folder"), "{refused}");
        // A link in it leads out.
        let linked = root.path().join("owner3/.pi/agent/bin");
        cf_e2e::door::make_dir(&linked).unwrap();
        cf_e2e::door::symlink(&owner_bin, linked.join("out")).unwrap();
        let refused = copy_managed_pi(&linked, &root.path().join("kit3/pi")).unwrap_err().to_string();
        assert!(refused.contains("is a link"), "{refused}");
    }

    /// The version a managed Pi says is read with the machine's variables and the kit's home, and
    /// nothing of the caller's environment.
    #[cfg(unix)]
    #[test]
    fn a_pi_launcher_is_asked_its_version_on_the_kits_home_and_not_the_callers_environment() {
        let root = cf_e2e::door::made("says-pi-").unwrap();
        let launcher = root.path().join("pi");
        cf_e2e::door::write_executable(
            &launcher,
            "#!/bin/sh\nif [ -n \"$CODEX_HOME$XDG_CONFIG_HOME$PI_CODING_AGENT_DIR\" ]; then echo caller; else echo \"v$HOME\"; fi\n",
        )
        .unwrap();
        let said = says(&launcher, &[("HOME".to_owned(), "/kit/home".to_owned())]);
        assert_eq!(said, "v/kit/home");
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
        let child_vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = vars
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
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
