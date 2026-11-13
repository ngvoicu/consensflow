//! A home laid out as the real one is, outside any rig's folder, with another run's things in
//! it: what the tests of the kit's rule (a live run ends no process and removes or overwrites no
//! path that run did not make, T-1592) run through.
//!
//! A stand-in run's home is inside its rig's own folder, so the shared folders of a real one
//! never appear in it: the live folder below `~/.consensflow-candidate` that every run puts its
//! project in, and the temp folder that holds every run's rig folder and login copies. A
//! [`Layout`] makes both in a folder of its own and puts another run's files in them: its
//! project folders (`bench`, `finish-claude`, a Devin resume's workspace, `own-cf-home`,
//! `pi-parity`), an older rig folder of a run that is gone, one a person kept, and the login
//! copies of a run that is gone and of one that is there. The test runs a case as a program of
//! its own with `HOME` and the temp folder set to them ([`Layout::vars`]), and the layout says
//! afterwards whether any of what it planted was written over ([`Layout::untouched`]) or
//! anything at all below it changed ([`Layout::unchanged`]).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::door::{self, Made};

/// The project folders another run left in the live folder, by the names the cases give theirs.
pub const OTHER_RUNS_PROJECTS: [&str; 5] = [
    "bench",
    "finish-claude",
    "devin-resume-at-rest",
    "own-cf-home",
    "pi-parity",
];

/// A process number no process has: the run of a login copy that is gone.
pub const GONE: u32 = 3_999_999_999;

/// What a path of the layout was, as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Folder(SystemTime),
    File(Vec<u8>, SystemTime),
    Link,
}

/// A home laid out as the real one is, with another run's things in it.
pub struct Layout {
    base: Made,
    planted: Vec<PathBuf>,
    before: BTreeMap<PathBuf, Entry>,
}

impl Layout {
    /// The layout, made in a folder of the temp folder that goes when it is dropped.
    pub fn new() -> io::Result<Layout> {
        let base = door::made("consensflow-layout-")?;
        let mut layout = Layout {
            base,
            planted: Vec::new(),
            before: BTreeMap::new(),
        };
        for folder in [layout.live(), layout.temp(), layout.signals()] {
            fs::create_dir_all(folder)?;
        }
        layout.plant()?;
        layout.before = snapshot(&layout.planted);
        Ok(layout)
    }

    /// The person's home of the layout: what a run's `HOME` is.
    pub fn home(&self) -> PathBuf {
        self.base.path().join("home")
    }

    /// The folder every run's project is in (`<home>/.consensflow-candidate/live`).
    pub fn live(&self) -> PathBuf {
        self.home().join(".consensflow-candidate").join("live")
    }

    /// The temp folder of the layout: what a run's rig folder and login copies are made in.
    pub fn temp(&self) -> PathBuf {
        self.base.path().join("temp")
    }

    /// A folder the test and its programs talk in, which holds nothing of another run's.
    pub fn signals(&self) -> PathBuf {
        self.base.path().join("signals")
    }

    /// The variables that make a program of the test find this layout as its machine: its home
    /// and its temp folder.
    pub fn vars(&self) -> Vec<(String, String)> {
        let home = self.home().to_string_lossy().into_owned();
        let temp = self.temp().to_string_lossy().into_owned();
        let mut vars = vec![
            ("HOME".to_owned(), home.clone()),
            ("TMPDIR".to_owned(), temp.clone()),
        ];
        if cfg!(windows) {
            vars.extend([
                ("USERPROFILE".to_owned(), home),
                ("TEMP".to_owned(), temp.clone()),
                ("TMP".to_owned(), temp),
            ]);
        }
        vars
    }

    /// The older rig folder of another run, with a kept one beside it.
    pub fn other_rig(&self) -> PathBuf {
        self.temp().join("consensflow-integration-OtherRun")
    }

    /// The rig folder a person kept.
    pub fn kept_rig(&self) -> PathBuf {
        self.temp().join("consensflow-integration-KeptRun")
    }

    /// The login copy of a run that is gone, and the one of a run that is there (the test's).
    pub fn login_copies(&self) -> [PathBuf; 2] {
        [
            self.temp()
                .join(format!("consensflow-live-claude-{GONE}-GoneRun")),
            self.temp().join(format!(
                "consensflow-live-claude-{}-ThereRun",
                std::process::id()
            )),
        ]
    }

    /// The home of a harness of a run that is gone.
    pub fn harness_home(&self) -> PathBuf {
        self.temp()
            .join(format!("consensflow-live-home-{GONE}-GoneHome"))
    }

