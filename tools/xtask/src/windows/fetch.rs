//! What a run leaves in `evals/reports` on the machine, brought back here once the run has
//! ended: a long run's report and ledger (`evals/reports/long/`), an eval's report, a stop's. The
//! sweep of the next run there removes what is older than [`REPORTS_KEPT_DAYS`] from that
//! folder, so a report that lived only there would be gone in two weeks (T-141, P3-3). The
//! folder here is the main checkout's `evals/reports` (the parent of the repository's common
//! git folder), not that of the tree the run went from: a worker's tree goes with its task, and
//! what lived only in it would go too (T-194, P3-3). It is ignored by git, and the privacy line
//! holds: nothing of it goes anywhere but this machine. Where there is no repository to ask, it
//! is the checkout's own.
//!
//! The machine lists what it holds ([`reports_script`]), each file with its size, and only a
//! file this checkout does not have is copied (`scp`, one at a time, to a name of its own that is
//! made the file's when its size is the one listed): what is here is never replaced, so a
//! report brought back once is kept as it was. What the matrix writes under `windows/` is
//! written here and is not listed. A name that is none of what a report's is (a path out of the
//! folder, a character no report's name holds) is not copied, and said. None of it changes how
//! the run ended: what cannot be brought back is said on the error stream, and the run's status
//! is the command's own.
//!
//! [`REPORTS_KEPT_DAYS`]: super::script::REPORTS_KEPT_DAYS
//! [`reports_script`]: super::script::reports_script

use std::fs;
use std::path::{Path, PathBuf};

use super::interrupt::Interrupt;
use super::options::Options;
use super::run::{self, Outcome};
use super::script::reports_script;
use super::Error;
use crate::context::Context;
use crate::dispatch::Console;
use crate::process::{self, Invocation};

/// How many of the names brought back are said, before the rest is counted.
const SAID: usize = 4;

/// [`run::run`], and then, where the command ended there with a status, what it left in
/// `evals/reports` brought back here. A run that could not be sent brings back nothing, and a run
/// that was stopped here is not waited for any longer: it is ended there, and its half-written
/// report is the next run's to be pruned.
pub fn run_keeping(
    context: &Context,
    options: &Options,
    console: &mut Console,
    interrupt: &Interrupt,
) -> Result<Outcome, Error> {
    let ended = run::run(context, options, console, interrupt)?;
    if let Outcome::Exited(_) = ended {
        // What cannot be said is no more than what cannot be brought: the run's status stands.
        let _ = bring_back(context, options, console);
    }
    Ok(ended)
}

/// One file the machine lists: its name from `evals/reports`, with `/`, and its size.
#[derive(Debug, PartialEq, Eq)]
struct Listed {
    name: String,
    size: u64,
}

/// What `text`, the machine's listing, says: a file for each line of a name, a tab and a size. A
/// line that is no such thing says nothing (PowerShell may say other words), and a name that
/// is not a report's is none of them: its words are returned as what could not be taken.
fn parse(text: &str) -> (Vec<Listed>, Vec<String>) {
    let (mut files, mut refused) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let Some((name, size)) = line.trim_end().split_once('\t') else {
            continue;
        };
        let Ok(size) = size.trim().parse::<u64>() else {
            continue;
        };
        if name.starts_with("windows/") {
            continue;
        }
        if is_report_name(name) {
            files.push(Listed {
                name: name.to_owned(),
                size,
            });
        } else {
            refused.push(name.to_owned());
        }
    }
    (files, refused)
}

/// Whether `name` is a path inside the reports folder of the characters a report's name has: each
/// part is made of letters, digits, `.`, `_`, `+` and `-`, and is none of the folder's own (`.`
/// and `..`). Such a name is safe in `scp`'s words and in a path here.
fn is_report_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
        })
}

