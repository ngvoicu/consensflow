//! `cargo xtask build-identity`: the release's bundle identifier is built by
//! GitHub's release workflows and by nothing else (T-903).
//!
//! macOS keeps what a person allows an app, Screen & System Audio Recording
//! say, for the app's bundle identifier; for an app signed ad hoc, which has no
//! certificate to name, it keeps it for the one binary's hash. A build made on a
//! developer's Mac that carried `dev.ngvoicu.consensflow` and was allowed
//! anything left a grant the installed, Developer ID signed app could never
//! satisfy: System Settings showed ConsensFlow on, and ConsensFlow read the
//! permission as missing.
//!
//! So the configuration every build starts from (`tauri.conf.json`) names
//! another identifier, the release's is the file `tauri.release.conf.json` that
//! `.github/workflows/release.yml` and `windows-build.yml` merge over it, and
//! this is the first step of `prepare:app`, which Tauri runs before every
//! `tauri build` and `tauri dev`. It works out the identifier the build will
//! have, as Tauri does (the file, the platform's file over it, and the
//! `--config` Tauri hands its hooks in `TAURI_CONFIG` over those), and refuses
//! the release's anywhere but GitHub's own runner. Nothing it cannot read is
//! allowed.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};

/// The release's bundle identifier: the installed app's, which the app itself
/// calls `PRODUCTION_IDENTIFIER` and a rule test holds this to.
pub const RELEASE: &str = "dev.ngvoicu.consensflow";

/// The variable Tauri hands a build's hooks the merged `--config` in.
const OVERRIDE: &str = "TAURI_CONFIG";

pub const COMMANDS: &[Command] = &[Command {
    words: &["build-identity"],
    about: "Refuse a build that would carry the release's bundle identifier anywhere but GitHub's runner",
    usage: "",
    run: Run::Native(run),
}];

fn run(context: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    if !args.is_empty() {
        return Err(Failure::Usage("build-identity takes no arguments".into()));
    }
    match check(&context.root, &context.env) {
        Ok(()) => Ok(0),
        Err(refused) => {
            writeln!(console.err, "build-identity: {refused}")?;
            Ok(1)
        }
    }
}

/// Where the identifier a build will have was given.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// `app/src-tauri/tauri.conf.json`, or the platform's file over it.
    File(String),
    /// The `--config` of the build.
    Override,
}

impl fmt::Display for Source {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(name) => write!(out, "{name}"),
            Self::Override => write!(out, "{OVERRIDE}, the build's --config"),
        }
    }
}

/// Why a build is not allowed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum Refusal {
    #[error(
        "this build would carry the release's bundle identifier, dev.ngvoicu.consensflow (from {from}), and \
         only GitHub's release workflows build that: macOS keeps what a person allows an app \
         for its identifier, and the installed app is never found to have what a build made \
         here was allowed. Build ConsensFlow Candidate with `cargo xtask candidate`, or \
         give --config a file whose identifier is another (app/src-tauri/tauri.candidate.conf.json)"
    )]
    Release { from: Source },
    #[error("{0}")]
    Unreadable(String),
}

/// Whether the build is on a runner GitHub hosts, as GitHub says it in the
/// variables it sets on every one: `GITHUB_ACTIONS` is `true` there, and
/// `RUNNER_ENVIRONMENT` is `github-hosted` (not `self-hosted`).
fn on_githubs_runner(env: &Env) -> bool {
    env.text("GITHUB_ACTIONS") == Some("true")
        && env.text("RUNNER_ENVIRONMENT") == Some("github-hosted")
}

/// Allows the build `env` and `root` describe, or says why not.
fn check(root: &Path, env: &Env) -> Result<(), Refusal> {
    let (identifier, from) = identifier_of(root, env)?;
    if identifier == RELEASE && !on_githubs_runner(env) {
        return Err(Refusal::Release { from });
    }
    Ok(())
}

