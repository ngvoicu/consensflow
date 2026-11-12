//! `cargo xtask live sweep [--in <folder>] [--older-than <days>] [--yes]`: the rig folders that
//! older runs left in `folder` (the temp folder unless named), each older than `days` (one
//! unless given), listed. With `--yes` each is taken through the kit's door
//! ([`cf_e2e::door::Door::remove_stale_rig`]): one a person kept, and one a process uses or
//! names, as the machine says at that moment, is left and the reason said. A rig's own start
//! takes none of them (T-1557, finding 3a), and nothing else does.

// A command a person runs answers on its own output.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cf_e2e::door::Door;
use cf_e2e::rig::sweep;

/// What the command line asks.
struct Asked {
    folder: PathBuf,
    older: Duration,
    yes: bool,
}

fn asked(args: &[String]) -> Result<Asked, String> {
    let mut asked = Asked {
        folder: std::env::temp_dir(),
        older: sweep::OLDER,
        yes: false,
    };
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--yes" => asked.yes = true,
            "--in" => {
                asked.folder = PathBuf::from(words.next().ok_or("--in takes a folder")?);
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
                    "{other} is no word of sweep: [--in <folder>] [--older-than <days>] [--yes]"
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
    let now = SystemTime::now();
    let found = sweep::older(&asked.folder, now, asked.older);
    println!(
        "{} rig folder{} older than {:.1} days in {}",
        found.len(),
        if found.len() == 1 { "" } else { "s" },
        asked.older.as_secs_f64() / 86_400.0,
        asked.folder.display()
    );
    for folder in &found {
        if asked.yes {
            println!(
                "{}",
                Door::of_run().remove_stale_rig(folder, asked.older, now)
            );
        } else {
            println!("  {}", folder.display());
        }
    }
    if !asked.yes && !found.is_empty() {
        println!("nothing was taken: --yes takes the ones nobody kept or uses");
    }
    ExitCode::SUCCESS
}
