//! What earlier runs left, listed and never taken (T-1557, T-1592). A run's own folders go with the
//! run, but a run that was killed, or one asked to keep its folders (`CONSENSFLOW_LIVE_KEEP`),
//! leaves them, and a folder that looks left may be another run's at work or its evidence. So
//! nothing of the kit takes one: a rig's start counts the rig folders older than a day in the
//! folder it makes its own in ([`homes`]), a login copy's or a harness home's start counts the
//! copies of earlier runs and says so once ([`say_copies`]), and `cargo xtask live sweep`
//! ([`listing`]) says of every folder there is what it is, how old, and why a person might keep
//! it. A person removes what they choose.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use crate::door::{KEPT, RIG_PREFIX};

mod usage;

pub use usage::Table;

/// How old a home is before a rig's start counts it.
pub const OLDER: Duration = Duration::from_secs(24 * 60 * 60);

/// Says how many rig folders older than a day stand in `folder`, when any do.
// The time of day is read in one place of the product ([`cf_base::time`]), which this crate
// does not depend on: the age of a folder is read against the clock its files are stamped by.
#[allow(clippy::disallowed_methods, clippy::print_stderr)] // Said in the output of the run.
pub(super) fn homes(folder: &Path) -> usize {
    let count = older(folder, SystemTime::now(), OLDER).len();
    if count > 0 {
        eprintln!(
            "{count} older rig folder{} stand{} in {}; `cargo xtask live sweep` lists them and \
             what each is: a person removes the ones nobody needs",
            if count == 1 { "" } else { "s" },
            if count == 1 { "s" } else { "" },
            folder.display()
        );
    }
    count
}

/// The rig folders in `folder` (named as a rig names its folder, a folder itself and not a link
/// or a report) last changed more than `older` before `now`. Nothing of them is read or taken.
pub fn older(folder: &Path, now: SystemTime, older: Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(RIG_PREFIX) && !name.ends_with("-left.txt")
        })
        .filter_map(|entry| {
            // The entry itself, not what a link leads to: a link is none of the rig's.
            let found = std::fs::symlink_metadata(entry.path()).ok()?;
            let age = now.duration_since(found.modified().ok()?).ok()?;
            (found.is_dir() && age > older).then(|| entry.path())
        })
        .collect();
    found.sort();
    found
}

/// What a folder an earlier run left is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A rig's folder: its home, its board's files and the workspace of its windows.
    Rig,
    /// A copy of Claude Code's login a run made for its windows to run on.
    LoginCopy,
    /// A home a run made for the windows of a harness, with a copy of its login in it.
    HarnessHome,
    /// A project folder of a live case, below `~/.consensflow-candidate/live`.
    Project,
}

impl Kind {
    /// What a person is told of it.
    fn is(self) -> &'static str {
        match self {
            Kind::Rig => "the folder of a rig a run started: its home, its board and its windows' workspace",
            Kind::LoginCopy => "a copy of Claude Code's login, made for a run's windows: it holds a login",
            Kind::HarnessHome => "a home a run made for a harness's windows: it holds a copy of that harness's login",
            Kind::Project => "the project folder of a live case: the brief it was given and what the case wrote there",
        }
    }
}

/// What the machine says of the processes that use a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Used {
    /// None was asked: the machine's processes were not read.
    NotAsked,
    /// A process works in it or names it.
    Yes,
    /// No process does.
    No,
    /// The machine's processes could not be read, and why.
    Unread(String),
}

/// A folder an earlier run left, and what is known of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    pub path: PathBuf,
    pub kind: Kind,
    /// How long ago it last changed, where the machine says.
    pub age: Option<Duration>,
    /// What its mark [`KEPT`] says, where it has one.
    pub kept: Option<String>,
    /// The process a login copy or a harness home is named for, and whether it is there.
    pub run: Option<(u32, bool)>,
    pub used: Used,
}

