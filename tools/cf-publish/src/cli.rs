//! The command line:
//!
//! ```text
//! cf-publish feeds plan --version <v> --archive <file> [--dry-run]
//!     prints the feeds this release moves, one per line, the new ones first
//! cf-publish feeds prerequisites --version <v> --base <url> [--dry-run]
//!     says whether the old feeds serve the bridge, which a later release needs
//! cf-publish feeds check --dir <dir> --version <v> --base <url>
//!     says whether, once published, the files and the feeds serve what the rule says
//! cf-publish publish --dir <dist> --tag v<version> --base <url> [--repo <owner>/<repo>]
//!     publishes the release built in <dist>, and moves its feeds
//! ```
//!
//! `--dry-run` is for a hand run of the workflow, which publishes nothing: what
//! a tag would be refused for is said, and is not a failure. `--attempts` and
//! `--wait` (milliseconds) say how long the reads wait for a feed that is not
//! right yet. `publish` works on `--repo`, else `GH_REPO`, else
//! `GITHUB_REPOSITORY`, and only for the push of the tag it is asked for
//! (`GITHUB_EVENT_NAME`, `GITHUB_REF_TYPE` and `GITHUB_REF_NAME`).
//!
//! Every failure ends the run with status 1 and one line on stderr, prefixed by
//! the command (`feeds: ` or `publish: `); a refusal of the rule is that too.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::assets::{members_of, missing_for_old_apps};
use crate::checks::{check_feeds, check_prerequisites};
use crate::failure::Failure;
use crate::gh::ProcessGh;
use crate::manifest::Manifest;
use crate::publish::{publish_release, Means, Publication};
use crate::read::Patience;
use crate::rule::{plan_feeds, role_of, Role};
use crate::version::Version;

/// What the workflow's run says about itself, read once by `main`: `publish`
/// publishes only for the push of the version tag it is asked for.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    /// `GITHUB_EVENT_NAME`.
    pub event_name: Option<String>,
    /// `GITHUB_REF_TYPE`.
    pub ref_type: Option<String>,
    /// `GITHUB_REF_NAME`.
    pub ref_name: Option<String>,
    /// `GH_REPO`, the repository `gh` works on.
    pub gh_repo: Option<String>,
    /// `GITHUB_REPOSITORY`.
    pub github_repository: Option<String>,
}

/// The words after a command's name, by option.
struct Options {
    values: BTreeMap<String, String>,
    flags: BTreeSet<String>,
}

impl Options {
    /// `args` as options: `--name value` or `--name=value` for the names in
    /// `takes`, `--name` for the names in `flags`; anything else is refused.
    fn parse(args: &[String], takes: &[&str], flags: &[&str]) -> Result<Self, Failure> {
        let mut options = Self {
            values: BTreeMap::new(),
            flags: BTreeSet::new(),
        };
        let mut words = args.iter();
        while let Some(word) = words.next() {
            let Some(option) = word.strip_prefix("--") else {
                return Err(Failure::new(if word.starts_with('-') && word.len() > 1 {
                    format!("Unknown option '{word}'")
                } else {
                    format!(
                        "Unexpected argument '{word}'. This command does not take positional arguments"
                    )
                }));
            };
            let (name, joined) = match option.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (option, None),
            };
            if takes.contains(&name) {
                let value = match joined {
                    Some(value) => value.to_string(),
                    None => words.next().cloned().ok_or_else(|| {
                        Failure::new(format!("Option '--{name} <value>' argument missing"))
                    })?,
                };
                options.values.insert(name.to_string(), value);
            } else if flags.contains(&name) && joined.is_none() {
                options.flags.insert(name.to_string());
            } else {
                return Err(Failure::new(format!("Unknown option '{word}'")));
            }
        }
        Ok(options)
    }

    /// The value of `name`, where one was given and is not empty.
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    /// How long reads wait: what `--attempts` and `--wait` say, else twelve
    /// asks five seconds apart.
    fn patience(&self) -> Result<Patience, Failure> {
        let number = |name: &str, otherwise: u64| -> Result<u64, Failure> {
            match self.values.get(name) {
                None => Ok(otherwise),
                Some(text) => text.parse().map_err(|_| {
                    Failure::new(format!("--{name} needs a whole number, not {text}"))
                }),
            }
        };
        let attempts = u32::try_from(number("attempts", 12)?).unwrap_or(u32::MAX);
        if attempts == 0 {
            return Err(Failure::new("--attempts needs at least 1"));
        }
        Ok(Patience::new(
            attempts,
            Duration::from_millis(number("wait", 5000)?),
        ))
    }
}

/// What a command found, said as it is by the one that runs it: refusals, or
/// for a hand run what a tag would meet. The status is 1 for a refusal, 0 for a
/// hand run's.
fn say(
    problems: &[String],
    dry: bool,
    ok: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    if problems.is_empty() {
        writeln!(out, "feeds: {ok}")?;
        return Ok(0);
    }
    for problem in problems {
        let note = if dry { " (a tag would be refused)" } else { "" };
        writeln!(err, "feeds{note}: {problem}")?;
    }
    Ok(u8::from(!dry))
}

/// The feeds a release moves, and the role it has, or why it may not.
fn planned(version: &str, archive: &str) -> Result<(Version, Role, Vec<String>), Failure> {
    let manifest = Manifest::embedded()?;
    let members = members_of(Path::new(archive))?;
    let version = Version::parse(version)?;
    let feeds = plan_feeds(&version, &missing_for_old_apps(&members), &manifest)?;
    let role = role_of(&version, &manifest)?;
    Ok((version, role, feeds))
}

