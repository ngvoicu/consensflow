//! The Claude transcripts the tests of this crate read (`tests/fixtures/claude`)
//! are of live runs, made fixtures by `cargo xtask scrub-transcript`, and the Muse
//! session logs (`tests/fixtures/muse`) are of runs on Muse's fake model, made
//! fixtures by `cargo xtask scrub-muse-session`: no folder of a user or of the
//! machine's temporary folder, no address and no id of an account may be left in
//! one, whatever else a person adds to a folder by hand. How the tools scrub is held
//! by their own tests (`tools/xtask/src/scrub_transcript`,
//! `tools/xtask/src/scrub_muse_session`); this holds what they were made for.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

use flate2::read::MultiGzDecoder;
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

/// What a screen recording's redraws leave of the owner's folders even when a path is cut
/// short (`/Users/` and the first letters of a name): the owner's home name, the repository's
/// namespace and the kit's kept folders. A recording of any harness's window holds none of them.
const FRAGMENTS: [(&str, &str); 6] = [
    // The patterns and their samples are spelled in parts, so that this file holds none of the
    // names it looks for.
    (r"(?i)gabr[i]el", concat!("/Users/gab", "riel...")),
    (r"(?i)voic[u]", concat!("...ngvoi", "cu/consens...")),
    (r"(?i)kit-homes", "/trees/.kit-homes/agy"),
    (
        r"(?i)consensflow-trees",
        concat!("ngvoi", "cu/consensflow-trees/.kit"),
    ),
    (r"(?i)agy-mac", "/.kit-homes/agy-mac/.gemini"),
    // A path cut at its first folder: the start of `/Users`, with nothing after it.
    (r"/Users?(?:[^A-Za-z0-9_.-]|$)", "/Users\\n"),
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
        } else if kind.is_symlink() {
            return Err(std::io::Error::other(format!(
                "a fixture link cannot be checked: {}",
                entry.path().display()
            )));
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

/// Find every fixture directory, including those outside crates' `tests` directories.
/// Never follow links or descend into generated build and package directories.
fn fixtures_below(folder: &std::path::Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() && entry.file_name() == "fixtures" {
            return Err(std::io::Error::other(format!(
                "a fixture directory link cannot be checked: {}",
                entry.path().display()
            )));
        }
        if !kind.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if name == "fixtures" {
            fixture_files(&entry.path(), files)?;
        } else if ![
            ".git",
            ".cache",
            "node_modules",
            "target",
            "test-results",
            "playwright-report",
        ]
        .iter()
        .any(|excluded| name == *excluded)
        {
            fixtures_below(&entry.path(), files)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_fixture_link_cannot_silently_hide_a_file_or_a_fixture_directory() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("record.txt"), "record").unwrap();
    let fixtures = root.path().join("case");
    fs::create_dir(&fixtures).unwrap();
    symlink(source.join("record.txt"), fixtures.join("link.txt")).unwrap();
    assert!(fixture_files(&fixtures, &mut Vec::new()).is_err());
    symlink(&source, root.path().join("fixtures")).unwrap();
    assert!(fixtures_below(root.path(), &mut Vec::new()).is_err());
}

/// Unescape ASCII JSON Unicode escapes and optional slash escapes even in prose and
/// nested strings. The synthetic homes preserve path shapes used by recorded tests.
fn privacy_text(text: &str) -> String {
    static ASCII_ESCAPE: OnceLock<Regex> = OnceLock::new();
    let ascii = ASCII_ESCAPE.get_or_init(|| {
        Regex::new(r"(?i)\\u00([0-7][0-9a-f])").unwrap_or_else(|error| panic!("{error}"))
    });
    ascii
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let byte =
                u8::from_str_radix(&capture[1], 16).unwrap_or_else(|error| panic!("{error}"));
            char::from(byte).to_string()
        })
        .replace(r"\/", "/")
        .replace("/Users/someone/", "/placeholder/")
        .replace("/Users/someone-home/", "/placeholder/")
        .replace("/Users/user/", "/placeholder/")
        .replace("/Users/me/", "/placeholder/")
        .replace("$ROOT/home/", "$ROOT/placeholder/")
}

