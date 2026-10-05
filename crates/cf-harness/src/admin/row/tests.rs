//! A row and an update's outcome, written in the order and with the keys
//! `src/harness-admin.js` builds them: absent where it left a key out, null
//! where it wrote one.

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
        r#"{"id":"codex","path":"/bin/codex","installed":true,"checkedAt":7,"version":{"state":"checked","value":"1.0.0"},"update":{"state":"current","value":"1.0.1","source":"https://feed","command":null}}"#
    );
    found.distribution = Distribution::Unrecognized;
    assert!(text(&found).ends_with(r#""distribution":null}"#));
    found.distribution = Distribution::Named("Homebrew".to_owned());
    found.setup = Some(Setup::Ready);
    found.extension = Some(Extension::unverified("/ext.mjs".to_owned()));
    assert!(
        text(&found).ends_with(
            r#""update":{"state":"current","value":"1.0.1","source":"https://feed","command":null},"setup":{"state":"ready"},"distribution":"Homebrew","extension":{"state":"installed-unverified","path":"/ext.mjs"}}"#
        ),
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
        said(Extension::of_opencode(
            opencode::Extension::InstalledUnverified {
                path: "/p".to_owned(),
                config: "/tui.json".to_owned()
            }
        )),
        json!({"state": "installed-unverified", "path": "/p"}),
        "OpenCode's settings file is the launch's, not the row's"
    );
    assert_eq!(
        serde_json::to_string(&Extension::of_opencode(opencode::Extension::Error {
            reason: "EACCES".to_owned()
        }))
        .unwrap(),
        r#"{"state":"error","path":null,"reason":"EACCES"}"#
    );
}

#[test]
fn an_outcome_keeps_its_reason_before_the_row_and_an_unsupported_one_has_no_command() {
    let harness = Rc::new(row(Version::NotInstalled, Update::NotChecked));
    let ran = Outcome::Ran {
        id: Harness::Codex,
        state: Ended::Failed,
        before: Some("1.0.0".to_owned()),
        after: None,
        command: "codex update".to_owned(),
        output: "no network".to_owned(),
        reason: Some("Command failed".to_owned()),
        harness: Rc::clone(&harness),
    };
    let json = serde_json::to_value(&ran).unwrap();
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["id", "state", "before", "after", "command", "output", "reason", "harness"]
    );
    assert_eq!(
        (&json["state"], &json["after"]),
        (&json!("failed"), &json!(null))
    );
    let unsupported = Outcome::Unsupported {
        id: Harness::Pi,
        state: Ended::Unsupported,
        reason: "r".to_owned(),
        harness,
    };
    let json = serde_json::to_value(&unsupported).unwrap();
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["id", "state", "reason", "harness"]);
    assert_eq!(json["state"], "unsupported");
    let not_failed = Outcome::Ran {
        id: Harness::Codex,
        state: Ended::Updated,
        before: None,
        after: None,
        command: String::new(),
        output: String::new(),
        reason: None,
        harness: Rc::new(row(Version::NotInstalled, Update::NotChecked)),
    };
    assert!(serde_json::to_value(&not_failed)
        .unwrap()
        .get("reason")
        .is_none());
}
