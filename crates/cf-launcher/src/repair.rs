//! Making every launcher of ours run this bundle: the repair the app runs at
//! its start, in the flip release and in the deletion release.
//!
//! A launcher names absolute paths. The one an older build wrote names its
//! bundled Node and `cf.mjs`, which the bundle after the deletion no longer
//! holds, and the one this build writes names a `cf` that a portable
//! runtime's next version, or an app moved to another folder, leaves behind.
//! A user may also skip the flip release altogether, with a disk image or an
//! installer, so the deletion release repairs what the flip release would
//! have.
//!
//! What it does is small and keeps every promise it makes:
//!
//! - a command of ours (it holds the mark) that serves this app's home and
//!   does not name this bundle's `cf` is rewritten in the new shape, in
//!   place, with the home it pinned and with no other;
//! - on Windows a command is a launch file and the shim beside it
//!   ([`crate::program`]), and one that is not whole is made whole: the `.cmd`
//!   of a build before the shim, which cmd.exe cuts a many-line argument of,
//!   becomes a launch file and a shim and is taken away, and a shim that is not
//!   the bundle's is replaced;
//! - a command serves the home it pins, or, where it pins none, the default
//!   one (`~/.consensflow`): one that serves another home is left as it is,
//!   for the app of that home to repair. A Candidate and the app that is
//!   installed run on one machine, each with its own home, and a terminal's
//!   `cf` would be whichever app started last, running one app's binary
//!   against the other's home, if each rewrote every command it found;
//! - a command that is not ours is left as it is, and so is one that is
//!   already this bundle's;
//! - none is made where there was none, and no folder either: a machine
//!   that never ran `cf setup` has no terminal command, and has none after;
//! - none is pointed at a place that goes when the app ends: an app opened
//!   from Downloads runs from a copy macOS makes for the run, and leaves a
//!   command that works as it was ([`Repair::Transient`]);
//! - it can run again and again with the same result: the second run finds
//!   every command of ours naming this `cf` and writes nothing.
//!
//! Kept from Node on purpose: the command is written in place, as `cf setup`
//! writes it, through a link and into the file a running shell may hold, and
//! not beside itself and moved over. So a start killed within the few hundred
//! bytes of the write leaves a file that holds no mark, which is no command
//! of ours, and which no later repair finds.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::home::{config_root, default_root};
use cf_base::path;

use crate::install::{read_text, write_launcher};
use crate::places::{names, program_of, script_of, Places};
use crate::program;
use crate::text::{is_ours, launcher, pinned_home, runs, spelled, Runs};

/// What the repair did with one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repair {
    /// There was none, and none is made.
    Absent,
    /// Someone else's, left as it is.
    Unmarked,
    /// Ours, but it serves another home than the one the app that repairs runs
    /// on (it pins that one, or pins none and this app's is not the default
    /// one): left as it is, byte for byte, for the app of that home.
    Elsewhere,
    /// Ours, and it already runs this bundle's `cf`: left as it is.
    Current,
    /// Ours, and it ran something else: now it runs this bundle's `cf`.
    Rewritten,
    /// Ours, and it runs something else, but this `cf` is in a place macOS
    /// made for an app opened from where it was downloaded and takes away
    /// when the app ends: a command that named it would name nothing by then.
    /// Left as it is, for the app that is installed to repair at its start.
    Transient,
    /// What could not be read or written, as Node's error would say it.
    Failed(String),
}

/// One command looked at: where, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaired {
    pub path: PathBuf,
    pub outcome: Repair,
}

/// Makes every launcher of ours in `places` that serves the home `env` names
/// run `cf`, the native one of this bundle, and says what became of each name
/// in each folder, in order. The home each pinned is the home it keeps: the
/// repair never changes which home a terminal command talks to, only what runs
/// it, and never touches one that talks to another home than the app's.
///
/// `cf` is the program of the bundle that is running, which is there: a
/// command made to name one that is not is a command that is not there, which
/// [`Wiring::report`](crate::Wiring::report) then says. Where there is no
/// home to look in, there is nothing to repair.
pub fn repair(env: &Env, cf: &Path, places: &Places) -> Vec<Repaired> {
    let Ok(folders) = places.folders(env) else {
        return Vec::new();
    };
    let windows = env.on_windows();
    let cf = spelled(cf, windows);
    // The program of a Windows launcher, read once for every name in every folder.
    let shim = windows.then(|| program::read_bundled(Path::new(&cf)));
    let mut looked_at = Vec::new();
    for dir in &folders {
        for name in names(windows) {
            let file = PathBuf::from(path::join(&[&dir.to_string_lossy(), name]));
            let outcome = repaired(&file, windows, &cf, shim.as_ref(), env);
            looked_at.push(Repaired {
                path: file,
                outcome,
            });
        }
    }
    looked_at
}