/// The identifier the build will have, and where it was given: the last of the
/// configuration file, the platform's file and the build's `--config` to give one.
fn identifier_of(root: &Path, env: &Env) -> Result<(String, Source), Refusal> {
    let folder = root.join("app").join("src-tauri");
    let main = "tauri.conf.json";
    let mut found = read_identifier(&folder.join(main), main)?
        .map(|identifier| (identifier, Source::File(main.to_owned())))
        .ok_or_else(|| Refusal::Unreadable(format!("{main} gives no identifier")))?;
    let platform = format!("tauri.{}.conf.json", platform());
    let file = folder.join(&platform);
    if file.is_file() {
        if let Some(identifier) = read_identifier(&file, &platform)? {
            found = (identifier, Source::File(platform));
        }
    }
    if let Some(text) = env.text(OVERRIDE) {
        let merged: Value = serde_json::from_str(text)
            .map_err(|cause| Refusal::Unreadable(format!("{OVERRIDE} is not JSON: {cause}")))?;
        if let Some(identifier) = identifier_in(&merged, OVERRIDE)? {
            found = (identifier, Source::Override);
        }
    }
    Ok(found)
}

/// The name Tauri gives the platform's configuration file.
fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn read_identifier(file: &Path, name: &str) -> Result<Option<String>, Refusal> {
    let text = fs::read_to_string(file)
        .map_err(|cause| Refusal::Unreadable(format!("could not read {name}: {cause}")))?;
    let config: Value = serde_json::from_str(&text)
        .map_err(|cause| Refusal::Unreadable(format!("{name} is not JSON: {cause}")))?;
    identifier_in(&config, name)
}

