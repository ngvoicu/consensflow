//! Starting a rig: the home it runs on, the daemon (held to being the native one
//! by the start line in its log), the pane host, and the routers between them.

use std::io::BufReader;
use std::path::Path;
use std::process::ChildStdout;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::install::{environment, var, write_fake_install, write_roster};
use super::route::Shared;
use super::{Config, Rig};
use crate::daemon::Daemon;
use crate::daemon_log::{self, errors_with_cause, lines_of, StartLine};
use crate::process::{Run, Spawned};
use crate::wire::{self, First};
use crate::{cf, checkout, files, pattern, serial, Error, Result};

/// How long the daemon and the pane host have to say they are ready: they are
/// started for the first time on a cold machine, and Node, whose time this was,
/// was given none.
const READY: Duration = Duration::from_secs(60);

/// What the pane host says first of all.
const HOST_KIND: &str = "consensflow-bridge";

/// An address on loopback, with a port.
static URL: OnceLock<Regex> = OnceLock::new();

impl Rig {
    /// Starts a rig. Takes its turn first ([`serial`]); builds the `cf` and the
    /// pane host under test if they are not built; refuses a daemon whose start
    /// line says it is not the native one.
    pub fn start(config: Config) -> Result<Self> {
        let turn = serial::turn();
        // A home of its own, which goes if the rig never starts; or the one it is
        // to resume on.
        let (root, scratch) = match &config.existing_root {
            Some(root) => (root.clone(), None),
            None => {
                let scratch = tempfile::Builder::new()
                    .prefix("consensflow-integration-")
                    .tempdir()
                    .map_err(|source| Error::File {
                        action: "make a folder in",
                        path: std::env::temp_dir(),
                        source,
                    })?;
                (scratch.path().to_path_buf(), Some(scratch))
            }
        };
        let workspace = root.join("workspace");
        files::make_dir(&workspace)?;
        let fake_bin = write_fake_install(&root, &config.stand_in)?;
        let env = environment(&root, &fake_bin, &config);
        // A restart over the same home keeps its roster, as the app's does: the
        // agents a case wrote are still the ones its chief and staff run on.
        if config.existing_root.is_none() {
            write_roster(&root)?;
        }

        let (mut daemon, start_line) = start_daemon(&config, &env)?;
        let (mut host, host_output) = start_host(&env)?;
        let shared = route(&mut daemon, &mut host, host_output)?;

        // The rig owns its home from here on: it goes with it.
        if let Some(scratch) = scratch {
            let _ = scratch.keep();
        }
        Ok(Self {
            root,
            workspace,
            env,
            start_line,
            shared,
            daemon,
            host,
            keep_root: false,
            closed: false,
            _turn: turn,
        })
    }
}

/// The daemon under test, started and held to being the native one: its handle
/// line read, and its start line in its log, which says what runtime it is.
fn start_daemon(config: &Config, env: &[(String, String)]) -> Result<(Daemon, StartLine)> {
    let mut daemon = match &config.daemon {
        Some(program) => Daemon::run(program, [] as [&str; 0], env.iter().cloned())?,
        None => Daemon::run(
            cf::binary()?,
            ["ui", "--json", "--no-open"],
            env.iter().cloned(),
        )?,
    };
    let log = Path::new(var(env, "CONSENSFLOW_HOME")).join("daemon.log");
    let handle = daemon.handle(READY).map_err(|failed| {
        // A daemon that refuses its home says why on its standard error and in
        // its log.
        let logged = errors_with_cause(&lines_of(
            &files::read_string(&log).unwrap_or_default(),
            daemon.id(),
        ))
        .join(": ");
        if logged.is_empty() {
            failed
        } else {
            Error::Daemon(format!("{failed}: {logged}"))
        }
    })?;
    let start_line = daemon_log::assert_started(&files::read_string(&log)?, daemon.id())?;
    let on_loopback = handle["url"]
        .as_str()
        .is_some_and(|url| pattern::once(&URL, r"^http://127\.0\.0\.1:\d+/$").is_match(url));
    if !on_loopback {
        return Err(Error::Daemon(format!(
            "the daemon's handle holds no address on loopback: {handle}"
        )));
    }
    Ok((daemon, start_line))
}

/// The pane host, started: its handle line read, and what it goes on to say
/// kept for the routers.
fn start_host(env: &[(String, String)]) -> Result<(Spawned, BufReader<ChildStdout>)> {
    let mut host = Run::new(cf::pane_host()?)
        .vars(env.iter().map(|(name, value)| (name, value)))
        .finding_programs()
        .cwd(checkout::root())
        .spawn()?;
    let output = host
        .take_output()
        .ok_or_else(|| Error::Daemon("the pane host has no output".to_owned()))?;
    let (first, rest) = wire::first_line(output, READY);
    let handle: Value = match first {
        First::Line(line) => serde_json::from_str(&line).map_err(|source| {
            Error::Daemon(format!(
                "the pane host's first line is no handle ({source}): {line}"
            ))
        })?,
        First::Ended => {
            return Err(Error::Daemon(format!(
                "the pane host ended before its handle: {}",
                host.errors()
            )))
        }
        First::TimedOut => {
            return Err(Error::Daemon(format!(
                "the pane host said nothing in {} s: {}",
                READY.as_secs(),
                host.errors()
            )))
        }
    };
    if handle["kind"] != HOST_KIND {
        return Err(Error::Daemon(format!(
            "the pane host's handle is not the bridge's: {handle}"
        )));
    }
    let rest = rest.ok_or_else(|| Error::Daemon("the pane host's output was lost".to_owned()))?;
    Ok((host, rest))
}

/// Both have spoken; from here every frame one writes is read, kept and passed
/// to the other. The daemon's input ends when the pane host's output does.
fn route(
    daemon: &mut Daemon,
    host: &mut Spawned,
    host_output: BufReader<ChildStdout>,
) -> Result<Arc<Shared>> {
    let (Some(to_daemon), Some(to_host)) = (daemon.sink(), host.sink()) else {
        return Err(Error::Daemon(
            "a program of the rig has no input".to_owned(),
        ));
    };
    let shared = Arc::new(Shared::default());
    {
        let (shared, to_host) = (Arc::clone(&shared), to_host.clone());
        daemon.read_each(
            move |line| {
                shared.route_daemon(&line, &to_host);
                true
            },
            || {},
        );
    }
    let routed = Arc::clone(&shared);
    let ended = to_daemon.clone();
    wire::read_each(
        host_output,
        move |line| {
            routed.route_host(&line, &to_host, &to_daemon);
            true
        },
        // The pane host is gone: the daemon's end of it is too.
        move || ended.end(),
    );
    Ok(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_address_on_loopback_with_a_port_is_the_daemons() {
        let url = pattern::once(&URL, r"^http://127\.0\.0\.1:\d+/$");
        assert!(url.is_match("http://127.0.0.1:51234/"));
        for other in [
            "http://127.0.0.1/",
            "http://localhost:51234/",
            "https://127.0.0.1:51234/",
            "http://127.0.0.1:51234",
        ] {
            assert!(!url.is_match(other), "{other}");
        }
    }
}