impl Left {
    /// What a person is told of it: its path, what it is, and why they might keep it.
    pub fn says(&self) -> String {
        let age = self
            .age
            .map_or("of an age the machine does not say".to_owned(), |age| {
                format!("{:.1} days old", age.as_secs_f64() / 86_400.0)
            });
        let mut keep = Vec::new();
        match &self.kept {
            Some(why) => keep.push(format!(
                "it is marked kept (\"{}\"): a person or a run left it to be looked at",
                why.trim()
            )),
            None => keep.push("it is not marked kept".to_owned()),
        }
        match self.run {
            Some((pid, true)) => keep.push(format!(
                "its run (process {pid}) is there, and may be at work"
            )),
            Some((pid, false)) => keep.push(format!("its run (process {pid}) is gone")),
            None => {}
        }
        match &self.used {
            Used::NotAsked => {}
            Used::Yes => keep.push(
                "a process works in it or names it, so a run may be at work there".to_owned(),
            ),
            Used::No => keep.push("no process works in it or names it".to_owned()),
            Used::Unread(why) => keep.push(format!(
                "whether a process uses it could not be read ({why})"
            )),
        }
        if self.age.is_some_and(|age| age < OLDER) {
            keep.push("it changed less than a day ago".to_owned());
        }
        format!(
            "{}\n    {}; {age}\n    {}",
            self.path.display(),
            self.kind.is(),
            keep.join("; ")
        )
    }
}

/// The folders earlier runs left in the temp folder `temp` (rig folders, login copies, harness
/// homes) and in the live folder `live` (project folders), each last changed at least `older`
/// before `now`, with what the machine says of the processes that use them. Nothing is changed.
pub fn listing(temp: &Path, live: &Path, now: SystemTime, older: Duration) -> Vec<Left> {
    let mut found = in_temp(temp, now, older, true);
    found.extend(in_live(live, now, older));
    let table = Table::read();
    for left in &mut found {
        left.used = match &table {
            Ok(table) if table.uses(&left.path) => Used::Yes,
            Ok(_) => Used::No,
            Err(why) => Used::Unread(why.clone()),
        };
    }
    found
}

/// What names in `temp` are an earlier run's: by the name a rig, a login copy or a harness home
/// is given, a folder itself and not a link or a report.
fn in_temp(temp: &Path, now: SystemTime, older: Duration, with_rigs: bool) -> Vec<Left> {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return Vec::new();
    };
    let mut found: Vec<Left> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = if with_rigs && name.starts_with(RIG_PREFIX) && !name.ends_with("-left.txt")
            {
                Kind::Rig
            } else if name.starts_with(crate::live::login::PREFIX) {
                Kind::LoginCopy
            } else if name.starts_with(crate::live::isolation::PREFIX) {
                Kind::HarnessHome
            } else {
                return None;
            };
            let run = match kind {
                Kind::LoginCopy => Some(crate::live::login::PREFIX),
                Kind::HarnessHome => Some(crate::live::isolation::PREFIX),
                _ => None,
            }
            .and_then(|prefix| name.strip_prefix(prefix))
            .and_then(|rest| rest.split('-').next())
            .and_then(|id| id.parse::<u32>().ok())
            .map(|pid| (pid, crate::process::is_alive(pid)));
            left(&entry.path(), kind, now, older, run)
        })
        .collect();
    found.sort_by(|one, other| (one.kind, &one.path).cmp(&(other.kind, &other.path)));
    found
}

/// The project folders in `live`.
fn in_live(live: &Path, now: SystemTime, older: Duration) -> Vec<Left> {
    let Ok(entries) = std::fs::read_dir(live) else {
        return Vec::new();
    };
    let mut found: Vec<Left> = entries
        .flatten()
        .filter_map(|entry| left(&entry.path(), Kind::Project, now, older, None))
        .collect();
    found.sort_by(|one, other| one.path.cmp(&other.path));
    found
}

/// `path` as a left folder, where it is a folder itself (a link is none of the run's) at least
/// `older` old.
fn left(
    path: &Path,
    kind: Kind,
    now: SystemTime,
    older: Duration,
    run: Option<(u32, bool)>,
) -> Option<Left> {
    let found = std::fs::symlink_metadata(path).ok()?;
    if !found.is_dir() {
        return None;
    }
    let age = found
        .modified()
        .ok()
        .and_then(|changed| now.duration_since(changed).ok());
    if age.is_some_and(|age| age < older) {
        return None;
    }
    Some(Left {
        path: path.to_path_buf(),
        kind,
        age,
        kept: std::fs::read_to_string(path.join(KEPT)).ok(),
        run,
        used: Used::NotAsked,
    })
}