/// The identifier a configuration gives: none where it does not say, and an
/// error where it says something that is not one (a merge patch with `null`
/// takes the identifier away, and a build with none is Tauri's to refuse).
fn identifier_in(config: &Value, name: &str) -> Result<Option<String>, Refusal> {
    let object = config
        .as_object()
        .ok_or_else(|| Refusal::Unreadable(format!("{name} is not a JSON object")))?;
    match object.get("identifier") {
        None => Ok(None),
        Some(Value::String(identifier)) if !identifier.is_empty() => Ok(Some(identifier.clone())),
        Some(_) => Err(Refusal::Unreadable(format!(
            "{name} gives an identifier that is not a name"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout whose `tauri.conf.json` gives `main` as the identifier, and
    /// the platform's file the text `platform_file`.
    fn checkout(main: &str, platform_file: Option<&str>) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("app").join("src-tauri");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("tauri.conf.json"),
            format!(r#"{{"productName":"ConsensFlow","identifier":"{main}"}}"#),
        )
        .unwrap();
        if let Some(text) = platform_file {
            fs::write(folder.join(format!("tauri.{}.conf.json", platform())), text).unwrap();
        }
        root
    }

    fn env(vars: &[(&str, &str)]) -> Env {
        Env::from_vars(vars.iter().copied())
    }

    const RUNNER: [(&str, &str); 2] = [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
    ];

    #[test]
    fn the_configurations_own_identifier_is_what_a_build_with_no_override_has() {
        let root = checkout("dev.ngvoicu.consensflow.local", None);
        assert_eq!(
            identifier_of(root.path(), &env(&[])),
            Ok((
                "dev.ngvoicu.consensflow.local".to_owned(),
                Source::File("tauri.conf.json".to_owned())
            ))
        );
        assert_eq!(check(root.path(), &env(&[])), Ok(()));
    }

    #[test]
    fn a_configuration_that_gives_the_release_identifier_is_refused_off_github_and_not_on_it() {
        let root = checkout(RELEASE, None);
        assert_eq!(
            check(root.path(), &env(&[])),
            Err(Refusal::Release {
                from: Source::File("tauri.conf.json".to_owned())
            })
        );
        assert_eq!(check(root.path(), &env(&RUNNER)), Ok(()));
    }

    #[test]
    fn the_override_is_merged_over_the_file_and_the_platforms_file_between() {
        let local = "dev.ngvoicu.consensflow.local";
        let release = format!(r#"{{"identifier":"{RELEASE}"}}"#);
        let root = checkout(local, Some(&release));
        // The platform's file over the main one.
        assert_eq!(
            identifier_of(root.path(), &env(&[])).unwrap().0,
            RELEASE,
            "the platform's file gives the release's"
        );
        // The override over both.
        let candidate =
            r#"{"bundle":{"resources":null},"identifier":"dev.ngvoicu.consensflow.candidate"}"#;
        assert_eq!(
            identifier_of(root.path(), &env(&[(OVERRIDE, candidate)])),
            Ok((
                "dev.ngvoicu.consensflow.candidate".to_owned(),
                Source::Override
            ))
        );
        // An override that says nothing of the identifier leaves what was there.
        let version = r#"{"version":"9.9.9"}"#;
        assert_eq!(
            check(root.path(), &env(&[(OVERRIDE, version)])),
            Err(Refusal::Release {
                from: Source::File(format!("tauri.{}.conf.json", platform()))
            })
        );
    }

    #[test]
    fn an_override_that_gives_the_release_identifier_is_refused_off_github() {
        let root = checkout("dev.ngvoicu.consensflow.local", None);
        let release = format!(r#"{{"identifier":"{RELEASE}"}}"#);
        assert_eq!(
            check(root.path(), &env(&[(OVERRIDE, &release)])),
            Err(Refusal::Release {
                from: Source::Override
            })
        );
        let said = check(root.path(), &env(&[(OVERRIDE, &release)]))
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("TAURI_CONFIG, the build's --config"),
            "{said}"
        );
    }

    #[test]
    fn githubs_runner_is_both_variables_it_sets_and_nothing_a_job_could_set_by_halves() {
        let root = checkout(RELEASE, None);
        for vars in [
            vec![("GITHUB_ACTIONS", "true")],
            vec![("RUNNER_ENVIRONMENT", "github-hosted")],
            vec![
                ("GITHUB_ACTIONS", "true"),
                ("RUNNER_ENVIRONMENT", "self-hosted"),
            ],
            vec![
                ("GITHUB_ACTIONS", "1"),
                ("RUNNER_ENVIRONMENT", "github-hosted"),
            ],
            vec![
                ("GITHUB_ACTIONS", ""),
                ("RUNNER_ENVIRONMENT", "github-hosted"),
            ],
        ] {
            assert!(
                matches!(
                    check(root.path(), &env(&vars)),
                    Err(Refusal::Release { .. })
                ),
                "{vars:?}"
            );
        }
    }

    #[test]
    fn what_cannot_be_read_is_refused_in_words_that_name_it() {
        let root = checkout("dev.ngvoicu.consensflow.local", None);
        for (text, mentions) in [
            ("{not json", "TAURI_CONFIG is not JSON"),
            ("[]", "TAURI_CONFIG is not a JSON object"),
            (r#"{"identifier":null}"#, "TAURI_CONFIG gives an identifier"),
            (r#"{"identifier":""}"#, "TAURI_CONFIG gives an identifier"),
            (r#"{"identifier":3}"#, "TAURI_CONFIG gives an identifier"),
        ] {
            let said = check(root.path(), &env(&[(OVERRIDE, text)]))
                .unwrap_err()
                .to_string();
            assert!(said.contains(mentions), "{text}: {said}");
        }
        let bare = tempfile::tempdir().unwrap();
        let said = check(bare.path(), &env(&[])).unwrap_err().to_string();
        assert!(said.contains("could not read tauri.conf.json"), "{said}");
    }

    #[test]
    fn an_override_that_is_empty_is_no_override() {
        let root = checkout("dev.ngvoicu.consensflow.local", None);
        assert_eq!(check(root.path(), &env(&[(OVERRIDE, "")])), Ok(()));
    }
}
