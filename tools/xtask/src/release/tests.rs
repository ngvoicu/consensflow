//! The release's checks and steps, on a checkout in a temporary tree whose
//! remotes are bare repositories beside it, and a GitHub that is a script:
//! nothing here reaches GitHub or moves a feed. This file is what the two
//! commands share (their words, the version they take, the notes, the change
//! that is the version alone); `alpha` and `promote` are each command's run.

mod alpha;
mod fake;
mod promote;

use std::ffi::OsString;
use std::path::Path;

use super::promote::{version_only, VERSION_FILES};
use super::*;
use fake::version_files;

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

/// What a check refused, or the error it was.
fn refusal(outcome: Result<String, Error>) -> String {
    match outcome {
        Ok(done) => panic!("it went on: {done}"),
        Err(Error::Refused(why)) => why,
        Err(other) => panic!("not a refusal: {other}"),
    }
}

/// What a refusal says to do about `tag` on upstream, whose Release run is `run`,
/// written out as a person reads it.
fn advice(tag: &str, run: Option<u64>) -> String {
    let feeds = "https://github.com/ngvoicu/consensflow/releases/download";
    let rerun = match run {
        Some(id) => format!(
            "if run {id} failed, rerun it, `gh run rerun {id} --failed --repo ngvoicu/consensflow`"
        ),
        None => "find its run, `gh run list --repo ngvoicu/consensflow --workflow release.yml --event push`, \
                 and if it failed rerun it, `gh run rerun <id> --failed --repo ngvoicu/consensflow`"
            .to_owned(),
    };
    format!(
        "{rerun}: the publisher finishes from where it stopped; then read the feeds, \
         {feeds}/feed-alpha/latest.json and {feeds}/feed-stable/latest.json. \
         A failure that is real spends {}: release the next version",
        &tag[1..]
    )
}

/// What a refusal says of `tag` that the checkout or origin has and upstream does
/// not: it is `place`, and `undo` takes it away.
fn not_on_upstream(tag: &str, place: &str, undo: &str) -> String {
    format!(
        "{tag} is {place} but not on upstream: a run stopped before upstream had it, and no \
         Release run started. Push it, `git push upstream {tag}`, and watch its Release run; \
         or delete it, `{undo}`, and run again"
    )
}

#[test]
fn each_command_takes_its_notes_its_dry_run_and_promote_its_alpha() {
    assert_eq!(
        options(&words("--notes n.txt"), false).unwrap(),
        Options {
            version: None,
            notes: "n.txt".into(),
            dry_run: false
        }
    );
    assert_eq!(
        options(&words("3.0.6-alpha.1 --dry-run --notes n.txt"), true).unwrap(),
        Options {
            version: Some("3.0.6-alpha.1".into()),
            notes: "n.txt".into(),
            dry_run: true
        }
    );
    for (line, takes_version, said) in [
        ("", false, "release takes --notes FILE"),
        ("--notes", false, "--notes takes a file"),
        (
            "--notes n.txt 3.0.6-alpha.1",
            false,
            "release does not take 3.0.6-alpha.1",
        ),
        (
            "--notes n.txt --force",
            false,
            "release does not take --force",
        ),
        (
            "--notes n.txt",
            true,
            "release promote takes the alpha to promote, X.Y.Z-alpha.N",
        ),
        (
            "3.0.6-alpha.1 3.0.6-alpha.2 --notes n.txt",
            true,
            "release does not take 3.0.6-alpha.2",
        ),
    ] {
        let Err(Failure::Usage(message)) = options(&words(line), takes_version) else {
            panic!("{line}: taken");
        };
        assert_eq!(message, said, "{line}");
    }
}

#[test]
fn an_alpha_is_x_y_z_alpha_n_and_nothing_else_the_workflow_would_take() {
    assert_eq!(plain_of("3.0.6-alpha.1").unwrap(), "3.0.6");
    assert_eq!(plain_of("10.20.30-alpha.17").unwrap(), "10.20.30");
    for version in [
        "3.0.6",
        "3.0.6-rc.1",
        "3.0.6-beta.1",
        "3.0.6-alpha",
        "3.0.6-alpha.x",
        "3.0.6-alpha.1.2",
        "3.0.6-alpha-1",
    ] {
        let said = refusal(plain_of(version).map(str::to_owned));
        assert_eq!(
            said,
            format!("{version} is not an alpha: an alpha is X.Y.Z-alpha.N")
        );
    }
    // What is no version at all is told as the release tool tells it.
    assert_eq!(
        refusal(plain_of("v3.0.6-alpha.1").map(str::to_owned)),
        "the version is not a canonical semantic version: v3.0.6-alpha.1"
    );
    assert_eq!(
        refusal(plain_of("3.0.6-alpha.01").map(str::to_owned)),
        "the version has a leading-zero prerelease identifier"
    );
}

