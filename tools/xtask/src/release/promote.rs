//! `release promote X.Y.Z-alpha.N`: the plain `X.Y.Z` of an alpha a person has
//! tested, released from the alpha's own commit with the version alone changed.
//!
//! 1. The notes say something, and the checkout is clean, on `main`, and `main`
//!    is upstream's.
//! 2. The alpha's tag is on upstream, and its commit is the one promoted;
//!    `feed-alpha` serves that very alpha. An alpha that a later one replaced
//!    is not what alpha installs run, and is not promoted.
//! 3. `vX.Y.Z` is no tag in the checkout, on origin or on upstream, and
//!    `feed-stable` serves a version before it: a tag whose publish died leaves
//!    the feed behind, and a feed can be ahead of any tag here.
//! 4. In a tree of its own at the alpha's commit (`app/src-tauri/target/promote/`,
//!    removed after), the version becomes `X.Y.Z` in the five files that hold
//!    it: `package.json`, `package-lock.json` (twice), the root `Cargo.toml`,
//!    `app/src-tauri/tauri.conf.json`, and the root `Cargo.lock`, which
//!    `cargo update --workspace --offline` writes. Any change beyond those
//!    files, or in them beyond `"X.Y.Z-alpha.N"` becoming `"X.Y.Z"`, is refused
//!    ([`version_only`]). The change is committed there, its parent the alpha's
//!    commit.
//! 5. Where `main` is still the alpha's commit, the version commit goes on
//!    `main`, to origin and upstream, and the checkout's `main` is moved to it;
//!    CI's push run on it is watched, and the Release workflow's dry run.
//!    Where `main` has moved on, the release comes from a branch,
//!    `release/X.Y.Z`, at the version commit, pushed to origin and upstream; CI
//!    runs on it by hand (`ci.yml` runs on its own only for `main`) and is
//!    watched. There is no dry run on it: the `release` environment admits
//!    `main` and `v*` tags alone, and the tag's own Release run gates and
//!    smoke-tests the commit before it publishes anything.
//! 6. The tag `vX.Y.Z` on the version commit, with the notes; its Release run
//!    watched to the end; both feeds read until they serve `X.Y.Z`.
//!
//! Nothing goes back to `main` from a branch: its one commit is the version, and
//! `main` holds more than the alpha did. What `main` needs is its next version
//! after `X.Y.Z` before its next alpha, which `release alpha` enforces, as
//! `feed-alpha` then serves `X.Y.Z`.
//!
//! `--dry-run` does 1 to 4, the commit made and left in no branch, and says
//! where it would go.

use std::fs;
use std::path::{Path, PathBuf};

use cf_publish::version::Version;
use cf_release::version::source_version;

use super::github::{served_text, Watched};
use super::{notes_at, plain_of, refused, short, Error, Options, Release, ALPHA_FEED, STABLE_FEED};
use crate::process::Invocation;

/// The files a version change changes, and nothing else, as the checkout names them.
pub(super) const VERSION_FILES: [&str; 5] = [
    "package.json",
    "package-lock.json",
    "Cargo.toml",
    "Cargo.lock",
    "app/src-tauri/tauri.conf.json",
];

/// The files the promotion writes itself, and how many times each says the
/// version; `Cargo.lock` is cargo's.
const WRITTEN: [(&str, usize); 4] = [
    ("package.json", 1),
    ("package-lock.json", 2),
    ("Cargo.toml", 1),
    ("app/src-tauri/tauri.conf.json", 1),
];

