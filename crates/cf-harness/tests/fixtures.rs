//! The Claude transcripts the tests of this crate read (`tests/fixtures/claude`)
//! are of live runs, made fixtures by `cargo xtask scrub-transcript`, and the Muse
//! session logs (`tests/fixtures/muse`) are of runs on Muse's fake model, made
//! fixtures by `cargo xtask scrub-muse-session`: no folder of a user or of the
//! machine's temporary folder, no address and no id of an account may be left in
//! one, whatever else a person adds to a folder by hand. How the tools scrub is held
//! by their own tests (`tools/xtask/src/scrub_transcript`,
//! `tools/xtask/src/scrub_muse_session`); this holds what they were made for.

use std::fs;
use std::path::PathBuf;

use regex::Regex;
use serde_json::Value;

/// What a transcript of a live run has in it that is personal, each with a sample:
/// a folder of a user's, on a Mac and on Linux, a Mac's temporary folder (a user's
/// own, with `/private` before it or not), Muse's runtime socket (its folder names the
/// user's uid, its file a profile's id), an address, and the ids of an account's
/// bridge sessions. (A word is what JavaScript's `\w` made it: ASCII.)
const PERSONAL: [(&str, &str); 7] = [
    (r"/Users/", "/Users/jane/Projects/app"),
    (r"/home/", "/home/jane/app"),
    (
        r"/var/folders/",
        "/private/var/folders/5f/cy8ywl/T/muse-rec-1/work",
    ),
    (
        r"/tmp/tbh-",
        "/private/tmp/tbh-501-rt/muse/ms-2ee2d561e12e.sock",
    ),
    (
        r"[A-Za-z0-9_.+-]+@[A-Za-z0-9_-]+\.[A-Za-z0-9_]+",
        "jane@example.com",
    ),
    (r"cse_0[A-Za-z0-9_]{6,}", "cse_01Abcdefg"),
    (r"session_0[A-Za-z0-9_]{6,}", "session_01Abcdefg"),
];

fn folder(harness: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(harness)
}

/// A value-based check for identifiers, including labels inside log messages.
struct IdentityCheck {
    email: Regex,
    labelled: Regex,
}

impl IdentityCheck {
    fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            email: Regex::new(PERSONAL[4].0)?,
            labelled: Regex::new(
                r#"(?i)\b(?:[a-z0-9_-]*(?:account|organization|organisation|user|workspace)[_-]?(?:uuid|identifier|scope[_-]?digest|digest|hash|id)|auid|euid|uid)["']?\s*[:=]\s*["']?(\[scrubbed(?:: \d+ bytes)?\]|null|[^"'\s,}\]]+)"#,
            )?,
        })
    }

    fn normalized(name: &str) -> String {
        name.chars()
            .filter(|c| !matches!(c, '_' | '-' | ' '))
            .flat_map(char::to_lowercase)
            .collect()
    }

    fn field(name: &str) -> bool {
        let name = Self::normalized(name);
        matches!(name.as_str(), "uid" | "auid" | "euid")
            || [
                "account",
                "organization",
                "organisation",
                "user",
                "workspace",
            ]
            .iter()
            .any(|entity| {
                ["id", "uuid", "identifier", "digest", "scopedigest", "hash"]
                    .iter()
                    .any(|suffix| name.ends_with(&format!("{entity}{suffix}")))
            })
            || ["email", "emailaddress", "mailaddress"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
    }

    fn scrubbed(value: &str) -> bool {
        value == "[scrubbed]"
            || value
                .strip_prefix("[scrubbed: ")
                .and_then(|rest| rest.strip_suffix(" bytes]"))
                .is_some_and(|length| {
                    !length.is_empty() && length.chars().all(|c| c.is_ascii_digit())
                })
    }

    fn text(&self, text: &str) -> bool {
        self.email.is_match(text)
            || self.labelled.captures_iter(text).any(|found| {
                let Some(value) = found.get(1) else {
                    return true;
                };
                let terminated = text[value.end()..].chars().next().is_none_or(|next| {
                    next.is_whitespace() || matches!(next, ',' | '}' | ']' | '"' | '\'' | ';' | ')')
                });
                !terminated
                    || (!Self::scrubbed(value.as_str())
                        && !value.as_str().eq_ignore_ascii_case("null"))
            })
    }

    fn json(&self, value: &Value) -> bool {
        self.json_in(value, false)
    }

    fn json_in(&self, value: &Value, identity_object: bool) -> bool {
        match value {
            Value::Object(fields) => fields.iter().any(|(name, value)| {
                let has_value =
                    !value.is_null() && value != "" && !value.as_str().is_some_and(Self::scrubbed);
                let normalized = Self::normalized(name);
                let object_id = identity_object
                    && matches!(
                        normalized.as_str(),
                        "id" | "uuid" | "identifier" | "digest" | "hash"
                    );
                let nested_identity = [
                    "account",
                    "organization",
                    "organisation",
                    "user",
                    "workspace",
                ]
                .iter()
                .any(|entity| {
                    normalized.ends_with(entity) || normalized.ends_with(&format!("{entity}s"))
                });
                self.text(name)
                    || ((Self::field(name) || object_id) && has_value)
                    || self.json_in(value, nested_identity)
            }),
            Value::Array(items) => items.iter().any(|item| self.json_in(item, identity_object)),
            Value::String(text) => self.text(text),
            _ => false,
        }
    }
}

