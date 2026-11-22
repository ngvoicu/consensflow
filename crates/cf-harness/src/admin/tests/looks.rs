//! What a look at one harness says of it: its version, its release, how it
//! was installed, what Devin may be opened on, and the extension Pi loads.

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

/// The sentence a launch refuses an older Devin with.
const DEVIN_REFUSED: &str =
    "Devin 3000.10.21 or newer is required for complete worker replies. Update Devin before opening this pane.";

#[test]
fn a3_review_devin_setup_is_the_launch_s_reading_beside_the_proved_release() {
    let release = crate::devin::declared::NEWEST_TESTED;
    let mut setups = Vec::new();
    for separator in ["\n", "\r\n"] {
        let said =
            format!("workspace launcher 1.0.0{separator}devin {release} (053434014dbd){separator}");
        let fixture = look("devin", "bin", says(&said), Ok(release.to_owned()));
        let row = fixture.row("devin", false);
        assert_eq!(row.version.value(), Some(release));
        assert_eq!(json_of(&row)["tested"]["release"], release);
        assert_eq!(json_of(&row)["update"]["state"], "current");
        setups.push(row.setup.clone());
    }
    // The launch's own check reads the answer as a launch does, wrapper first.
    let required = Some(Setup::UpdateRequired {
        reason: DEVIN_REFUSED.to_owned(),
    });
    assert_eq!(setups, vec![required.clone(), required]);

    // The inverse: the launch's reading is the answer's first number, not the proved release.
    let fixture = look(
        "devin",
        "bin",
        says("workspace launcher 9999.0.0\ndevin 0.0.1 (053434014dbd)\n"),
        Ok("0.0.2".to_owned()),
    );
    let row = fixture.row("devin", false);
    assert_eq!(row.setup, Some(Setup::Ready));
    assert_eq!(row.version.value(), Some("0.0.1"));
    assert_eq!(json_of(&row)["tested"]["release"], "0.0.1");
    assert_eq!(json_of(&row)["update"]["state"], "available");
}

#[test]
fn a3_review_muse_setup_is_the_launch_s_reading_of_its_own_line() {
    let said = "workspace launcher 1.0.0\r\nMuse Code 1.4.14 (1.4.14-R7309.1)\r\n";
    let fixture = look("muse", "bin", says(said), Ok("1.4.14-R7309.1".to_owned()));
    let row = fixture.row("muse", false);
    assert_eq!(row.version.value(), Some("1.4.14-R7309.1"));
    assert_eq!(json_of(&row)["tested"]["release"], "1.4.14-R7309.1");
    assert_eq!(json_of(&row)["update"]["state"], "current");
    // The launch reads Muse's own line, so the wrapper's number before it decides nothing.
    assert_eq!(row.setup, Some(Setup::Ready));

    let said = "workspace launcher 9999.0.0\nMuse Code 1.4.12 (1.4.12-R6000)\n";
    let fixture = look("muse", "bin", says(said), Ok("1.4.13-R6996.1".to_owned()));
    let row = fixture.row("muse", false);
    assert_eq!(row.version.value(), Some("1.4.12-R6000"));
    assert_eq!(json_of(&row)["update"]["state"], "available");
    assert_eq!(
        row.setup,
        Some(Setup::UpdateRequired {
            reason: crate::muse::version::required()
        })
    );
}