/// Lists what the machine has in `evals/reports` and brings back what is not here, saying what it
/// did on the error stream. Nothing is said where there was nothing to do.
fn bring_back(context: &Context, options: &Options, console: &mut Console) -> Result<(), Error> {
    let line = run::powershell(&reports_script(&options.dir))?;
    let listed = process::capture(&run::ssh(context, &options.host, &line), &context.env)?;
    if listed.code != 0 {
        writeln!(
            console.err,
            "windows: the reports on the machine could not be listed, and none was brought back: ssh ended with status {}",
            listed.code
        )
        .map_err(Error::Output)?;
        return Ok(());
    }
    let (files, refused) = parse(&listed.stdout);
    for name in &refused {
        writeln!(
            console.err,
            "windows: not brought back, for it is no name of a report's: {name:?}"
        )
        .map_err(Error::Output)?;
    }
    let (here, in_main) = reports_folder(context);
    let mut brought = Vec::new();
    for file in files {
        let to = here.join(file.name.replace('/', std::path::MAIN_SEPARATOR_STR));
        if to.exists() {
            continue;
        }
        match fetch(context, options, &file, &to) {
            Ok(()) => brought.push(file.name),
            Err(said) => writeln!(
                console.err,
                "windows: the report {} could not be brought back: {said}",
                file.name
            )
            .map_err(Error::Output)?,
        }
    }
    if let Some(told) = told(&brought, in_main) {
        writeln!(console.err, "windows: {told}").map_err(Error::Output)?;
    }
    Ok(())
}

/// Where the reports are kept here, and whether that is the main checkout's folder and not the
/// tree's own: the main checkout's `evals/reports` (the parent of the repository's common git
/// folder), for a run that goes from a tree linked to it goes with its task, and what lived only
/// in the tree would go too (T-194, P3-3). The checkout itself is where there is no repository
/// to ask, or its common folder is no `.git` (a bare repository's is not a checkout's).
fn reports_folder(context: &Context) -> (PathBuf, bool) {
    let own = context.root.join("evals").join("reports");
    let asked = Invocation::new("git", &context.root).args(["rev-parse", "--git-common-dir"]);
    let Ok(answered) = process::capture(&asked, &context.env) else {
        return (own, false);
    };
    let common = answered.stdout.trim();
    if answered.code != 0 || common.is_empty() {
        return (own, false);
    }
    let common = context.root.join(common);
    let main = (common.file_name().is_some_and(|name| name == ".git"))
        .then(|| common.parent().map(Path::to_path_buf))
        .flatten();
    match main {
        Some(main) if main.is_dir() => {
            let differs = fs::canonicalize(&main).ok() != fs::canonicalize(&context.root).ok();
            (main.join("evals").join("reports"), differs)
        }
        _ => (own, false),
    }
}

/// What is said of the files brought back: how many, and the first of them; none where none was.
fn told(brought: &[String], in_main: bool) -> Option<String> {
    if brought.is_empty() {
        return None;
    }
    let shown: Vec<&str> = brought.iter().take(SAID).map(String::as_str).collect();
    let more = brought.len().saturating_sub(SAID);
    Some(format!(
        "brought back {} of the machine's reports into {}evals/reports: {}{}",
        brought.len(),
        if in_main { "the main checkout's " } else { "" },
        shown.join(", "),
        if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        }
    ))
}

/// Copies `file` from the machine to `to`: to a name of its own first, which is `to` once the
/// size is the one listed (a copy that stopped half way is no report); why not, if it could not.
fn fetch(context: &Context, options: &Options, file: &Listed, to: &Path) -> Result<(), String> {
    let folder = to.parent().ok_or("it has no folder")?;
    fs::create_dir_all(folder).map_err(|cause| format!("{}: {cause}", folder.display()))?;
    let part = partial(to);
    let scp = Invocation::new("scp", &context.root)
        .arg("-q")
        .arg(format!(
            "{}:{}/evals/reports/{}",
            options.host, options.dir, file.name
        ))
        .arg(&part);
    let copied = process::capture(&scp, &context.env).map_err(|failure| failure.to_string())?;
    let result = if copied.code != 0 {
        Err(format!(
            "scp ended with status {}{}",
            copied.code,
            said(&copied.stderr)
        ))
    } else {
        match fs::metadata(&part) {
            Ok(found) if found.len() == file.size => {
                fs::rename(&part, to).map_err(|cause| format!("{}: {cause}", to.display()))
            }
            Ok(found) => Err(format!(
                "{} bytes came, and the machine listed {}",
                found.len(),
                file.size
            )),
            Err(cause) => Err(format!("{}: {cause}", part.display())),
        }
    };
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// The name a copy of `to` is made under until it is whole.
fn partial(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_owned();
    name.push(".part");
    to.with_file_name(name)
}

/// What a program said, as the end of a sentence: nothing where it said nothing.
fn said(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

#[cfg(test)]
mod tests;