/// The parents [`say_copies`] has said for, in this process.
static SAID: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Says, once for this process and `parent`, how many login copies and harness homes of earlier
/// runs stand in `parent` (the temp folder a run makes its own in), and that none is taken: a
/// copy of a run that is gone may be in use by a harness that run detached, or be its evidence,
/// and holds a login, so a person removes it (`cargo xtask live sweep` lists them). Said in a
/// line, and answers how many it said. The processes of the machine are not read.
#[allow(clippy::disallowed_methods, clippy::print_stderr)] // Said in the output of the run.
pub fn say_copies(parent: &Path) -> usize {
    let first = SAID
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(parent.to_path_buf());
    if !first {
        return 0;
    }
    let own = std::process::id();
    let found: Vec<Left> = in_temp(parent, SystemTime::now(), Duration::ZERO, false)
        .into_iter()
        .filter(|copy| copy.run.is_none_or(|(pid, _)| pid != own))
        .collect();
    if found.is_empty() {
        return 0;
    }
    let gone = found
        .iter()
        .filter(|copy| copy.run.is_some_and(|(_, there)| !there))
        .count();
    let names: Vec<String> = found
        .iter()
        .take(5)
        .filter_map(|copy| copy.path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    eprintln!(
        "{} login cop{} of earlier runs stand{} in {} ({gone} of runs that are gone): {}{}; none is \
         taken, and `cargo xtask live sweep` says what each is",
        found.len(),
        if found.len() == 1 { "y" } else { "ies" },
        if found.len() == 1 { "s" } else { "" },
        parent.display(),
        names.join(", "),
        if found.len() > names.len() {
            format!(" and {} more", found.len() - names.len())
        } else {
            String::new()
        }
    );
    found.len()
}

// A test stamps its folders by the clock the count reads their ages against.
#[allow(clippy::disallowed_methods)]
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const DAY: Duration = OLDER;

    /// Stamps the folder `folder` as last changed `ago` ago. Windows opens a folder only with
    /// the flag that asks for it (`FILE_FLAG_BACKUP_SEMANTICS`).
    fn aged(folder: &Path, ago: Duration) {
        let mut open = fs::OpenOptions::new();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.write(true).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        open.read(true);
        open.open(folder)
            .unwrap()
            .set_modified(SystemTime::now() - ago)
            .unwrap();
    }

    /// A home called `name` in `folder`, last changed `ago` ago, with a note in it.
    fn home(folder: &Path, name: &str, ago: Duration) -> PathBuf {
        let home = folder.join(name);
        fs::create_dir_all(home.join("workspace")).unwrap();
        fs::write(home.join("workspace").join("note"), "a note").unwrap();
        aged(&home, ago);
        home
    }

    #[test]
    fn only_a_rig_folder_older_than_a_day_is_counted() {
        let folder = tempfile::tempdir().unwrap();
        let old = home(folder.path(), "consensflow-integration-old", DAY * 2);
        home(folder.path(), "consensflow-integration-young", DAY / 2);
        home(folder.path(), "another-old-folder", DAY * 2);
        fs::write(
            folder.path().join("consensflow-integration-old-left.txt"),
            "a report",
        )
        .unwrap();
        #[cfg(unix)]
        {
            let elsewhere = home(folder.path(), "elsewhere", DAY * 2);
            std::os::unix::fs::symlink(
                &elsewhere,
                folder.path().join("consensflow-integration-link"),
            )
            .unwrap();
        }

        assert_eq!(older(folder.path(), SystemTime::now(), DAY), [old]);
    }

    /// A rig's start takes nothing of another run's: an older folder a run kept on purpose
    /// (`CONSENSFLOW_LIVE_KEEP`, its evidence) and one a killed run left both stand after it, and
    /// nobody's process is asked about. The rig only counts them (T-1557, finding 3a).
    #[test]
    fn a_rig_start_leaves_the_folders_older_runs_left_and_only_counts_them() {
        let folder = tempfile::tempdir().unwrap();
        let kept = home(folder.path(), "consensflow-integration-kept", DAY * 2);
        fs::write(kept.join("consensflow-kept.txt"), "kept on purpose").unwrap();
        aged(&kept, DAY * 2);
        let left = home(folder.path(), "consensflow-integration-left", DAY * 2);

        assert_eq!(homes(folder.path()), 2);

        for there in [&kept, &left] {
            assert!(
                there.join("workspace").join("note").is_file(),
                "a rig's start took {}",
                there.display()
            );
        }
    }
}