#[test]
fn a3_review_muse_unreadable_build_names_no_version_as_the_launch_reads_none() {
    let newest = crate::muse::declared::NEWEST_TESTED;
    let semver = newest.split_once("-R").unwrap().0;
    let mut rows = Vec::new();
    for build in ["4294967296.1", "4294967295.4294967296"] {
        let said = format!("Muse Code {semver} ({semver}-R{build})");
        let fixture = look("muse", "bin", says(&said), Ok(newest.to_owned()));
        let row = json_of(&fixture.row("muse", false));
        rows.push(json!({
            "version": row["version"]["value"],
            "release": row["tested"]["release"],
            "beyond": row["tested"]["beyond"],
            "update": row["update"]["state"],
        }));
    }
    // The launch's reader names no version of a build it cannot read, and opens: the page
    // shows what the launch knows, and compares nothing.
    let fallback = json!({"version": null, "release": null, "beyond": false, "update": "unknown"});
    assert_eq!(rows, vec![fallback.clone(), fallback]);

    for line in [
        "Muse Code 4294967296.4.23 (4294967296.4.23-R1.1)",
        "Muse Code 1.4294967296.23 (1.4294967296.23-R1.1)",
        "Muse Code 1.4.4294967296 (1.4.4294967296-R1.1)",
    ] {
        assert!(crate::muse::declared::release(line).is_none(), "{line}");
    }

    let valid = format!("{semver}-R4294967295.4294967295");
    let said = format!("Muse Code {semver} ({valid})");
    let fixture = look("muse", "bin", says(&said), Ok(valid.clone()));
    let row = json_of(&fixture.row("muse", false));
    assert_eq!(row["tested"]["release"], valid);
    assert_eq!(row["version"]["value"], valid);
    assert_eq!(row["update"]["state"], "current");
    assert!(crate::muse::declared::pin_release(&format!("{semver}-R4294967296.1")).is_none());
}

#[test]
fn a3_review_claude_installer_round_trips_its_text_feed() {
    use crate::admin::{feed::release_version, release_source, Format};
    use cf_base::env::Env;

    let fixture = Fixture::in_env(|root| {
        let text = |folder: &str| root.join(folder).to_string_lossy().into_owned();
        Env::from_vars([
            ("HOME", text("home")),
            ("USERPROFILE", text("home")),
            ("PATH", text("home/.local/share/claude/versions")),
            ("CONSENSFLOW_HOME", text("consensflow")),
            ("CLAUDE_CONFIG_DIR", text("home/.claude")),
        ])
    });
    let executable = fixture.install("home/.local/share/claude/versions", "claude");
    let newest = crate::claude::declared::NEWEST_TESTED;
    let source = release_source(Harness::Claude, &executable.to_string_lossy(), &fixture.env);
    assert_eq!(source.format, Format::Text);
    // The installer's channel supplies plain text, not an npm JSON document.
    let body = format!("{newest}\r\n");
    let feed = release_version(source.format, &body);
    assert_eq!(feed.as_deref(), Ok(newest));
    fixture.capture.says(
        "claude --version",
        &format!("workspace launcher 1.0.0\n{newest} (Claude Code)\n"),
    );
    fixture.latest.says(Harness::Claude, &feed.unwrap());
    let row = json_of(&fixture.row("claude", false));
    assert_eq!(row["tested"]["newest"], newest);
    assert_eq!(row["tested"]["release"], newest);
    assert_eq!(row["tested"]["beyond"], false);
    assert_eq!(row["version"]["value"], newest);
    assert_eq!(row["update"]["state"], "current");
    assert!(row["update"]["source"]
        .as_str()
        .unwrap()
        .ends_with("/latest"));
}

#[test]
fn a3_every_installed_row_names_its_tested_range() {
    for command in ["claude", "codex", "pi", "devin", "muse"] {
        let fixture = look(command, "bin", says("unusual"), Ok("9.9.9".to_owned()));
        let row = json_of(&fixture.row(command, false));
        let (newest, oldest) = match command {
            "claude" => (
                crate::claude::declared::NEWEST_TESTED,
                Some(crate::claude::declared::OLDEST_TESTED.to_owned()),
            ),
            "codex" => (crate::codex::declared::NEWEST_TESTED, None),
            "pi" => (
                crate::pi::declared::NEWEST_TESTED,
                Some(crate::pi::declared::OLDEST_TESTED.to_owned()),
            ),
            "devin" => (
                crate::devin::declared::NEWEST_TESTED,
                Some(crate::devin::minimum_version()),
            ),
            "muse" => (
                crate::muse::declared::NEWEST_TESTED,
                Some(crate::muse::version::minimum()),
            ),
            _ => unreachable!(),
        };
        assert_eq!(row["tested"]["newest"], newest, "{command}");
        assert_eq!(row["tested"]["oldest"], json!(oldest), "{command}");
        assert_eq!(row["tested"]["release"], json!(null), "{command}");
        assert_eq!(row["tested"]["beyond"], false, "{command}");
    }
}

