//! The live app wrote outside its home: a worker's answer copied into the project as
//! `deliveries/d-N.md` (303 files across the owner's projects by 2026-09-18, alpha.61),
//! Pi's mailboxes as `<project>/.consensflow/`, and old launchers in `~/.local/bin` (the
//! issue `live-app-writes-into-projects` in the brain, and its write audit of
//! 2026-09-19). The Node-era writes are gone (the clean slate, 2026-10-02), and no test
//! held it (T-971).
//!
//! What the owner should have: a full round of a board (a chief, a worker launched on a
//! task, its result, the chief's accept) writes nothing outside ConsensFlow's home but
//! what the project's own settings name, and here they name none. The case runs the round
//! with every place a program finds a home in pointed at decoys of its own, made before
//! any daemon starts (the home, the harnesses' configuration folders, the temp folder),
//! takes the size, time and bytes of every file in them and in the project's folder
//! before and after, and names every change. The stand-in Claude's own transcripts and
//! window status files, which the harness writes and ConsensFlow reads, are the one change
//! that is not ConsensFlow's: the first run (2026-10-30) found that and nothing else.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cf_e2e::rig::{Project, Rig};
use cf_e2e::{files, Error};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::tier;
use crate::{config, secs, Outcome};

/// What a file is, as a snapshot keeps it: its size, its time and its bytes' hash; a
/// folder and a link by what they are.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    File {
        size: u64,
        modified: Option<SystemTime>,
        hash: u64,
    },
    Folder,
    Link(PathBuf),
}

/// Every entry under `folder`, by its path below it.
fn snapshot(folder: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut entries = BTreeMap::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(at) = folders.pop() {
        let Ok(listed) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in listed.flatten() {
            let path = entry.path();
            let below = path.strip_prefix(folder).unwrap_or(&path).to_path_buf();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let kept = if meta.file_type().is_symlink() {
                Entry::Link(std::fs::read_link(&path).unwrap_or_default())
            } else if meta.is_dir() {
                folders.push(path.clone());
                Entry::Folder
            } else {
                let mut hash = DefaultHasher::new();
                hash.write(&std::fs::read(&path).unwrap_or_default());
                Entry::File {
                    size: meta.len(),
                    modified: meta.modified().ok(),
                    hash: hash.finish(),
                }
            };
            entries.insert(below, kept);
        }
    }
    entries
}

/// What changed from `before` to `after` under `place`: each path added, changed or
/// removed, said with it.
fn changes(
    place: &str,
    before: &BTreeMap<PathBuf, Entry>,
    after: &BTreeMap<PathBuf, Entry>,
) -> Vec<String> {
    let mut said = Vec::new();
    for (path, entry) in after {
        match before.get(path) {
            None => said.push(format!("{place}/{} added", path.display())),
            Some(was) if was != entry => said.push(format!("{place}/{} changed", path.display())),
            Some(_) => {}
        }
    }
    for path in before.keys().filter(|path| !after.contains_key(*path)) {
        said.push(format!("{place}/{} removed", path.display()));
    }
    said
}

/// Whether a change is the stand-in Claude's own, as Claude Code keeps them in its
/// configuration folder: its transcripts (`projects`), and the status of each running
/// window (`sessions/<pid>.json`, which it takes away as it ends, leaving the folder).
fn the_harnesses_own(change: &str) -> bool {
    ["home/.claude/projects", "home/.claude/sessions"]
        .iter()
        .any(|own| change.starts_with(own))
}

#[test]
fn a_full_round_writes_nothing_outside_the_home_and_the_projects_own_files() -> Outcome {
    let outer = tempfile::tempdir().map_err(|source| Error::File {
        action: "make",
        path: std::env::temp_dir(),
        source,
    })?;
    let home = outer.path().join("home");
    let temp = outer.path().join("tmp");
    // The decoy home, with what an owner's has in it.
    for (file, text) in [
        (".claude/settings.json", "{}\n"),
        (".claude/.claude.json", "{}\n"),
        (".codex/config.toml", ""),
        (".config/README", "the owner's\n"),
    ] {
        files::write(&home.join(file), text)?;
    }
    files::make_dir(&temp)?;
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut config = config()
        .homes_in(outer.path().join("rigs"))
        .var("HOME", text(&home))
        .var("CLAUDE_CONFIG_DIR", text(&home.join(".claude")))
        .var("CODEX_HOME", text(&home.join(".codex")))
        .var("XDG_CONFIG_HOME", text(&home.join(".config")))
        .var("CF_TEST_ACCEPT_RESULTS", "1");
    config = if cfg!(windows) {
        config
            .var("USERPROFILE", text(&home))
            .var("TEMP", text(&temp))
            .var("TMP", text(&temp))
    } else {
        config.var("TMPDIR", text(&temp))
    };
    let home_before = snapshot(&home);
    let temp_before = snapshot(&temp);

    let rig = Rig::start(config)?;
    let workspace = rig.workspace().to_path_buf();
    let project_before = snapshot(&workspace);
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} Read the parser", tier(&added)),
    )?;
    rig.wait_for("T-1 to be accepted", secs(90), || {
        Ok(project.task(1)?["state"] == "accepted")
    })?;
    let results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .collect();
    assert_eq!(results.len(), 1, "the worker's result reached the chief");
    let root = rig.close_keeping_the_root()?;
    let project_after = snapshot(&workspace);
    std::fs::remove_dir_all(&root).map_err(|source| Error::File {
        action: "remove",
        path: root.clone(),
        source,
    })?;

    let mut written = changes("home", &home_before, &snapshot(&home));
    written.retain(|change| !the_harnesses_own(change));
    written.extend(changes("tmp", &temp_before, &snapshot(&temp)));
    written.extend(changes("project", &project_before, &project_after));
    assert_eq!(
        written,
        Vec::<String>::new(),
        "a round wrote outside ConsensFlow's home"
    );
    Ok(())
}
