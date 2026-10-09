//! `cargo xtask portable pack` and `portable inspect` (landing S2): the portable
//! Windows exe, whose layout is `cf-portable`'s alone.
//!
//! - `pack` writes it from the built app and the release folder around it (the
//!   runtime the daemon and the terminals need), and says where.
//! - `inspect` says what a file carries: the size of its payload, the CRC that
//!   names the folder the app unpacks the runtime into, and, given the version,
//!   that folder. The workflow that starts the exe once reads the folder here,
//!   and does no arithmetic on the bytes of the footer.
//!
//! A relative path is from the checkout's root, where the script this replaces
//! ran (`cargo xtask` runs the same from any folder). An option takes its value
//! as `--out DIR` or `--out=DIR`, once.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use cf_portable::{inspect, pack};
use cf_release::version::source_version;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};

/// Where `npm --prefix app run build` leaves the app, and the resources beside
/// it, from the checkout's root: the workspace's one build folder
/// (`.cargo/config.toml`).
const RELEASE: &str = "app/src-tauri/target/release";
const PACK_USAGE: &str = "[--release DIR] [--out DIR] [--version X]";
const INSPECT_USAGE: &str = "EXE [--version X]";

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["portable", "pack"],
        about: "Pack the portable Windows exe from the built app, its runtime and a footer that finds it",
        usage: PACK_USAGE,
        run: Run::Native(run_pack),
    },
    Command {
        words: &["portable", "inspect"],
        about: "Say what a portable exe carries: its payload, its CRC and, given the version, its runtime folder",
        usage: INSPECT_USAGE,
        run: Run::Native(run_inspect),
    },
];

/// `portable pack`: the exe at `<out>/ConsensFlow_<version>_x64-portable.exe`,
/// from the app and the runtime in `<release>`. The defaults are the build's own
/// release folder, `bundle/portable` in the release folder given or defaulted,
/// and the version the sources agree on. It says `portable: <path>`.
fn run_pack(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let given = Given::read(
        "pack",
        PACK_USAGE,
        &["--release", "--out", "--version"],
        args,
    )?;
    if !given.words.is_empty() {
        return Err(refused("pack", PACK_USAGE, &given.words));
    }
    let release = given
        .path("--release", context)
        .unwrap_or_else(|| context.path(RELEASE));
    let out = given
        .path("--out", context)
        .unwrap_or_else(|| release.join("bundle").join("portable"));
    let version = match given.version("pack")? {
        Some(version) => version,
        None => source_version(&context.root)?,
    };
    let exe = out.join(format!("ConsensFlow_{version}_x64-portable.exe"));
    let packed = pack(&release.join("ConsensFlow.exe"), &release, &exe)?;
    writeln!(console.out, "portable: {}", packed.path.display())?;
    Ok(0)
}

/// `portable inspect`: `payload: <length> bytes` and `crc: <8 hex digits>`, and
/// with `--version` the `runtime: <version>-<crc>` folder the app unpacks into.
/// A file that carries no footer is an error that names it, with the status 1.
fn run_inspect(
    context: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    let given = Given::read("inspect", INSPECT_USAGE, &["--version"], args)?;
    let [exe] = given.words.as_slice() else {
        return Err(refused("inspect", INSPECT_USAGE, &given.words));
    };
    let version = given.version("inspect")?;
    let payload = inspect(&context.root.join(exe))?;
    writeln!(console.out, "payload: {} bytes", payload.length)?;
    writeln!(console.out, "crc: {}", payload.crc_hex())?;
    if let Some(version) = version {
        writeln!(console.out, "runtime: {}", payload.folder(&version))?;
    }
    Ok(0)
}

/// What a command line gave: the options that take a value, by name, and the
/// words that are no option's.
struct Given {
    options: BTreeMap<&'static str, OsString>,
    words: Vec<OsString>,
}

impl Given {
    /// Reads `args` for `command`, which takes the options `takes` (`usage` is
    /// what its help says). One that is not among them, one without a value or
    /// with an empty one, and one given twice, are refused.
    fn read(
        command: &str,
        usage: &str,
        takes: &[&'static str],
        args: &[OsString],
    ) -> Result<Self, Failure> {
        let mut options = BTreeMap::new();
        let mut words = Vec::new();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let Some(text) = arg.to_str().filter(|text| text.starts_with("--")) else {
                words.push(arg.clone());
                continue;
            };
            let (flag, joined) = match text.split_once('=') {
                Some((flag, value)) => (flag, Some(value)),
                None => (text, None),
            };
            let Some(&name) = takes.iter().find(|name| **name == flag) else {
                return Err(refused(command, usage, std::slice::from_ref(arg)));
            };
            let value = match joined {
                Some(value) => OsString::from(value),
                None => rest
                    .next()
                    .filter(|word| !word.to_string_lossy().starts_with('-'))
                    .cloned()
                    .unwrap_or_default(),
            };
            if value.is_empty() {
                return Err(Failure::Usage(format!(
                    "portable {command}: {name} needs a value"
                )));
            }
            if options.insert(name, value).is_some() {
                return Err(Failure::Usage(format!(
                    "portable {command}: {name} is given twice"
                )));
            }
        }
        Ok(Self { options, words })
    }

    /// The path an option names: from the checkout's root, or as it is when it
    /// is absolute.
    fn path(&self, name: &str, context: &Context) -> Option<PathBuf> {
        self.options.get(name).map(|value| context.root.join(value))
    }

    /// The version `--version` names, as text.
    fn version(&self, command: &str) -> Result<Option<String>, Failure> {
        self.options
            .get("--version")
            .map(|value| {
                value.to_str().map(str::to_string).ok_or_else(|| {
                    Failure::Usage(format!("portable {command}: --version is not text"))
                })
            })
            .transpose()
    }
}

/// A command line that is not one the command takes: what it takes, and what it
/// was given instead.
fn refused(command: &str, usage: &str, given: &[OsString]) -> Failure {
    let takes = format!("portable {command} takes {usage}");
    if given.is_empty() {
        return Failure::Usage(takes);
    }
    let given: Vec<_> = given.iter().map(|word| word.to_string_lossy()).collect();
    Failure::Usage(format!("{takes}, not {}", given.join(" ")))
}

#[cfg(test)]
mod tests;
