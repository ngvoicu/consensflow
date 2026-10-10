//! The options of `cargo xtask smoke-updater`, as `node:util`'s `parseArgs` read
//! them in the script this replaces: `--name value` or `--name=value` for the
//! ones that take a value, `--name` for the others; one that is not among them, a
//! word that is no option's, and a value that is missing are refused.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::build::Release;

/// What the options of the smoke say, as given: paths are as they were written.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The installed releases to run, comma apart.
    pub from: Option<String>,
    pub from_app: Option<PathBuf>,
    pub from_release: Option<String>,
    pub to_app: Option<PathBuf>,
    /// A checkout of the bridge to build from.
    pub bridge: Option<PathBuf>,
    /// A checkout of the flip release to build from.
    pub flip: Option<PathBuf>,
    pub flip_ref: Option<String>,
    pub cache: Option<PathBuf>,
    pub build_only: bool,
    pub reuse: bool,
    pub export_bridge: bool,
    /// The words the names of the cases asked for hold.
    pub only: Vec<String>,
    pub machines: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub keep: bool,
}

/// What the smoke is given, for the help of the command.
pub const USAGE: &str = "[--from RELEASES] [--only WORDS] [--reuse] [--build-only] [--keep] \
    [--from-app APP --from-release NAME --to-app APP] [--bridge DIR] [--flip DIR] \
    [--flip-ref REF] [--cache DIR] [--export-bridge] [--machines DIR] [--timeout MS]";

/// The options that take a value, and the flags.
const VALUES: [&str; 11] = [
    "--from",
    "--from-app",
    "--from-release",
    "--to-app",
    "--bridge",
    "--flip",
    "--flip-ref",
    "--cache",
    "--only",
    "--machines",
    "--timeout",
];
const FLAGS: [&str; 4] = ["--build-only", "--reuse", "--export-bridge", "--keep"];

fn refused(said: impl Into<String>) -> String {
    said.into()
}

impl Options {
    /// Reads the command line. A refusal is what is wrong with it, in words.
    pub fn read(args: &[OsString]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let text = arg.to_string_lossy();
            let (name, joined) = match text.split_once('=') {
                Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
                _ => (text.as_ref(), None),
            };
            if FLAGS.contains(&name) {
                if joined.is_some() {
                    return Err(refused(format!("{name} takes no value")));
                }
                options.set_flag(name);
            } else if VALUES.contains(&name) {
                let value = match joined {
                    Some(value) => value,
                    // A value may not look like an option: `--only --keep` is a value missing.
                    None => match rest.next() {
                        Some(next) if !next.to_string_lossy().starts_with('-') => {
                            next.to_string_lossy().into_owned()
                        }
                        _ => {
                            return Err(refused(format!(
                                "{name} takes a value (to start one with a dash: {name}=-value)"
                            )))
                        }
                    },
                };
                options.set(name, &value)?;
            } else if name.starts_with('-') {
                return Err(refused(format!("unknown option: {name}")));
            } else {
                return Err(refused(format!("unexpected argument: {text}")));
            }
        }
        Ok(options)
    }

    fn set_flag(&mut self, name: &str) {
        match name {
            "--build-only" => self.build_only = true,
            "--reuse" => self.reuse = true,
            "--export-bridge" => self.export_bridge = true,
            _ => self.keep = true,
        }
    }

    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        let path = || Some(PathBuf::from(value));
        let text = || Some(value.to_string());
        match name {
            "--from" => self.from = text(),
            "--from-app" => self.from_app = path(),
            "--from-release" => self.from_release = text(),
            "--to-app" => self.to_app = path(),
            "--bridge" => self.bridge = path(),
            "--flip" => self.flip = path(),
            "--flip-ref" => self.flip_ref = text(),
            "--cache" => self.cache = path(),
            "--only" => {
                self.only = value
                    .split(',')
                    .filter(|word| !word.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "--machines" => self.machines = path(),
            _ => {
                let milliseconds: u64 = value.parse().map_err(|_| {
                    refused(format!(
                        "--timeout is a number of milliseconds, not {value}"
                    ))
                })?;
                self.timeout = Some(Duration::from_millis(milliseconds));
            }
        }
        Ok(())
    }

    /// The installed releases this run is about, in the order they run.
    pub fn releases(&self) -> Result<Vec<Release>, String> {
        let names: Vec<&str> = if self.from_app.is_some() {
            if self.from.is_some() {
                return Err(refused(
                    "--from-app takes one release: name it with --from-release",
                ));
            }
            vec![self.from_release.as_deref().unwrap_or("flip")]
        } else if self.from_release.is_some() {
            return Err(refused("--from-release says which release --from-app is"));
        } else {
            match &self.from {
                None => vec!["flip", "bridge"],
                Some(from) => from.split(',').filter(|name| !name.is_empty()).collect(),
            }
        };
        names
            .into_iter()
            .map(|name| {
                Release::from_name(name).ok_or_else(|| {
                    let known: Vec<_> = Release::ALL.iter().map(|release| release.name()).collect();
                    refused(format!(
                        "{name} is no release to install from: {}",
                        known.join(" or ")
                    ))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