    /// The files of another run the layout planted, each with the bytes it holds.
    pub fn planted_files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.before
            .iter()
            .filter_map(|(path, entry)| match entry {
                Entry::File(bytes, _) => Some((path.clone(), bytes.clone())),
                _ => None,
            })
            .collect()
    }

    fn plant(&mut self) -> io::Result<()> {
        let day = Duration::from_secs(24 * 60 * 60);
        for name in OTHER_RUNS_PROJECTS {
            let folder = self.live().join(name);
            write(
                &folder.join("AGENTS.md"),
                &format!("another run's brief for {name}\n"),
            )?;
            write(
                &folder.join("CLAUDE.md"),
                &format!("another run's brief for {name}, for Claude\n"),
            )?;
            write(
                &folder.join("evidence").join("result.txt"),
                &format!("what another run found in {name}\n"),
            )?;
            self.planted.push(folder);
        }
        let other = self.other_rig();
        write(
            &other.join("consensflow").join("ledger.db"),
            "another run's ledger",
        )?;
        write(&other.join("workspace").join("note"), "another run's note")?;
        write(
            &other.join("home").join(".claude").join("history"),
            "its history",
        )?;
        aged(&other, day * 2)?;
        let kept = self.kept_rig();
        write(&kept.join("workspace").join("note"), "kept for a person")?;
        write(&kept.join(door::KEPT), "kept for a person to look at\n")?;
        aged(&kept, day * 2)?;
        for copy in self.login_copies() {
            write(
                &copy.join(".credentials.json"),
                "{\"login\":\"of another run\"}",
            )?;
            write(&copy.join(".claude.json"), "{\"projects\":{}}")?;
            self.planted.push(copy);
        }
        let home = self.harness_home();
        write(
            &home.join(".codex").join("auth.json"),
            "{\"login\":\"of another run\"}",
        )?;
        self.planted.extend([other, kept, home]);
        Ok(())
    }

    /// Whether every file of another run's that was planted is there, with the bytes it had and
    /// the time it was last written, and every folder of it is: what a run that writes over no
    /// file of another's leaves. The error names each that is not.
    pub fn untouched(&self) -> Result<(), String> {
        let now = snapshot(&self.planted);
        let mut wrong = Vec::new();
        for (path, was) in &self.before {
            match (was, now.get(path)) {
                (Entry::Folder(_), Some(Entry::Folder(_))) => {}
                (was, Some(now)) if was == now => {}
                (_, None) => wrong.push(format!("{} is gone", path.display())),
                (_, Some(_)) => wrong.push(format!("{} was written over", path.display())),
            }
        }
        verdict(wrong)
    }

    /// Whether nothing below any folder of another run's changed at all: [`Layout::untouched`],
    /// and no file or folder made in them, and no folder of them touched. What a say-only run
    /// leaves.
    pub fn unchanged(&self) -> Result<(), String> {
        let now = snapshot(&self.planted);
        let mut wrong = Vec::new();
        for path in self
            .before
            .keys()
            .chain(now.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            match (self.before.get(path), now.get(path)) {
                (Some(was), Some(now)) if was == now => {}
                (Some(_), None) => wrong.push(format!("{} is gone", path.display())),
                (None, Some(_)) => wrong.push(format!("{} is new", path.display())),
                (Some(_), Some(_)) => wrong.push(format!("{} changed", path.display())),
                (None, None) => {}
            }
        }
        verdict(wrong)
    }
}

fn verdict(wrong: Vec<String>) -> Result<(), String> {
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(path, text)
}

/// Stamps the folder `folder` as last changed `ago` ago. Windows opens a folder only with the
/// flag that asks for it (`FILE_FLAG_BACKUP_SEMANTICS`).
// A layout stamps its folders by the clock a sweep reads their ages against.
#[allow(clippy::disallowed_methods)]
fn aged(folder: &Path, ago: Duration) -> io::Result<()> {
    let mut open = fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        open.write(true).custom_flags(0x0200_0000);
    }
    #[cfg(not(windows))]
    open.read(true);
    open.open(folder)?.set_modified(SystemTime::now() - ago)
}

/// Everything in `roots`, by path, as it is now.
fn snapshot(roots: &[PathBuf]) -> BTreeMap<PathBuf, Entry> {
    fn walk(path: &Path, found: &mut BTreeMap<PathBuf, Entry>) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if meta.is_dir() {
            found.insert(path.to_path_buf(), Entry::Folder(modified));
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    walk(&entry.path(), found);
                }
            }
        } else if meta.is_file() {
            found.insert(
                path.to_path_buf(),
                Entry::File(fs::read(path).unwrap_or_default(), modified),
            );
        } else {
            found.insert(path.to_path_buf(), Entry::Link);
        }
    }
    let mut found = BTreeMap::new();
    for root in roots {
        walk(root, &mut found);
    }
    found
}
