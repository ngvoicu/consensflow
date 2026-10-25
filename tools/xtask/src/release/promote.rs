//! `release promote X.Y.Z-alpha.N`: the plain `X.Y.Z` of an alpha a person has
//! tested, released from the alpha's own commit with the version alone changed,
//! from a branch `release/X.Y.Z`. It never writes `main`.
//!
//! 1. The notes say something.
//! 2. The alpha's tag is on upstream (`ls-remote`, then fetched by name: a
//!    fetch that fails is git's error, never "not released"), and its commit is
//!    the one promoted; `feed-alpha` serves that very alpha. An alpha a later one
//!    replaced is not what alpha installs run, and is not promoted.
//! 3. `vX.Y.Z` is no tag in the checkout, on origin or on upstream, and
//!    `feed-stable` serves a version before it: a tag whose publish died leaves
//!    the feed behind, and a feed can be ahead of any tag here. No
//!    `release/X.Y.Z` is in the checkout, on origin or on upstream: one a run
//!    left is refused with what to do about it.
//! 4. In a tree of its own at the alpha's commit (`app/src-tauri/target/promote/`,
//!    removed after), the version becomes `X.Y.Z` in the five files that hold
//!    it: `package.json`, `package-lock.json` (twice), the root `Cargo.toml`,
//!    `app/src-tauri/tauri.conf.json`, and the root `Cargo.lock`, which
//!    `cargo update --workspace --offline` writes. Any change beyond those
//!    files, or in them beyond `"X.Y.Z-alpha.N"` becoming `"X.Y.Z"`, is refused
//!    ([`version_only`]). The change is committed there, its parent the alpha's
//!    commit.
//! 5. The branch `release/X.Y.Z` at the version commit, in the checkout and
//!    pushed to origin and upstream. Its push runs CI (`ci.yml` runs on
//!    `release/*`), which is the gate of the version commit: the release
//!    publishes only a commit CI passed on both platforms, and it is watched to
//!    its end before any tag. A CI run that fails leaves the branch and no tag,
//!    and says how to rerun it and finish by hand, or start over. There is no
//!    Release dry run (the `release` environment admits `main` and `v*` tags
//!    alone): the tag's own Release run builds, signs and smoke-tests both
//!    platforms' packages before it publishes.
//! 6. The tag `vX.Y.Z` on the version commit, with the notes, pushed to origin
//!    and upstream; its Release run checks CI's green gate of that commit (its
//!    `gate` job), and is watched to its end; both feeds read until they serve
//!    `X.Y.Z`. A Release run that fails on a flake is finished with `gh run rerun`.
//!
//! Nothing goes back to `main`: the branch's one commit is the version, and
//! `main`'s next alpha needs a version after `X.Y.Z` (`X.Y.(Z+1)-alpha.1` or
//! later), which `release alpha` enforces, as `feed-alpha` then serves `X.Y.Z`.
//!
//! `--dry-run` does 1 to 4, the commit made and left in no branch, asks `gh`
//! for CI's runs as 5 does before its first push, and says what it would push.

use std::fs;
use std::path::{Path, PathBuf};

use cf_publish::version::Version;
use cf_release::ci_passed::CI_WORKFLOW;
use cf_release::version::source_version;