pub(super) fn release(release: &mut Release, options: &Options) -> Result<String, Error> {
    let alpha = options.version.as_deref().unwrap_or_default();
    let plain = plain_of(alpha)?.to_owned();
    let notes = notes_at(&options.notes)?;
    let main = release.main_as_upstream_has_it()?;

    let alpha_tag = format!("v{alpha}");
    let fetch = format!("+refs/tags/{alpha_tag}:refs/tags/{alpha_tag}");
    if !release.git_says_yes(&["fetch", "-q", "--no-tags", "upstream", &fetch])? {
        return Err(refused(format!(
            "{alpha_tag} is not on upstream: {alpha} was never released"
        )));
    }
    let commit = release.git(&["rev-list", "-n", "1", &alpha_tag])?;
    let tag = format!("v{plain}");
    release.tag_is_new(&tag)?;

    let alpha_served = release.feed_version(ALPHA_FEED)?;
    if alpha_served.as_ref().map(Version::as_str) != Some(alpha) {
        return Err(refused(format!(
            "{ALPHA_FEED} serves {}, not {alpha}: only the alpha it serves, which alpha installs run, is promoted",
            served_text(alpha_served.as_ref())
        )));
    }
    let ours = Version::parse(&plain).map_err(|cause| refused(cause.to_string()))?;
    if let Some(served) = release
        .feed_version(STABLE_FEED)?
        .filter(|served| *served >= ours)
    {
        return Err(refused(format!(
            "{STABLE_FEED} already serves {served}: {plain} was released, or a later version"
        )));
    }
    let moved = main != commit;
    let branch = format!("release/{plain}");
    if moved {
        for remote in ["origin", "upstream"] {
            let heads = format!("refs/heads/{branch}");
            if !release
                .git(&["ls-remote", "--heads", remote, &heads])?
                .is_empty()
            {
                return Err(refused(format!(
                    "{remote} already has {branch}: a promotion of {plain} was begun; finish it or delete the branch"
                )));
            }
        }
    }

    let promoted = version_commit(release, &commit, alpha, &plain)?;
    release.say(&format!(
        "{plain}: {} on {alpha}'s commit {}, the version alone changed",
        short(&promoted),
        short(&commit)
    ));
    let next = next_patch(&plain);
    if options.dry_run {
        let place = if moved {
            format!("pushed as {branch} (main has moved on from {alpha}'s commit)")
        } else {
            "pushed to main".to_owned()
        };
        return Ok(format!(
            "dry run: every check passed; {} would be {place} on origin and upstream, tagged {tag} with {} and pushed\n",
            short(&promoted),
            notes.display()
        ));
    }

    if moved {
        release.git(&["branch", &branch, &promoted])?;
        release.git(&["push", "-q", "origin", &branch])?;
        release.git(&["push", "-q", "upstream", &branch])?;
        let before = release.newest_run("ci.yml", Some(&promoted), Some("workflow_dispatch"))?;
        release.dispatch("ci.yml", &branch)?;
        release.watch(&Watched {
            label: "ci",
            workflow: "ci.yml",
            commit: &promoted,
            event: Some("workflow_dispatch"),
            before,
            both_platforms: true,
        })?;
        release.say(&format!(
            "no dry run on {branch}: the release environment admits main and v* tags alone; \
             the tag's own Release run gates and smoke-tests it before it publishes"
        ));
    } else {
        let onto_main = format!("{promoted}:refs/heads/main");
        release.git(&["push", "-q", "origin", &onto_main])?;
        release.git(&["push", "-q", "upstream", &onto_main])?;
        release.git(&["merge", "-q", "--ff-only", &promoted])?;
        release.watch(&Watched {
            label: "ci",
            workflow: "ci.yml",
            commit: &promoted,
            event: Some("push"),
            before: None,
            both_platforms: true,
        })?;
        let before =
            release.newest_run("release.yml", Some(&promoted), Some("workflow_dispatch"))?;
        release.dispatch("release.yml", "main")?;
        release.watch(&Watched {
            label: "dry run",
            workflow: "release.yml",
            commit: &promoted,
            event: Some("workflow_dispatch"),
            before,
            both_platforms: false,
        })?;
    }

    let before = release.newest_run("release.yml", Some(&promoted), Some("push"))?;
    release.tag_and_push(&tag, &notes, &promoted)?;
    release.watch(&Watched {
        label: "release",
        workflow: "release.yml",
        commit: &promoted,
        event: Some("push"),
        before,
        both_platforms: false,
    })?;
    release.wait_for_feed(ALPHA_FEED, &plain)?;
    release.wait_for_feed(STABLE_FEED, &plain)?;
    let main_now = if moved {
        format!(
            "nothing goes back to main: {branch} holds {plain}'s version commit alone, and main holds more than {alpha} did; \
             main's next alpha needs a version after {plain} ({next}-alpha.1 or later)"
        )
    } else {
        format!(
            "main is at {plain}: its next alpha needs a version after it ({next}-alpha.1 or later)"
        )
    };
    Ok(format!(
        "released {plain} from {alpha}'s commit {} as {}{}: {ALPHA_FEED} and {STABLE_FEED} serve it\n{main_now}\n",
        short(&commit),
        short(&promoted),
        if moved { format!(" on {branch}") } else { " on main".to_owned() },
    ))
}

/// The version after `plain` by its patch: `3.0.7` after `3.0.6`.
fn next_patch(plain: &str) -> String {
    let mut numbers: Vec<String> = plain.split('.').map(str::to_owned).collect();
    if let Some(patch) = numbers.last_mut() {
        if let Ok(number) = patch.parse::<u64>() {
            *patch = (number + 1).to_string();
        }
    }
    numbers.join(".")
}

