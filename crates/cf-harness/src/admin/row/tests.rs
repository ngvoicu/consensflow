//! A row and an update's outcome, written in the order and with the keys Node's
//! admin built them: absent where it left a key out, null where it wrote one.

use serde_json::json;

use super::*;

fn row(version: Version, update: Update) -> Row {
    Row {
        id: Harness::Codex,
        path: Some("/bin/codex".to_owned()),
        installed: true,
        checked_at: 7,
        version,
        update,
        tested: Tested::of(Harness::Codex, None),
        setup: None,
        distribution: Distribution::Unlooked,
        extension: None,
    }
}

fn release() -> Release {
    Release {
        value: "1.0.1".to_owned(),
        source: "https://feed".to_owned(),
        command: None,
    }
}

#[test]
fn a3_threshold_takes_the_declared_bound_and_compares_muse_builds() {
    let tested =
        |harness, newest, release| Tested::with_newest(harness, newest, None, Some(release));
    assert!(tested(Harness::Claude, "98.1.326", "98.1.330").beyond);
    assert!(!tested(Harness::Claude, "98.1.330", "98.1.330").beyond);
    assert!(tested(Harness::Muse, "1.4.14-R7309.1", "1.4.14-R7400.1").beyond);
    assert!(!tested(Harness::Muse, "1.4.14-R7400.1", "1.4.14-R7309.1").beyond);
}

#[test]
fn every_state_of_a_version_and_an_update_is_written_as_its_word_with_its_fields() {
    for (version, text) in [
        (Version::NotInstalled, r#"{"state":"not-installed"}"#),
        (
            Version::Checked {
                value: "1.0.0".to_owned(),
            },
            r#"{"state":"checked","value":"1.0.0"}"#,
        ),
        (
            Version::Unknown {
                reason: "r".to_owned(),
            },
            r#"{"state":"unknown","reason":"r"}"#,
        ),
        (
            Version::Error {
                reason: "r".to_owned(),
            },
            r#"{"state":"error","reason":"r"}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&version).unwrap(), text);
    }
    let said = |update: Update| serde_json::to_string(&update).unwrap();
    assert_eq!(said(Update::NotChecked), r#"{"state":"not-checked"}"#);
    for (update, state) in [
        (Update::Unknown(release()), "unknown"),
        (Update::Available(release()), "available"),
        (Update::Current(release()), "current"),
    ] {
        assert_eq!(
            said(update),
            format!(
                r#"{{"state":"{state}","value":"1.0.1","source":"https://feed","command":null}}"#
            )
        );
    }
    assert_eq!(
        said(Update::Error {
            reason: "offline".to_owned(),
            command: Some("brew upgrade codex".to_owned())
        }),
        r#"{"state":"error","reason":"offline","command":"brew upgrade codex"}"#
    );
    assert_eq!(
        serde_json::to_string(&Setup::UpdateRequired {
            reason: "r".to_owned()
        })
        .unwrap(),
        r#"{"state":"update-required","reason":"r"}"#
    );
    assert_eq!(
        serde_json::to_string(&Setup::Ready).unwrap(),
        r#"{"state":"ready"}"#
    );
}

#[test]
fn a_row_leaves_out_what_was_never_looked_at_and_writes_null_for_what_was_not_found() {
    let mut found = row(
        Version::Checked {
            value: "1.0.0".to_owned(),
        },
        Update::Current(release()),
    );
    let text = |row: &Row| serde_json::to_string(row).unwrap();
    assert_eq!(
        text(&found),
        format!(
            r#"{{"id":"codex","path":"/bin/codex","installed":true,"checkedAt":7,"version":{{"state":"checked","value":"1.0.0"}},"update":{{"state":"current","value":"1.0.1","source":"https://feed","command":null}},"tested":{}}}"#,
            serde_json::to_string(&found.tested).unwrap()
        )
    );
    found.distribution = Distribution::Unrecognized;
    assert!(text(&found).ends_with(r#""distribution":null}"#));
    found.distribution = Distribution::Named("Homebrew".to_owned());
    found.setup = Some(Setup::Ready);
    found.extension = Some(Extension::unverified("/ext.mjs".to_owned()));
    assert!(
        text(&found).ends_with(&format!(
            r#""update":{{"state":"current","value":"1.0.1","source":"https://feed","command":null}},"tested":{},"setup":{{"state":"ready"}},"distribution":"Homebrew","extension":{{"state":"installed-unverified","path":"/ext.mjs"}}}}"#,
            serde_json::to_string(&found.tested).unwrap()
        )),
        "{}",
        text(&found)
    );
}

#[test]
fn an_extension_is_its_state_and_its_path_and_the_reason_when_it_failed() {
    let said = |extension: Extension| serde_json::to_value(extension).unwrap();
    assert_eq!(
        said(Extension::of_pi(pi::Extension::NotInstalled)),
        json!({"state": "not-installed", "path": null})
    );
    assert_eq!(
        said(Extension::of_pi(pi::Extension::InstalledUnverified {
            path: "/p".to_owned()
        })),
        json!({"state": "installed-unverified", "path": "/p"})
    );
    assert_eq!(
        serde_json::to_string(&Extension::of_pi(pi::Extension::Error {
            reason: "EACCES".to_owned()
        }))
        .unwrap(),
        r#"{"state":"error","path":null,"reason":"EACCES"}"#
    );
}