use super::github::{finish_by_hand, Left, Watched};
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

    let alpha_tag = format!("v{alpha}");
    let reference = format!("refs/tags/{alpha_tag}");
    if release
        .git(&["ls-remote", "--tags", "upstream", &reference])?
        .is_empty()
    {
        return Err(refused(format!(
            "{alpha_tag} is not on upstream: {alpha} was never released"
        )));
    }
    let fetch = format!("+{reference}:{reference}");
    release.git(&["fetch", "-q", "--no-tags", "upstream", &fetch])?;
    let commit = release.git(&["rev-list", "-n", "1", &alpha_tag])?;
    let tag = format!("v{plain}");
    release.tag_is_new(&tag)?;

    let alpha_served = release.served_by(ALPHA_FEED)?;
    if alpha_served.as_str() != alpha {
        return Err(refused(format!(
            "{ALPHA_FEED} serves {alpha_served}, not {alpha}: only the alpha it serves, which alpha installs run, is promoted"
        )));
    }
    let ours = Version::parse(&plain).map_err(|cause| refused(cause.to_string()))?;
    let stable_served = release.served_by(STABLE_FEED)?;
    if stable_served >= ours {
        return Err(refused(format!(
            "{STABLE_FEED} already serves {stable_served}: {plain} was released, or a later version"
        )));
    }
    let branch = format!("release/{plain}");
    branch_is_new(release, &branch, &tag, &commit)?;

    let promoted = version_commit(release, &commit, alpha, &plain)?;
    release.say(&format!(
        "{plain}: {} on {alpha}'s commit {}, the version alone changed",
        short(&promoted),
        short(&commit)
    ));
    // Asked before anything is pushed, and in the dry run too: a gh that is not
    // logged in, or no network, then stops it with nothing on a remote.
    let ci_before = release.newest_run(CI_WORKFLOW, Some(&promoted), Some("push"))?;
    if options.dry_run {
        return Ok(format!(
            "dry run: every check passed; {} would be {branch} in the checkout and on origin and upstream, \
             tagged {tag} with {} and pushed\n",
            short(&promoted),
            notes.display()
        ));
    }

    release.git(&["branch", &branch, &promoted])?;
    release.push("origin", &branch, |said| {
        format!(
            "origin refused {branch} ({said}): no remote has it; delete it, `git branch -D {branch}`, and run again"
        )
    })?;
    release.push("upstream", &branch, |said| {
        format!(
            "upstream refused {branch} ({said}), which origin has; delete it, \
             `git push origin --delete {branch} && git branch -D {branch}`, and run again"
        )
    })?;
    // The branch's push started CI on the version commit: the gate, before the tag.
    release.watch(&Watched {
        label: "ci",
        workflow: CI_WORKFLOW,
        commit: &promoted,
        event: Some("push"),
        before: ci_before,
        both_platforms: true,
        left: Left::Branch {
            branch: &branch,
            tag: &tag,
            commit: &promoted,
        },
    })?;
    let before = release.newest_run("release.yml", Some(&promoted), Some("push"))?;
    release.tag_and_push(&tag, &notes, &promoted)?;
    let run = release.watch(&Watched {
        label: "release",
        workflow: "release.yml",
        commit: &promoted,
        event: Some("push"),
        before,
        both_platforms: false,
        left: Left::Tag(&tag),
    })?;
    release.wait_for_feed(ALPHA_FEED, &plain, run)?;
    release.wait_for_feed(STABLE_FEED, &plain, run)?;
    Ok(format!(
        "released {plain} from {alpha}'s commit {} as {} on {branch}: {ALPHA_FEED} and {STABLE_FEED} serve it\n\
         nothing goes back to main: its next alpha needs a version after {plain} ({}-alpha.1 or later)\n",
        short(&commit),
        short(&promoted),
        next_patch(&plain)
    ))
}

/// Refuses a `branch` a promotion that stopped left, in the checkout, on
/// origin or on upstream: it stopped before its tag `tag` (which no remote has),
/// and is deleted and the promotion run again, or, when its commit is the
/// version commit on the alpha's `alpha_commit`, finished by hand.
fn branch_is_new(
    release: &mut Release,
    branch: &str,
    tag: &str,
    alpha_commit: &str,
) -> Result<(), Error> {
    let heads = format!("refs/heads/{branch}");
    let mut places = Vec::new();
    let mut undo = Vec::new();
    let mut at = None;
    for remote in ["origin", "upstream"] {
        let said = release.git(&["ls-remote", "--heads", remote, &heads])?;
        if let Some(commit) = said.split_whitespace().next() {
            at.get_or_insert_with(|| commit.to_owned());
            places.push(format!("on {remote}"));
            undo.push(format!("git push {remote} --delete {branch}"));
        }
    }
    if release.git_says_yes(&["rev-parse", "-q", "--verify", &heads])? {
        let commit = release.git(&["rev-parse", &heads])?;
        at.get_or_insert(commit);
        places.insert(0, "in the checkout".to_owned());
        undo.push(format!("git branch -D {branch}"));
    }
    let Some(at) = at else {
        return Ok(());
    };
    Err(refused(format!(
        "{branch} is {}, at {}: a promotion stopped before its tag. Start over: `{}`, and run again; \
         or, if its parent is {alpha_commit} and CI is green on it, finish it by hand: `{}`, \
         then watch its Release run and read both feeds",
        places.join(" and "),
        short(&at),
        undo.join(" && "),
        finish_by_hand(tag, &at)
    )))
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