fn a3_release_cases(command: &str, own: &str, release: &str) {
    for (said, expected) in [
        (own.to_owned(), Some(release)),
        (
            format!("workspace launcher 1.0.0\r\n{own}\r\n"),
            Some(release),
        ),
        (format!("{own}\nworkspace launcher 1.0.0\n"), Some(release)),
        (format!("workspace launcher 1.0.0 {own}"), None),
        (format!(" {own}"), None),
        (format!("{own} trailing text"), None),
        (format!("{own}\n{own}"), None),
        ("workspace launcher 1.0.0".to_owned(), None),
    ] {
        let fixture = look(command, "bin", says(&said), Ok(release.to_owned()));
        let row = json_of(&fixture.row(command, false));
        assert_eq!(
            row["tested"]["release"],
            json!(expected),
            "{command}: {said:?}"
        );
    }
}

#[test]
fn a3_claude_release_requires_one_whole_native_line() {
    a3_release_cases("claude", "9999.0.2 (Claude Code)", "9999.0.2");
}

#[test]
fn a3_codex_release_requires_one_whole_native_line() {
    a3_release_cases("codex", "codex-cli 9999.0.2", "9999.0.2");
}

#[test]
fn a3_pi_release_requires_one_whole_native_line() {
    a3_release_cases("pi", "9999.0.2", "9999.0.2");
}

#[test]
fn a3_devin_release_requires_one_whole_native_line() {
    a3_release_cases("devin", "devin 9999.0.2 (053434014dbd)", "9999.0.2");
}

#[test]
fn a3_muse_release_requires_one_whole_native_line() {
    a3_release_cases(
        "muse",
        "Muse Code 9999.0.2 (9999.0.2-R7400.1)",
        "9999.0.2-R7400.1",
    );
    for invalid in [
        "Muse Code 1.4.14 (1.4.15-R7400.1)",
        "Muse Code 1.4.14 (1.4.14)",
        "Muse Code 1.4.14-beta (1.4.14-R7400.1)",
        "1.4.14-R7400.1",
    ] {
        let fixture = look(
            "muse",
            "bin",
            says(invalid),
            Ok("1.4.14-R7400.1".to_owned()),
        );
        let row = json_of(&fixture.row("muse", false));
        assert_eq!(row["tested"]["release"], json!(null), "{invalid}");
        assert_eq!(row["tested"]["beyond"], false, "{invalid}");
    }
}

#[test]
fn a3_release_round_trip_keeps_the_feed_and_display_in_agreement() {
    use crate::admin::feed::release_version;
    use crate::admin::source::Format;

    for (command, own, release) in [
        ("claude", "9999.0.2 (Claude Code)", "9999.0.2"),
        ("codex", "codex-cli 9999.0.2", "9999.0.2"),
        ("pi", "9999.0.2", "9999.0.2"),
        ("devin", "devin 9999.0.2 (053434014dbd)", "9999.0.2"),
        (
            "muse",
            "Muse Code 9999.0.2 (9999.0.2-R7309.1)",
            "9999.0.2-R7309.1",
        ),
    ] {
        let feed = release_version(Format::Npm, &json!({"version": release}).to_string()).unwrap();
        let said = format!("workspace launcher 1.0.0\n{own}\n");
        let fixture = look(command, "bin", says(&said), Ok(feed.clone()));
        let row = json_of(&fixture.row(command, false));
        assert_eq!(row["tested"]["release"], feed, "{command}");
        assert_eq!(row["version"]["value"], feed, "{command}");
        assert_eq!(row["update"]["state"], "current", "{command}: {row}");
    }

    // Use each actual declaration as the feed and CLI release. A future bump
    // changes neither this test nor the scenario goldens.
    for command in ["claude", "codex", "pi", "devin", "muse"] {
        let newest = json_of(&Fixture::new().row(command, false))["tested"]["newest"]
            .as_str()
            .unwrap()
            .to_owned();
        let own = match command {
            "claude" => format!("{newest} (Claude Code)"),
            "codex" => format!("codex-cli {newest}"),
            "pi" => newest.clone(),
            "devin" => format!("devin {newest} (053434014dbd)"),
            "muse" => format!(
                "Muse Code {} ({newest})",
                newest.split_once("-R").unwrap().0
            ),
            _ => unreachable!(),
        };
        let feed = release_version(Format::Npm, &json!({"version": newest}).to_string()).unwrap();
        let fixture = look(command, "bin", says(&own), Ok(feed.clone()));
        let row = json_of(&fixture.row(command, false));
        assert_eq!(row["tested"]["newest"], feed, "{command}");
        assert_eq!(row["tested"]["release"], feed, "{command}");
        assert_eq!(row["tested"]["beyond"], false, "{command}");
        assert_eq!(row["version"]["value"], feed, "{command}");
        assert_eq!(row["update"]["state"], "current", "{command}");
    }
}