#[test]
fn notes_are_a_file_whose_first_line_is_a_title() {
    let folder = tempfile::tempdir().unwrap();
    let good = folder.path().join("good.txt");
    fake::write(&good, "3.0.6-alpha.1: the title\n\nThe rest.\n");
    assert_eq!(notes_at(&good).unwrap(), good);
    for (name, text) in [
        ("empty.txt", ""),
        ("blank.txt", "  \nThe rest, with no title.\n"),
    ] {
        let notes = folder.path().join(name);
        fake::write(&notes, text);
        let said = refusal(notes_at(&notes).map(|path| path.display().to_string()));
        assert!(
            said.contains("have no title: their first line is the release's"),
            "{said}"
        );
    }
    let missing = notes_at(&folder.path().join("missing.txt")).unwrap_err();
    assert!(
        matches!(
            missing,
            Error::File {
                action: "read the notes",
                ..
            }
        ),
        "{missing}"
    );
}

/// The version files of the alpha and of the plain version, as a promotion
/// leaves them: each path, before and after.
fn changed_files(alpha: &str, plain: &str) -> Vec<(&'static str, String, String)> {
    version_files(alpha)
        .into_iter()
        .zip(version_files(plain))
        .map(|((path, before), (_, after))| (path, before, after))
        .collect()
}

fn borrowed<'a>(
    files: &'a [(&'static str, String, String)],
) -> Vec<(&'static str, &'a str, &'a str)> {
    files
        .iter()
        .map(|(path, before, after)| (*path, before.as_str(), after.as_str()))
        .collect()
}

const STATUS: &str = " M Cargo.lock\n M Cargo.toml\n M app/src-tauri/tauri.conf.json\n M package-lock.json\n M package.json\n";

#[test]
fn the_version_alone_changed_in_the_version_files_is_a_promotion() {
    let files = changed_files("3.0.6-alpha.1", "3.0.6");
    version_only("3.0.6-alpha.1", "3.0.6", STATUS, &borrowed(&files)).unwrap();
    // The files are every one a version change changes.
    let paths: Vec<_> = files.iter().map(|(path, ..)| *path).collect();
    assert_eq!(paths, VERSION_FILES);
}

#[test]
fn a_path_beyond_the_version_files_is_refused() {
    let files = changed_files("3.0.6-alpha.1", "3.0.6");
    for line in [" M README.md", "?? stray.txt", " M crates/cf/src/lib.rs"] {
        let status = format!("{STATUS}{line}\n");
        let said = refusal(
            version_only("3.0.6-alpha.1", "3.0.6", &status, &borrowed(&files))
                .map(|()| String::new()),
        );
        let path = &line[3..];
        assert_eq!(
            said,
            format!(
                "the promotion changed {path}, which holds no version: only the version may change"
            )
        );
    }
}

#[test]
fn a_version_file_changed_beyond_its_version_is_refused() {
    for (at, more) in [
        (3, "\n[[package]]\nname = \"new\"\nversion = \"0.1.0\"\n"),
        (0, " "),
        (4, "\n"),
    ] {
        let mut files = changed_files("3.0.6-alpha.1", "3.0.6");
        files[at].2 += more;
        let said = refusal(
            version_only("3.0.6-alpha.1", "3.0.6", STATUS, &borrowed(&files))
                .map(|()| String::new()),
        );
        assert_eq!(
            said,
            format!(
                "{} changed beyond the version: only \"3.0.6-alpha.1\" may become \"3.0.6\"",
                files[at].0
            )
        );
    }
    // A version left as it was in one file is a change beyond it too.
    let mut files = changed_files("3.0.6-alpha.1", "3.0.6");
    files[1].2 = files[1].1.clone();
    let said = refusal(
        version_only("3.0.6-alpha.1", "3.0.6", STATUS, &borrowed(&files)).map(|()| String::new()),
    );
    assert!(
        said.starts_with("package-lock.json changed beyond the version"),
        "{said}"
    );
}

#[test]
fn a_version_file_that_does_not_say_the_alpha_is_refused() {
    // alpha.10's files say no "3.0.6-alpha.1": the quotes keep one from being the other.
    let files = changed_files("3.0.6-alpha.10", "3.0.6");
    let said = refusal(
        version_only("3.0.6-alpha.1", "3.0.6", STATUS, &borrowed(&files)).map(|()| String::new()),
    );
    assert_eq!(
        said,
        "package.json does not say \"3.0.6-alpha.1\" at the alpha's commit: it is no version file of that alpha"
    );
}

#[test]
fn the_feeds_are_the_ones_the_manifest_names_for_every_release_after_the_bridge() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("app").join("feeds.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["feeds"]["alpha"], ALPHA_FEED);
    assert_eq!(manifest["feeds"]["stable"], STABLE_FEED);
}

#[test]
fn the_version_files_are_every_file_the_sources_name_their_version_in() {
    // The three the release tool holds to one version are among them, and the two lockfiles.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let version = cf_release::version::source_version(root).unwrap();
    let quoted = format!("\"{version}\"");
    for path in VERSION_FILES {
        let text = fs::read_to_string(promote_path(root, path)).unwrap();
        assert!(text.contains(&quoted), "{path} does not say {version}");
    }
}

fn promote_path(root: &Path, relative: &str) -> std::path::PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}
