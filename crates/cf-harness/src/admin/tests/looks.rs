//! What a look at one harness says of it: its version, its release, how it
//! was installed, what Devin may be opened on, and the extension two
//! harnesses load.

use cf_proto::agents::Harness;
use serde_json::json;

use super::{fails, json_of, says, Fixture};
use crate::admin::{Distribution, Setup, Update, Version};
use crate::testing::{Response, Said, Told};

/// A look at `command` that says `version` and finds the release `release`
/// (an error when none).
fn look(command: &str, folder: &str, version: Response<Said>, release: Told) -> Fixture {
    let fixture = Fixture::new();
    fixture.install(folder, command);
    fixture
        .capture
        .answer(&format!("{command} --version"), [version]);
    let id = Harness::from_name(command).unwrap();
    fixture.latest.answer(id, [Response::Now(release)]);
    fixture
}

#[test]
fn a_cli_that_says_no_version_or_fails_to_is_told_why_and_never_a_guess() {
    for (answer, version) in [
        (
            says("unusual"),
            Version::Unknown {
                reason: "Version output was not recognized".to_owned(),
            },
        ),
        (
            fails(false),
            Version::Error {
                reason: "Version command failed".to_owned(),
            },
        ),
        (
            fails(true),
            Version::Error {
                reason: "Version check timed out".to_owned(),
            },
        ),
    ] {
        let fixture = look("claude", "bin", answer, Err("offline".to_owned()));
        let row = fixture.row("claude", false);
        assert_eq!(row.version, version);
        assert_eq!(
            row.update,
            Update::Error {
                reason: "offline".to_owned(),
                command: None
            },
            "a feed that fails is an error, not a latest or an incompatible"
        );
    }
}

#[test]
fn the_release_is_compared_a_number_at_a_time_and_kept_as_the_feed_said_it() {
    for (release, state) in [
        ("99.2.0", "available"),
        ("99.1.0", "current"),
        ("99.0.9", "current"),
        ("v99.2.0", "unknown"),
        ("codex 99.2.0", "unknown"),
    ] {
        let fixture = look(
            "codex",
            "bin",
            says("codex-cli 99.1.0"),
            Ok(release.to_owned()),
        );
        let row = fixture.row("codex", false);
        let update = json_of(&row)["update"].clone();
        assert_eq!(
            update,
            json!({
                "state": state,
                "value": release,
                "source": "https://registry.npmjs.org/@openai/codex/latest",
                "command": null,
            }),
            "{release}"
        );
    }
}

#[test]
fn a_version_nobody_could_read_is_compared_with_nothing() {
    let fixture = look("pi", "bin", says("unusual"), Ok("1.0.0".to_owned()));
    assert!(matches!(
        fixture.row("pi", false).update,
        Update::Unknown(_)
    ));
}

#[test]
fn how_a_cli_was_installed_is_said_with_the_command_that_updates_it_the_same_way() {
    let fixture = Fixture::new();
    let own = fixture.install("home/.codex/bin", "codex");
    fixture.capture.says("codex --version", "1.0.0");
    fixture.latest.says(Harness::Codex, "1.0.1");
    let row = fixture.row("codex", false);
    assert_eq!(
        row.distribution,
        Distribution::Named("Codex's installer".to_owned())
    );
    let json = json_of(&row);
    assert_eq!(json["distribution"], "Codex's installer");
    assert_eq!(
        json["update"]["command"],
        format!("{} update", own.display())
    );
    // Found on PATH nowhere it is recognized: no way to update it is known.
    let fixture = look("codex", "bin", says("1.0.0"), Ok("1.0.1".to_owned()));
    let row = fixture.row("codex", false);
    assert_eq!(row.distribution, Distribution::Unrecognized);
    let json = json_of(&row);
    assert_eq!(json["distribution"], json!(null));
    assert_eq!(json["update"]["command"], json!(null));
}

#[test]
fn devin_is_ready_only_from_its_minimum_and_no_other_harness_has_a_setup() {
    for (said, ready) in [
        ("3000.6.14", false),
        ("3000.10.20", false),
        ("3000.10.21", true),
        ("devin 3000.11.0", true),
        ("unusual", false),
    ] {
        let fixture = look("devin", "bin", says(said), Ok("3000.10.21".to_owned()));
        let row = fixture.row("devin", false);
        let expected = if ready {
            Setup::Ready
        } else {
            Setup::UpdateRequired {
                reason:
                    "Devin 3000.10.21 or newer is required. Update Devin before opening a pane."
                        .to_owned(),
            }
        };
        assert_eq!(row.setup, Some(expected), "{said}");
    }
    let fixture = look("codex", "bin", says("1.0.0"), Ok("1.0.0".to_owned()));
    assert_eq!(fixture.row("codex", false).setup, None);
}

#[test]
fn a_row_says_its_fields_in_the_order_node_builds_them() {
    let fixture = look(
        "devin",
        "bin",
        says("3000.10.21"),
        Ok("3000.10.21".to_owned()),
    );
    let json = json_of(&fixture.row("devin", false));
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "id",
            "path",
            "installed",
            "checkedAt",
            "version",
            "update",
            "setup",
            "distribution"
        ]
    );
}

#[test]
fn pi_and_opencode_are_given_the_extension_made_for_them_and_the_others_none() {
    let fixture = Fixture::new();
    for (command, version) in [("pi", "0.1.0"), ("opencode", "0.2.0"), ("codex", "0.3.0")] {
        fixture.install("bin", command);
        fixture
            .capture
            .says(&format!("{command} --version"), version);
        fixture
            .latest
            .says(Harness::from_name(command).unwrap(), version);
    }
    for (command, folder) in [("pi", "pi"), ("opencode", "opencode")] {
        let json = json_of(&fixture.row(command, false));
        let extension = &json["extension"];
        assert_eq!(extension["state"], "installed-unverified", "{command}");
        let path = extension["path"].as_str().unwrap().replace('\\', "/");
        assert!(
            path.contains(&format!("/consensflow/extensions/{folder}/")),
            "{path}"
        );
        let keys: Vec<&String> = extension.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["state", "path"],
            "{command}: OpenCode's config is no part of it"
        );
        assert_eq!(
            json.as_object().unwrap().keys().next_back().unwrap(),
            "extension"
        );
    }
    assert!(json_of(&fixture.row("codex", false))
        .get("extension")
        .is_none());
}

#[test]
fn every_harness_is_looked_at_at_once_each_asked_its_own_version_and_release() {
    let fixture = Fixture::new();
    for command in ["devin", "claude", "codex", "opencode", "pi"] {
        fixture.install("bin", command);
        fixture
            .capture
            .says(&format!("{command} --version"), "5.0.0");
        fixture
            .latest
            .says(Harness::from_name(command).unwrap(), "5.0.1");
    }
    let rows = fixture.check(None, false);
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["devin", "claude", "codex", "opencode", "pi"]);
    assert!(rows
        .iter()
        .all(|row| matches!(row.update, Update::Available(_))));
    let mut ran: Vec<String> = fixture
        .capture
        .take_ran()
        .iter()
        .map(|(program, _)| crate::testing::named(program))
        .collect();
    ran.sort();
    assert_eq!(
        ran,
        [
            "claude --version",
            "codex --version",
            "devin --version",
            "opencode --version",
            "pi --version"
        ]
    );
    let mut asked: Vec<&str> = fixture
        .latest
        .take_asked()
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    asked.sort_unstable();
    assert_eq!(asked, ["claude", "codex", "devin", "opencode", "pi"]);
    assert!(fixture.capture.unused().is_empty() && fixture.latest.unused().is_empty());
}