#[test]
fn a3_two_muse_builds_of_one_semver_are_distinct_releases() {
    for (build, feed, state) in [
        ("R7309.1", "1.4.14-R7400.1", "available"),
        ("R7400.1", "1.4.14-R7309.1", "current"),
    ] {
        let said = format!("Muse Code 1.4.14 (1.4.14-{build})");
        let fixture = look("muse", "bin", says(&said), Ok(feed.to_owned()));
        let row = json_of(&fixture.row("muse", false));
        assert_eq!(row["update"]["state"], state, "{said}: {row}");
    }
}

#[test]
fn a3_threshold_and_wrapper_advice_use_the_proved_release() {
    for (own, expected) in [("0.0.1", false), ("9999.0.2", true)] {
        let said = format!("workspace launcher 1.0.0\n{own} (Claude Code)");
        let fixture = look("claude", "bin", says(&said), Ok(own.to_owned()));
        let row = json_of(&fixture.row("claude", false));
        assert_eq!(row["tested"]["beyond"], expected, "{row}");
        assert_eq!(row["version"]["value"], own, "{row}");
        assert_eq!(row["update"]["state"], "current", "{row}");
    }
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
fn devin_is_ready_only_from_its_minimum_and_codex_is_ready_with_its_queue() {
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
                reason: DEVIN_REFUSED.to_owned(),
            }
        };
        assert_eq!(row.setup, Some(expected), "{said}");
    }
    let fixture = look("codex", "bin", says("1.0.0"), Ok("1.0.0".to_owned()));
    fixture
        .capture
        .says("codex queue --help", "--thread <ID> --message <TEXT>");
    assert_eq!(fixture.row("codex", false).setup, Some(Setup::Ready));
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
            "tested",
            "setup",
            "distribution"
        ]
    );
}

#[test]
fn pi_is_given_the_extension_made_for_it_and_the_others_none() {
    let fixture = Fixture::new();
    for (command, version) in [("pi", "0.1.0"), ("codex", "0.3.0")] {
        fixture.install("bin", command);
        fixture
            .capture
            .says(&format!("{command} --version"), version);
        fixture
            .latest
            .says(Harness::from_name(command).unwrap(), version);
    }
    let json = json_of(&fixture.row("pi", false));
    let extension = &json["extension"];
    assert_eq!(extension["state"], "installed-unverified");
    let path = extension["path"].as_str().unwrap().replace('\\', "/");
    assert!(path.contains("/consensflow/extensions/pi/"), "{path}");
    let keys: Vec<&String> = extension.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["state", "path"]);
    assert_eq!(
        json.as_object().unwrap().keys().next_back().unwrap(),
        "extension"
    );
    assert!(json_of(&fixture.row("codex", false))
        .get("extension")
        .is_none());
}

