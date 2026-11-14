//! `cargo xtask live sweep [--in <folder>] [--home <folder>] [--older-than <days>]`: the folders
//! that earlier runs left, each listed with what it is and why a person might keep it. The rig
//! folders, login copies and harness homes of runs are looked for in `folder` (the temp folder
//! unless named), and the project folders of live cases in the live folder of `home`
//! (`.consensflow-candidate/live` in the person's home unless named), each last changed at
//! least `days` ago (any age unless given). It takes nothing: a folder that looks left may be
//! another run's at work, or its evidence kept to be looked at, and a person removes what they
//! choose (T-1557, T-1592).

// A command a person runs answers on its own output.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cf_e2e::live::machine::live_root;
use cf_e2e::process::own_var;
use cf_e2e::rig::sweep;

/// What the command line asks.
struct Asked {
    folder: PathBuf,
    home: PathBuf,
    older: Duration,
}

fn asked(args: &[String]) -> Result<Asked, String> {
    let home = own_var("HOME")
        .or_else(|| own_var("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or("this machine says no home")?;
    let mut asked = Asked {
        folder: std::env::temp_dir(),
        home,
        older: Duration::ZERO,
    };
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--in" => {
                asked.folder = PathBuf::from(words.next().ok_or("--in takes a folder")?);
            }
            "--home" => {
                asked.home = PathBuf::from(words.next().ok_or("--home takes a folder")?);
            }
            "--older-than" => {
                let days: f64 = words
                    .next()
                    .and_then(|days| days.parse().ok())
                    .filter(|days: &f64| days.is_finite() && *days >= 0.0)
                    .ok_or("--older-than takes a number of days")?;
                asked.older = Duration::from_secs_f64(days * 24.0 * 60.0 * 60.0);
            }
            other => {
                return Err(format!(
                    "{other} is no word of sweep, which lists and takes nothing: \
                     [--in <folder>] [--home <folder>] [--older-than <days>]"
                ))
            }
        }
    }
    Ok(asked)
}

// The age of a folder is read against the clock its files are stamped by.
#[allow(clippy::disallowed_methods)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let asked = match asked(&args) {
        Ok(asked) => asked,
        Err(wrong) => {
            eprintln!("{wrong}");
            return ExitCode::from(2);
        }
    };
    let live = live_root(&asked.home);
    let found = sweep::listing(&asked.folder, &live, SystemTime::now(), asked.older);
    println!(
        "{} folder{} earlier runs left, in {} and {}; nothing is taken, a person removes what they choose",
        found.len(),
        if found.len() == 1 { "" } else { "s" },
        asked.folder.display(),
        live.display()
    );
    for left in &found {
        println!("{}", left.says());
    }
    if cf_e2e::door::Door::audit_asked() {
        // Sweep lists and takes nothing in any mode, so a say-only switch changes nothing in it;
        // the exit is what a say-only run's is, never a pass.
        eprintln!(
            "{} is set: sweep lists and takes nothing in any mode, so there is nothing for it to say only; a say-only run is no pass",
            cf_e2e::door::AUDIT
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
