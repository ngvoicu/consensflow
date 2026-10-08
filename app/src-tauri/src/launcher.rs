//! The terminal's command, repaired at the app's start (`cf_launcher::repair`),
//! in the flip release and in the deletion release alike: a user may skip the
//! flip release (a disk image, an installer), so the deletion release's first
//! start repairs what the flip release's would have.
//!
//! The command an older `cf setup` wrote names the bundled Node and `cf.mjs`,
//! which the bundle after the deletion no longer holds; the repair rewrites it
//! to name this bundle's `cf`, keeping the home it pins. It is meant to be a
//! thing the app does quietly, and so:
//!
//! - it repairs only the commands that serve this app's home, the one they pin or,
//!   with no pin, the default one: the owner runs the Candidate (its own home)
//!   and the installed app on one machine, and a terminal's `cf` would flip to
//!   whichever app started last if each rewrote every command it found;
//! - it makes no command: a machine that never ran `cf setup` has none and has
//!   none after, and a command that is not ours is left as it is;
//! - it names a plain `cf` that is there: Tauri may answer its folders in
//!   Windows' verbatim spelling, which cmd.exe starts nothing through, and a
//!   `cf` that is not there is no command to point a launcher at;
//! - what it could not do is one line of the app's error log, and it never stops
//!   the app: it runs on a thread of its own, after nothing, and its failure is
//!   that line.

use std::path::Path;
use std::thread;

use cf_base::env::Env;
use cf_launcher::{repair, Places, Repair, Repaired};
use tauri::AppHandle;

use crate::daemon_command::{bundled_cf, plain_path};

/// Repairs the terminal's command from a thread of its own, so that the app's
/// window opens meanwhile: on a portable Windows app the bundle may have to be
/// unpacked first. What it says goes to the error log.
pub(crate) fn repair_in_background(app: &AppHandle) {
    let app = app.clone();
    let spawned = thread::Builder::new()
        .name("consensflow-launcher-repair".to_owned())
        .spawn(move || {
            let cf = match bundled_cf(&app) {
                Ok(cf) => cf,
                Err(cause) => {
                    eprintln!("consensflow: the terminal command is not repaired: {cause}");
                    return;
                }
            };
            for line in repair_with(&Env::from_process(), &cf) {
                eprintln!("consensflow: {line}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("consensflow: the terminal command is not repaired: {error}");
    }
}

/// Repairs the commands of the home `env` names to run the bundle's `cf`, and
/// says what became of the ones that were not left alone, one line each: a
/// command now running `cf`, one that could not be repaired, one left as it is
/// for a reason the owner may want to know.
pub(crate) fn repair_with(env: &Env, cf: &Path) -> Vec<String> {
    let cf = plain_path(cf.to_path_buf());
    if !cf.is_absolute() || !cf.exists() {
        return vec![format!(
            "the terminal command is not repaired: the bundled cf is missing ({cf:?})"
        )];
    }
    said(&repair(env, &cf, &Places::default()), &cf)
}

/// What a repair says: a line for each command that was written, failed or put off.
fn said(repaired: &[Repaired], cf: &Path) -> Vec<String> {
    repaired
        .iter()
        .filter_map(|each| {
            let path = each.path.display();
            match &each.outcome {
                Repair::Rewritten => Some(format!(
                    "the terminal command {path} now runs {}",
                    cf.display()
                )),
                Repair::Failed(why) => Some(format!(
                    "the terminal command {path} could not be repaired: {why}"
                )),
                Repair::Transient => Some(format!(
                    "the terminal command {path} is left as it is: this app runs from a copy \
                     macOS takes away when it ends, and the installed app repairs it at its start"
                )),
                Repair::Absent | Repair::Unmarked | Repair::Elsewhere | Repair::Current => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