/// Every fixture directory in the workspace, not just the recorded harness logs.
fn fixture_files(folder: &std::path::Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fixture_files(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

#[test]
fn identity_check_finds_values_but_accepts_placeholders() {
    let check = IdentityCheck::new().unwrap();
    let clean = serde_json::json!({"nested": [{"ownerAccountUuid": "[scrubbed]", "userID": null, "workspace_scope_digest": "[scrubbed: 64 bytes]"}], "message": "auid=[scrubbed]"});
    assert!(!check.json(&clean));
    for value in [
        serde_json::json!({"accountUuid": "made-up"}),
        serde_json::json!({"nested": [{"ownerOrganizationUuid": "made-up"}]}),
        serde_json::json!({"workspace_id": "made-up"}),
        serde_json::json!({"workspace_scope_digest": "made-up"}),
        serde_json::json!({"owner": {"account": {"id": "made-up"}}}),
        serde_json::json!({"workspaces": [{"uuid": "made-up"}]}),
        serde_json::json!({"userID": 501}),
        serde_json::json!({"message": "euid=501"}),
        serde_json::json!({"message": "euid=[scrubbed]made-up"}),
        serde_json::json!({"message": "person@example.test"}),
        serde_json::json!({"person@example.test": "contact"}),
        serde_json::from_str::<Value>(r#"{"message":"person\u0040example.test"}"#).unwrap(),
    ] {
        assert!(check.json(&value));
    }
    assert!(!check.json(&serde_json::json!({"accountUuidSuffix": "kept"})));
}

#[test]
fn all_workspace_fixtures_hold_no_account_user_workspace_ids_or_email() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut files = Vec::new();
    for entry in fs::read_dir(crates).unwrap() {
        let folder = entry.unwrap().path().join("tests/fixtures");
        if folder.is_dir() {
            fixture_files(&folder, &mut files).unwrap();
        }
    }
    files.sort();
    assert!(!files.is_empty(), "no fixture files were checked");
    let check = IdentityCheck::new().unwrap();
    for path in files {
        let data = fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&data);
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert!(!check.json(&value), "identity value in {}", path.display());
            }
            Some("jsonl" | "ndjson") => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    let value: Value = serde_json::from_str(line).unwrap();
                    assert!(!check.json(&value), "identity value in {}", path.display());
                }
            }
            _ => assert!(!check.text(&text), "identity value in {}", path.display()),
        }
    }
}

#[test]
fn the_claude_fixtures_hold_no_folder_no_address_and_no_id_of_an_account() {
    let folder = folder("claude");
    let mut names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".jsonl"))
        .collect();
    names.sort();
    assert!(names.len() >= 2, "the fixtures are there: {names:?}");
    for name in names {
        let text = fs::read_to_string(folder.join(&name)).unwrap();
        for (pattern, _) in PERSONAL {
            let found = Regex::new(pattern).unwrap();
            assert!(!found.is_match(&text), "{name} matches {pattern}");
        }
        for line in text.lines().filter(|line| !line.is_empty()) {
            let record: Value = serde_json::from_str(line).unwrap();
            if let Some(session) = record.get("sessionId") {
                assert_eq!(session, "$SESSION", "{name}");
            }
        }
    }
}

#[test]
fn the_muse_fixtures_hold_no_folder_no_address_and_no_id_of_an_account() {
    let folder = folder("muse");
    // The echo recordings, and the live ones beside them (`live/`).
    let mut names: Vec<String> = ["", "live"]
        .into_iter()
        .flat_map(|sub| {
            fs::read_dir(folder.join(sub)).unwrap().map(move |entry| {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                if sub.is_empty() {
                    name
                } else {
                    format!("{sub}/{name}")
                }
            })
        })
        .filter(|name| {
            name.ends_with(".jsonl") || name.ends_with(".json") || name.ends_with(".sql")
        })
        .collect();
    names.sort();
    assert!(names.len() >= 11, "the fixtures are there: {names:?}");
    assert!(
        names.iter().any(|name| name.starts_with("live/")),
        "{names:?}"
    );
    for name in names {
        let text = fs::read_to_string(folder.join(&name)).unwrap();
        for (pattern, _) in PERSONAL {
            let found = Regex::new(pattern).unwrap();
            assert!(!found.is_match(&text), "{name} matches {pattern}");
        }
    }
}

#[test]
fn the_history_items_fixtures_hold_no_folder_no_address_and_no_id_of_an_account() {
    // The live chiefs' records the readers' history is held to (T-728), of Claude Code,
    // Codex, Pi and Muse Code.
    let folder = folder("history-items");
    for name in ["claude.jsonl", "codex.jsonl", "pi.jsonl", "muse.jsonl"] {
        let text = fs::read_to_string(folder.join(name)).unwrap();
        for (pattern, _) in PERSONAL {
            let found = Regex::new(pattern).unwrap();
            assert!(!found.is_match(&text), "{name} matches {pattern}");
        }
    }
}

#[test]
fn what_the_check_looks_for_is_what_a_transcript_that_was_not_scrubbed_has() {
    // A check is none if what it looks for is found nowhere.
    for (pattern, sample) in PERSONAL {
        assert!(
            Regex::new(pattern).unwrap().is_match(sample),
            "{pattern} does not find {sample}"
        );
    }
}