/// The folder and redraw checks cover every fixture, prose included, and the compressed
/// replay goldens and inline test screens. List every offending file in one failure.
#[test]
fn no_fixture_of_any_harness_holds_a_folder_of_the_owner_or_a_fragment_of_one() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let harness_fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files = Vec::new();
    fixtures_below(&root, &mut files).unwrap();
    fixture_files(
        &root.join("crates/cf-harness/tests/goldens/records"),
        &mut files,
    )
    .unwrap();
    files.extend(
        [
            "crates/cf-base/src/bundle.rs",
            "crates/cf-engine/src/shown/mask/tests.rs",
            "crates/cf-engine/src/shown/screen/tests.rs",
            "crates/cf-harness/src/shared/compacting/tests.rs",
        ]
        .into_iter()
        .map(|path| root.join(path)),
    );
    files.sort();
    for required in [
        harness_fixtures.join("claude"),
        harness_fixtures.join("muse"),
        harness_fixtures.join("history-items"),
        root.join("app/tests/fixtures"),
        root.join("tests/engine/fixtures"),
        root.join("crates/cf-harness/tests/goldens/records"),
    ] {
        assert!(
            files.iter().any(|path| path.starts_with(&required)),
            "{} has files",
            required.display()
        );
    }
    let folders: Vec<(Regex, &str)> = PERSONAL[..4]
        .iter()
        .map(|(pattern, _)| (Regex::new(pattern).unwrap(), *pattern))
        .collect();
    let fragments: Vec<(Regex, &str)> = FRAGMENTS
        .iter()
        .map(|(pattern, _)| (Regex::new(pattern).unwrap(), *pattern))
        .collect();
    let mut offences = Vec::new();
    for path in files {
        let mut data = Vec::new();
        if path.extension().is_some_and(|ext| ext == "gz") {
            MultiGzDecoder::new(fs::File::open(&path).unwrap())
                .read_to_end(&mut data)
                .unwrap();
        } else {
            data = fs::read(&path).unwrap();
        }
        let mut text = privacy_text(&String::from_utf8_lossy(&data));
        // These exact identities are part of the application and release contracts,
        // not a person's home or a recorded link to their repository.
        if path == root.join("crates/cf-panes/tests/fixtures/tcc/app-preflight.ndjson")
            || path == root.join("crates/cf-base/src/bundle.rs")
        {
            text = text.replace(
                concat!("dev.ngvoi", "cu.consensflow"),
                "dev.example.consensflow",
            );
        }
        if path == root.join("tests/fixtures/install/make-fixtures.mjs")
            || path == root.join("tools/cf-release/tests/fixtures/prepare-update/latest.json")
            || path == root.join("crates/cf-base/src/bundle.rs")
        {
            text = text.replace(
                concat!("https://github.com/ngvoi", "cu/consensflow/releases"),
                "https://github.com/example/consensflow/releases",
            );
        }
        for (found, pattern) in &folders {
            if found.is_match(&text) {
                offences.push(format!("{} matches {pattern}", path.display()));
            }
        }
        for (found, pattern) in &fragments {
            if found.is_match(&text) {
                offences.push(format!("{} matches {pattern}", path.display()));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "fixture privacy violations:\n{}",
        offences.join("\n")
    );
}

#[test]
fn folder_patterns_find_plain_and_json_escaped_paths_but_allow_synthetic_homes() {
    for (pattern, sample) in &PERSONAL[..4] {
        let found = Regex::new(pattern).unwrap();
        assert!(found.is_match(&privacy_text(sample)));
        assert!(found.is_match(&privacy_text(&sample.replace('/', r"\/"))));
        assert!(found.is_match(&privacy_text(&sample.replace('/', r"\u002f"))));
    }
    let found = Regex::new(PERSONAL[0].0).unwrap();
    for placeholder in [
        "/Users/someone/work",
        r"\/Users\/someone\/work",
        "/Users/someone-home/work",
        "/Users/user/work",
        "/Users/me/work",
    ] {
        assert!(!found.is_match(&privacy_text(placeholder)), "{placeholder}");
    }
    let linux_home = Regex::new(PERSONAL[1].0).unwrap();
    assert!(!linux_home.is_match(&privacy_text("$ROOT/home/integrations/devin")));
    let name = Regex::new(FRAGMENTS[0].0).unwrap();
    assert!(name.is_match(&privacy_text(concat!(r"g\u0061br", "iel"))));
}

/// The second reading's probe (T-1885, 15): a home folder mounted below another folder
/// (`/var/home/…`) is as personal as `/home/…`, which the check refused on main; the placeholder
/// paths of the fixtures are named so that no home folder is left in them.
#[test]
fn a_mounted_home_folder_is_found_as_a_personal_one() {
    let found = |text: &str| {
        PERSONAL
            .iter()
            .any(|(pattern, _)| Regex::new(pattern).unwrap().is_match(text))
    };
    for personal in [
        "/home/test-person/work",
        "/var/home/test-person/work",
        "/mnt/home/test-person/work",
    ] {
        assert!(found(personal), "{personal}");
    }
    assert!(!found("/kit/kept/.gemini"));
}

#[test]
fn what_the_fragment_check_looks_for_is_what_a_cut_redraw_has() {
    for (pattern, sample) in FRAGMENTS {
        assert!(
            Regex::new(pattern).unwrap().is_match(sample),
            "{pattern} does not find {sample}"
        );
    }
    // Placeholder paths of the fixtures are not found.
    for clean in ["/work/proj", "/kit/kept/.gemini", "/Users-of-this"] {
        assert!(
            !FRAGMENTS
                .iter()
                .any(|(p, _)| Regex::new(p).unwrap().is_match(clean)),
            "{clean}"
        );
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