/// Where the promotion's tree is, in the checkout's build folder (which git ignores).
fn tree_of(root: &Path, plain: &str) -> PathBuf {
    ["app", "src-tauri", "target", "promote", plain]
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// `relative`, written with `/` as the repository does, under `root`.
fn under(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Makes the version commit in a tree of its own at `commit`, and takes the
/// tree away again whatever happened in it: the commit.
fn version_commit(
    release: &mut Release,
    commit: &str,
    alpha: &str,
    plain: &str,
) -> Result<String, Error> {
    let tree = tree_of(&release.root, plain);
    let tree_text = tree.to_string_lossy().into_owned();
    // One left by a run that died is taken away first.
    release.git_says_yes(&["worktree", "remove", "--force", &tree_text])?;
    release.git(&["worktree", "prune"])?;
    release.git(&["worktree", "add", "-q", "--detach", &tree_text, commit])?;
    let made = change_the_version(release, &tree, commit, alpha, plain);
    let removed = release.git(&["worktree", "remove", "--force", &tree_text]);
    let made = made?;
    removed?;
    Ok(made)
}

fn change_the_version(
    release: &mut Release,
    tree: &Path,
    commit: &str,
    alpha: &str,
    plain: &str,
) -> Result<String, Error> {
    let source = source_version(tree).map_err(|cause| refused(cause.to_string()))?;
    if source != alpha {
        return Err(refused(format!(
            "v{alpha} is on {}, whose sources say {source}: it is not that alpha's commit",
            short(commit)
        )));
    }
    let read = |path: &str| {
        let file = under(tree, path);
        fs::read_to_string(&file).map_err(|source| Error::File {
            action: "read",
            path: file,
            source,
        })
    };
    let before: Vec<String> = VERSION_FILES
        .iter()
        .map(|path| read(path))
        .collect::<Result<_, _>>()?;
    let (from, to) = (quoted(alpha), quoted(plain));
    for (path, times) in WRITTEN {
        let file = under(tree, path);
        let text = read(path)?;
        let found = text.matches(&from).count();
        if found != times {
            return Err(refused(format!(
                "{path} says {from} {found} times at {}, not {times}: its version cannot be changed alone",
                short(commit)
            )));
        }
        fs::write(&file, text.replace(&from, &to)).map_err(|source| Error::File {
            action: "write",
            path: file,
            source,
        })?;
    }
    let lockfile = Invocation::new("cargo", tree).args(["update", "--workspace", "--offline"]);
    let updated = release.system.capture(&lockfile)?;
    if updated.code != 0 {
        return Err(Error::Program {
            command: lockfile.display(),
            code: updated.code,
            said: updated.stderr.trim().to_owned(),
        });
    }
    let status = release.git_in(tree, &["status", "--porcelain", "--untracked-files=all"])?;
    let after: Vec<String> = VERSION_FILES
        .iter()
        .map(|path| read(path))
        .collect::<Result<_, _>>()?;
    let files: Vec<(&str, &str, &str)> = VERSION_FILES
        .iter()
        .zip(before.iter().zip(&after))
        .map(|(path, (before, after))| (*path, before.as_str(), after.as_str()))
        .collect();
    version_only(alpha, plain, &status, &files)?;
    let message = format!(
        "{plain}\n\nPromoted from {alpha} ({commit}), the version alone changed \
         (cargo xtask release promote).\n"
    );
    release.git_in(tree, &["commit", "-q", "-a", "-m", &message])?;
    Ok(release
        .git_in(tree, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned())
}

/// `version` in quotes, as each version file writes it.
fn quoted(version: &str) -> String {
    format!("\"{version}\"")
}

/// Refuses a promotion that changed more than the version. `status` is what
/// `git status --porcelain` says of its tree: every path it names must be a
/// version file. `files` is each version file, as the alpha's commit had it and
/// as the tree has it now: the second must be the first with every `"alpha"`
/// written `"plain"`, and nothing else, and the first must say the alpha.
pub(super) fn version_only(
    alpha: &str,
    plain: &str,
    status: &str,
    files: &[(&str, &str, &str)],
) -> Result<(), Error> {
    for line in status.lines().filter(|line| !line.trim().is_empty()) {
        let path = line.get(3..).unwrap_or(line);
        if !VERSION_FILES.contains(&path) {
            return Err(refused(format!(
                "the promotion changed {path}, which holds no version: only the version may change"
            )));
        }
    }
    let (from, to) = (quoted(alpha), quoted(plain));
    for (path, before, after) in files {
        let expected = before.replace(&from, &to);
        if expected == *before {
            return Err(refused(format!(
                "{path} does not say {from} at the alpha's commit: it is no version file of that alpha"
            )));
        }
        if expected != *after {
            return Err(refused(format!(
                "{path} changed beyond the version: only {from} may become {to}"
            )));
        }
    }
    Ok(())
}