fn plan(options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Failure> {
    let (Some(version), Some(archive)) = (options.get("version"), options.get("archive")) else {
        return Err(Failure::new("plan needs --version and --archive"));
    };
    let dry = options.flag("dry-run");
    match planned(version, archive) {
        Ok((version, role, feeds)) => {
            writeln!(
                err,
                "feeds: {version} is {}: {}",
                role.describe(),
                feeds.join(", ")
            )?;
            writeln!(out, "{}", feeds.join("\n"))?;
            Ok(0)
        }
        Err(refusal) if dry => say(&[refusal.to_string()], dry, "", out, err),
        Err(refusal) => Err(refusal),
    }
}

fn prerequisites(
    options: &Options,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    let (Some(version), Some(base)) = (options.get("version"), options.get("base")) else {
        return Err(Failure::new("prerequisites needs --version and --base"));
    };
    let manifest = Manifest::embedded()?;
    let patience = options.patience()?;
    let (problems, bridge) = match Version::parse(version) {
        Err(refusal) => (vec![refusal.to_string()], false),
        Ok(version) => {
            let problems = check_prerequisites(&version, base, &manifest, patience);
            let bridge = problems.is_empty()
                && role_of(&version, &manifest).is_ok_and(|role| role == Role::Bridge);
            (problems, bridge)
        }
    };
    let ok = format!(
        "{version} may move its feeds ({})",
        if bridge {
            "it is the bridge"
        } else {
            "the old feeds serve the bridge, with its files"
        }
    );
    say(&problems, options.flag("dry-run"), &ok, out, err)
}

fn check(options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Failure> {
    let (Some(dir), Some(version), Some(base)) = (
        options.get("dir"),
        options.get("version"),
        options.get("base"),
    ) else {
        return Err(Failure::new("check needs --dir, --version and --base"));
    };
    let manifest = Manifest::embedded()?;
    let patience = options.patience()?;
    let problems = match Version::parse(version) {
        Err(refusal) => vec![refusal.to_string()],
        Ok(version) => check_feeds(Path::new(dir), &version, base, &manifest, patience)?,
    };
    say(
        &problems,
        false,
        "serve this release as the rule says",
        out,
        err,
    )
}

/// `cf-publish feeds …`.
fn feeds(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Failure> {
    let (command, rest) = args.split_first().map_or((None, &[][..]), |(first, rest)| {
        (Some(first.as_str()), rest)
    });
    let takes = ["version", "archive", "dir", "base", "attempts", "wait"];
    let options = Options::parse(rest, &takes, &["dry-run"])?;
    match command {
        Some("plan") => plan(&options, out, err),
        Some("prerequisites") => prerequisites(&options, out, err),
        Some("check") => check(&options, out, err),
        _ => Err(Failure::new(
            "usage: cf-publish feeds plan|prerequisites|check",
        )),
    }
}

/// `cf-publish publish …`: the release built in `--dir`, for the push of the
/// version tag `--tag` and no other run.
fn publish(args: &[String], env: &Environment, out: &mut dyn Write) -> Result<u8, Failure> {
    let options = Options::parse(args, &["dir", "tag", "base", "repo"], &[])?;
    let repo = options
        .get("repo")
        .or(env.gh_repo.as_deref())
        .or(env.github_repository.as_deref())
        .filter(|repo| !repo.is_empty());
    let (Some(dir), Some(tag), Some(base), Some(repo)) = (
        options.get("dir"),
        options.get("tag"),
        options.get("base"),
        repo,
    ) else {
        return Err(Failure::new(
            "publish needs --dir, --tag and --base, and the repository (GH_REPO)",
        ));
    };
    if env.event_name.as_deref() != Some("push") || env.ref_type.as_deref() != Some("tag") {
        return Err(Failure::new(format!(
            "only the push of a version tag publishes; this is {} on {}",
            env.event_name.as_deref().unwrap_or("not a workflow"),
            env.ref_type.as_deref().unwrap_or("nothing"),
        )));
    }
    if env.ref_name.as_deref() != Some(tag) {
        return Err(Failure::new(format!(
            "this run is for {}, and it was asked to publish {tag}",
            env.ref_name.as_deref().unwrap_or("undefined"),
        )));
    }
    let manifest = Manifest::embedded()?;
    let out = RefCell::new(out);
    let log = |line: &str| {
        let _ = writeln!(out.borrow_mut(), "publish: {line}");
    };
    let publication = Publication {
        dir: Path::new(dir),
        tag,
        base,
        repo,
        manifest: &manifest,
        patience: Patience::default(),
    };
    let means = Means {
        gh: &ProcessGh,
        members: &members_of,
        log: &log,
    };
    let done = publish_release(&publication, &means)?;
    let moved: Vec<String> = done
        .feeds
        .iter()
        .map(|(feed, did)| format!("{feed} {did}"))
        .collect();
    writeln!(
        out.borrow_mut(),
        "publish: {}: the release was {}; {}",
        done.version,
        done.release,
        moved.join(", ")
    )?;
    Ok(0)
}

/// Runs the command line `args` (without the program's name): the status it
/// ends with, 0 when it finished and 1 when it could not or the rule refused.
pub fn run(args: &[String], env: &Environment, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let (who, result) = match args.first().map(String::as_str) {
        Some("feeds") => ("feeds", feeds(&args[1..], out, err)),
        Some("publish") => ("publish", publish(&args[1..], env, out)),
        _ => (
            "cf-publish",
            Err(Failure::new(
                "usage: cf-publish feeds plan|prerequisites|check, or publish",
            )),
        ),
    };
    match result {
        Ok(status) => status,
        Err(failure) => {
            let _ = writeln!(err, "{who}: {failure}");
            1
        }
    }
}
