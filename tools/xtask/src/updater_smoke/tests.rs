//! The driver: what it is given, what it says, and the status it answers.

use cf_base::env::Env;

use super::build::BRIDGE_TAG;
use super::signing::tauri_bin;
use super::testing::{fake_bundle, Fake};
use super::*;

/// What `cargo xtask smoke-updater <words>` is, run here: the status, what it said, and what it said as a failure.
fn smoke_updater(words: &[&str]) -> (std::result::Result<i32, String>, String, String) {
    let context = Context::new(&Env::from_process()).unwrap();
    let args: Vec<OsString> = words.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = run(
        &context,
        &args,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    )
    .map_err(|failure| failure.to_string());
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Whether the machine has what a run of the smoke on fake bundles needs.
fn can_run() -> bool {
    let root = Context::new(&Env::default()).unwrap().root;
    let can = cfg!(target_os = "macos") && tauri_bin(&root).exists();
    if !can {
        eprintln!("skipped: this needs macOS and the Tauri CLI");
    }
    can
}

/// A folder holding the installed app, the update and the places a run keeps and makes things.
struct World {
    folder: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        fake_bundle(&folder.path().join("from"), &Fake::default());
        fake_bundle(
            &folder.path().join("to"),
            &Fake {
                version: "3.0.0-alpha.83",
                node: false,
                ..Fake::default()
            },
        );
        Self { folder }
    }

    fn path(&self, name: &str) -> String {
        self.folder.path().join(name).display().to_string()
    }

    /// The options that take both apps as they are built, and keep the run's things in this folder.
    fn options(&self, more: &[&str]) -> Vec<String> {
        let mut options = vec![
            "--from-app".to_string(),
            format!("{}/ConsensFlow.app", self.path("from")),
            "--from-release".into(),
            "flip".into(),
            "--to-app".into(),
            format!("{}/ConsensFlow.app", self.path("to")),
            "--cache".into(),
            self.path("cache"),
            "--machines".into(),
            self.path("machines"),
        ];
        options.extend(more.iter().map(ToString::to_string));
        options
    }

    fn smoke(&self, more: &[&str]) -> (std::result::Result<i32, String>, String, String) {
        let options = self.options(more);
        let words: Vec<&str> = options.iter().map(String::as_str).collect();
        smoke_updater(&words)
    }

    /// What the cache holds of the runs: the folder of each one.
    fn runs(&self) -> Vec<String> {
        fs::read_dir(self.folder.path().join("cache"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.starts_with("run-"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[test]
fn refuses_what_it_does_not_take_as_a_usage_before_it_does_anything() {
    let (status, out, err) = smoke_updater(&["--nope"]);
    assert_eq!(status.unwrap_err(), "unknown option: --nope");
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    let (status, ..) = smoke_updater(&["--only"]);
    assert!(status.unwrap_err().contains("--only takes a value"));
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the update path is macOS bundles and codesign"
)]
fn refuses_releases_that_are_none_and_options_that_do_not_go_together_as_a_usage() {
    let (status, out, err) = smoke_updater(&["--from", "nothing"]);
    assert_eq!(
        status.unwrap_err(),
        "nothing is no release to install from: bridge or flip"
    );
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    let (status, ..) = smoke_updater(&["--from-app", "a.app", "--from", "flip"]);
    assert_eq!(
        status.unwrap_err(),
        "--from-app takes one release: name it with --from-release"
    );
}

#[test]
#[cfg(not(target_os = "macos"))]
fn says_it_is_macos_bundles_and_codesign_elsewhere_and_ends_with_the_status_1() {
    let (status, out, err) = smoke_updater(&[]);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(out, "");
    assert_eq!(
        err,
        "smoke:updater: the update path is macOS bundles and codesign\n"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the update path is macOS bundles and codesign"
)]
fn exports_the_bridges_source_from_its_tag_into_the_cache_and_says_where() {
    let git = process::capture(
        &process::Invocation::new("git", ".").args(["tag", "--list", BRIDGE_TAG]),
        &Env::from_process(),
    )
    .unwrap();
    if git.stdout.trim() != BRIDGE_TAG {
        eprintln!("skipped: {BRIDGE_TAG} is not fetched here");
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let cache = folder.path().join("cache");
    let (status, out, err) =
        smoke_updater(&["--export-bridge", "--cache", &cache.display().to_string()]);
    assert_eq!(status.unwrap(), 0, "{err}");
    assert_eq!(err, "");
    let source = cache.join("bridge-source");
    assert_eq!(
        out,
        format!(
            "smoke:updater: the bridge's source is in {}\n",
            source.display()
        )
    );
    // What the product's plants plant in: the installed app's check of an update.
    assert!(source.join("app/src-tauri/src/update_install.rs").is_file());
    assert_eq!(
        fs::read_to_string(source.join(".exported-from")).unwrap(),
        format!("{BRIDGE_TAG}\n")
    );
    // Nothing else was made: the run's folder is for a run that builds or runs.
    assert!(!cache
        .read_dir()
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("run-")));
}

#[test]
fn takes_the_apps_it_is_given_makes_the_runs_key_and_runs_the_cases_asked_for_none_of_them() {
    if !can_run() {
        return;
    }
    let world = World::new();
    let (status, out, err) = world.smoke(&["--only", "no case has this word"]);
    assert_eq!(status.unwrap(), 0, "{err}\n{out}");
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[0].starts_with("smoke:updater: this run's updater key is "),
        "{out}"
    );
    assert!(lines[0].ends_with("updater.key.pub"), "{out}");
    assert!(
        lines[1].starts_with("smoke:updater: installed flip app: ")
            && lines[1].ends_with("/from/ConsensFlow.app"),
        "{out}"
    );
    assert!(
        lines[2].starts_with("smoke:updater: update: ")
            && lines[2].ends_with("/to/ConsensFlow.app"),
        "{out}"
    );
    assert_eq!(
        lines[3],
        "smoke:updater: == the update from the flip release"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("skip "))
            .count(),
        7,
        "{out}"
    );
    assert_eq!(
        lines[lines.len() - 2],
        "smoke:updater: flip: 0 passed, 0 failed, 7 skipped"
    );
    assert_eq!(lines[lines.len() - 1], "smoke:updater: flip: passed");
    // The run's folder, the key's, is the run's: it goes with it.
    assert_eq!(world.runs(), Vec::<String>::new());
}

#[test]
fn keeps_the_runs_folder_when_told_to() {
    if !can_run() {
        return;
    }
    let world = World::new();
    let (status, out, _) = world.smoke(&["--only", "no case has this word", "--keep"]);
    assert_eq!(status.unwrap(), 0, "{out}");
    let runs = world.runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let key = world
        .folder
        .path()
        .join("cache")
        .join(&runs[0])
        .join("keys")
        .join("updater.key");
    assert!(key.is_file(), "{}", key.display());
}

#[test]
fn builds_and_says_where_and_runs_nothing_when_told_to_build_only() {
    if !can_run() {
        return;
    }
    let world = World::new();
    let (status, out, err) = world.smoke(&["--build-only"]);
    assert_eq!(status.unwrap(), 0, "{err}");
    assert!(out.contains("smoke:updater: installed flip app: "), "{out}");
    assert!(out.contains("smoke:updater: update: "), "{out}");
    assert!(!out.contains("=="), "{out}");
    assert!(!world.folder.path().join("machines").exists());
}

#[test]
fn a_release_whose_inputs_are_not_what_the_update_needs_fails_and_the_run_ends_with_the_status_1() {
    if !can_run() {
        return;
    }
    let world = World::new();
    // The update is the installed app: it is not newer.
    let options = [
        "--from-app".to_string(),
        format!("{}/ConsensFlow.app", world.path("from")),
        "--to-app".into(),
        format!("{}/ConsensFlow.app", world.path("from")),
        "--cache".into(),
        world.path("cache"),
    ];
    let words: Vec<&str> = options.iter().map(String::as_str).collect();
    let (status, out, err) = smoke_updater(&words);
    assert_eq!(status.unwrap(), 1, "{err}");
    assert!(
        out.contains("smoke:updater: the inputs are not what the update needs: FROM_APP and TO_APP must be distinct source bundles"),
        "{out}"
    );
    assert!(out.ends_with("smoke:updater: flip: FAILED\n"), "{out}");
}

#[test]
fn a_step_that_cannot_be_done_is_said_on_the_error_stream_with_the_status_1() {
    if !can_run() {
        return;
    }
    let world = World::new();
    // Where the cache is to be there is a file.
    fs::write(world.folder.path().join("cache"), "not a folder").unwrap();
    let (status, out, err) = world.smoke(&[]);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(out, "");
    assert!(err.starts_with("smoke:updater: could not make "), "{err}");
    assert!(err.contains("cache"), "{err}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err}");
}

#[test]
fn an_app_that_is_not_there_is_an_input_that_fails_its_release_and_not_a_run_that_cannot_go_on() {
    if !can_run() {
        return;
    }
    let world = World::new();
    let (status, out, err) = smoke_updater(&[
        "--from-app",
        &world.path("nowhere.app"),
        "--to-app",
        &format!("{}/ConsensFlow.app", world.path("to")),
        "--cache",
        &world.path("cache"),
    ]);
    assert_eq!(status.unwrap(), 1);
    assert_eq!(err, "");
    // The run's own words reach whoever reads the run, and the failure says what it was.
    assert!(
        out.starts_with("smoke:updater: this run's updater key is "),
        "{out}"
    );
    assert!(
        out.contains("smoke:updater: the inputs are not what the update needs: could not find "),
        "{out}"
    );
    assert!(out.contains("nowhere.app"), "{out}");
    assert_eq!(
        world.runs(),
        Vec::<String>::new(),
        "the run's folder went with the run"
    );
}

#[test]
fn takes_the_apps_a_run_kept_where_it_is_told_to_reuse_them() {
    if !can_run() {
        return;
    }
    let world = World::new();
    // What a run keeps: the installed apps by release, and the update.
    for (kept, from) in [("flip", "from"), ("bridge", "from"), ("update", "to")] {
        bundle::copy_bundle(
            &world.folder.path().join(from).join("ConsensFlow.app"),
            &world
                .folder
                .path()
                .join("cache")
                .join(kept)
                .join("ConsensFlow.app"),
            &Env::from_process(),
        )
        .unwrap();
    }
    let (status, out, err) = smoke_updater(&[
        "--reuse",
        "--build-only",
        "--cache",
        &world.path("cache"),
        "--from",
        "flip,bridge",
    ]);
    assert_eq!(status.unwrap(), 0, "{err}");
    let cache = world.folder.path().join("cache");
    for name in ["flip", "bridge"] {
        assert!(
            out.contains(&format!(
                "smoke:updater: installed {name} app: {}",
                cache.join(name).join("ConsensFlow.app").display()
            )),
            "{out}"
        );
    }
    assert!(
        out.contains(&format!(
            "smoke:updater: update: {}",
            cache.join("update").join("ConsensFlow.app").display()
        )),
        "{out}"
    );
    // Taken, not built: no build said a word.
    assert!(!out.contains("building"), "{out}");
}

#[test]
fn what_the_command_says_of_its_options_is_what_the_options_take() {
    // Every option of the usage is one `Options::read` takes.
    for option in USAGE
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|word| word.starts_with("--"))
    {
        let line: Vec<OsString> = if option == "--export-bridge"
            || option.starts_with("--build")
            || option == "--reuse"
            || option == "--keep"
        {
            vec![option.into()]
        } else {
            vec![option.into(), "1000".into()]
        };
        assert!(Options::read(&line).is_ok(), "{option}");
    }
    assert!(USAGE.contains("--timeout MS"));
}