#[test]
fn every_harness_is_looked_at_at_once_each_asked_its_own_version_and_release() {
    let fixture = Fixture::new();
    for command in ["devin", "claude", "codex", "pi"] {
        fixture.install("bin", command);
        fixture
            .capture
            .says(&format!("{command} --version"), "5.0.0");
        if command == "codex" {
            fixture
                .capture
                .says("codex queue --help", "--thread --message");
        }
        fixture
            .latest
            .says(Harness::from_name(command).unwrap(), "5.0.1");
    }
    // Muse says its version in the file beside its launcher, and runs nothing; its
    // update is offered as the others' are, with no command of ours to run it.
    let muse = fixture.install("bin", "muse");
    std::fs::write(muse.with_file_name(".muse-version"), "1.4.13-R1\n").unwrap();
    fixture.install("bin", "muse-bin-1.4.13-R1");
    fixture.latest.says(Harness::Muse, "1.4.14-R1");
    let rows = fixture.check(None, false);
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["devin", "claude", "codex", "pi", "muse"]);
    assert!(rows
        .iter()
        .all(|row| matches!(row.update, Update::Available(_))));
    let Update::Available(release) = &rows[4].update else {
        unreachable!()
    };
    assert_eq!(release.command, None, "Muse's own launcher updates it");
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
            "codex queue --help",
            "devin --version",
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
    assert_eq!(asked, ["claude", "codex", "devin", "muse", "pi"]);
    assert!(fixture.capture.unused().is_empty() && fixture.latest.unused().is_empty());
}

#[test]
fn muses_version_is_the_line_beside_its_launcher_with_nothing_run_else_a_quiet_version() {
    // Its installer's launcher, which names the binary it runs beside it.
    let fixture = Fixture::new();
    let launcher = fixture.install("home/.local/bin", "muse");
    let line = launcher.parent().unwrap().join(".muse-version");
    std::fs::write(&line, "1.4.13-R6996.1\n").unwrap();
    fixture.install("home/.local/bin", "muse-bin-1.4.13-R6996.1");
    fixture.latest.says(Harness::Muse, "1.4.13-R6996.2");
    let row = fixture.row("muse", false);
    assert_eq!(
        row.version,
        Version::Checked {
            value: "1.4.13-R6996.1".to_owned()
        }
    );
    assert_eq!(json_of(&row)["tested"]["release"], "1.4.13-R6996.1");
    assert!(
        fixture.capture.take_ran().is_empty(),
        "the launcher's own --version checks for an update"
    );
    assert_eq!(row.setup, Some(Setup::Ready));
    assert_eq!(
        row.distribution,
        Distribution::Named("Muse Code's installer".to_owned())
    );
    // Its update is offered, and ConsensFlow runs none: Muse's launcher updates itself.
    let Update::Available(release) = &row.update else {
        panic!("{:?}", row.update);
    };
    assert_eq!(release.value, "1.4.13-R6996.2");
    assert_eq!(release.command, None);
    // An older one is to be updated, and the release is one.
    std::fs::write(&line, "1.4.12-R6000\n").unwrap();
    fixture.install("home/.local/bin", "muse-bin-1.4.12-R6000");
    fixture.latest.says(Harness::Muse, "1.4.13-R6996.1");
    let row = fixture.row("muse", true);
    assert_eq!(
        row.setup,
        Some(Setup::UpdateRequired {
            reason:
                "Muse Code 1.4.13 or newer is required. Update Muse Code before opening a pane."
                    .to_owned()
        })
    );
    assert!(
        matches!(row.update, Update::Available(_)),
        "{:?}",
        row.update
    );
    // With no line beside it, it is asked, with its update and its login off.
    std::fs::remove_file(&line).unwrap();
    fixture.capture.answer(
        "muse --version",
        [says("Muse Code 1.4.13 (1.4.13-R6996.1)")],
    );
    fixture.latest.says(Harness::Muse, "1.4.13-R6996.1");
    let row = fixture.row("muse", true);
    assert_eq!(
        row.version,
        Version::Checked {
            value: "1.4.13-R6996.1".to_owned()
        }
    );
    assert_eq!(row.setup, Some(Setup::Ready));
    let ran = fixture.capture.take_ran();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].0.env.text("MUSE_NO_AUTO_UPDATE"), Some("1"));
    assert_eq!(ran[0].0.env.text("MUSE_LOGIN"), Some("0"));
    assert_eq!(ran[0].0.env.text("HOME"), fixture.env.text("HOME"));
}