/// Whether `cf` is where macOS runs an app it has not yet been told is safe,
/// one opened from Downloads: a read-only copy under `AppTranslocation`, with
/// a name of its own for each run, which is gone when the app ends. Written as
/// the command, it would be a command that works until the app is closed.
fn is_translocated(cf: &str) -> bool {
    cf.contains("/AppTranslocation/")
}

/// Whether the command whose text is `text` talks to the home `env` names:
/// the one it pins, which is the text `install` wrote (the home as the
/// environment that installed it spelled it, and as the one that repairs spells
/// the same), or, where it pins none, the default one, which an ordinary
/// terminal's `cf` falls back to. A pin is a path, so on Windows, whose paths
/// are not case-sensitive, it is compared without regard to case.
fn serves(text: &str, windows: bool, env: &Env) -> bool {
    let Some(home) = config_root(env) else {
        return false;
    };
    let home = home.to_string_lossy();
    match pinned_home(text, windows) {
        Some(pin) if windows => pin.eq_ignore_ascii_case(&home),
        Some(pin) => pin == home,
        None => default_root(env).is_some_and(|default| default.to_string_lossy() == home),
    }
}

/// What becomes of the command at `file`: on Windows its launch file, whose
/// `.cmd` of a build before the shim is the command until it is repaired.
fn repaired(
    file: &Path,
    windows: bool,
    cf: &str,
    shim: Option<&Result<Vec<u8>, String>>,
    env: &Env,
) -> Repair {
    let script = script_of(file);
    let held = if file.exists() {
        file
    } else if windows && script.exists() {
        &script
    } else {
        return Repair::Absent;
    };
    let text = match read_text(held) {
        Ok(text) => text,
        Err(failed) => return Repair::Failed(failed),
    };
    if !is_ours(&text) {
        return Repair::Unmarked;
    }
    if !serves(&text, windows, env) {
        return Repair::Elsewhere;
    }
    if matches!(runs(&text, windows), Some(Runs::Native { cf: named }) if named == cf)
        && shim.is_none_or(|shim| is_whole(file, held, shim))
    {
        return Repair::Current;
    }
    if is_translocated(cf) {
        return Repair::Transient;
    }
    // Before anything is written: a `.cmd` that runs is not given up for a launch
    // file with no program to read it.
    if let Some(Err(why)) = shim {
        if !program_of(file).is_file() {
            return Repair::Failed(why.clone());
        }
    }
    let script_text = launcher(windows, cf, pinned_home(&text, windows).as_deref());
    if let Err(failed) = write_launcher(file, &script_text, windows) {
        return Repair::Failed(failed.to_string());
    }
    match shim.map(|shim| made_whole(file, shim)) {
        Some(Err(failed)) => Repair::Failed(failed),
        Some(Ok(())) | None => Repair::Rewritten,
    }
}

/// Whether the Windows launcher at the launch file `file` is whole, `held`
/// being the file its text was read from: the launch file itself, not the
/// `.cmd` of a build before the shim; the program the shim of the bundle (or,
/// where the bundle has none to compare, one that is there); and no `.cmd` of
/// ours left beside them for cmd.exe to run.
fn is_whole(file: &Path, held: &Path, shim: &Result<Vec<u8>, String>) -> bool {
    let program = program_of(file);
    held == file
        && match shim {
            Ok(bytes) => program::is_current(&program, bytes),
            Err(_) => program.is_file(),
        }
        && !is_leftover(&script_of(file))
}

/// Whether the `.cmd` at `script` is one of ours, which the shim replaces.
fn is_leftover(script: &Path) -> bool {
    script.exists() && read_text(script).is_ok_and(|text| is_ours(&text))
}

/// Makes the program of the Windows launcher at `file` the shim, where the
/// bundle has one, and takes away the `.cmd` of ours that it replaces.
fn made_whole(file: &Path, shim: &Result<Vec<u8>, String>) -> Result<(), String> {
    if let Ok(bytes) = shim {
        program::place(&program_of(file), bytes).map_err(|failed| failed.to_string())?;
    }
    let script = script_of(file);
    if is_leftover(&script) {
        std::fs::remove_file(&script)
            .map_err(|failed| format!("{}: {failed}", script.display()))?;
    }
    Ok(())
}
