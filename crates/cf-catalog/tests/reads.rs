//! The roster's reads through its public face: what each of the three
//! (`list`, `agent_row`, `preferences`) refuses, and how each answers, on
//! files written by hand. The ported tests of `roster.test.mjs` are in
//! `tests/roster`, the golden cases in `tests/goldens.rs`; these are the
//! rules each of them leaves to a test of its own.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::refusal::Refusal;
use cf_catalog::{Catalog, Preferences, Roster};
use serde_json::json;
use tempfile::{tempdir, TempDir};

/// A home with an agents file in it, or none, and the catalog it is read over.
struct Fixture {
    home: TempDir,
    catalog: Catalog,
}

impl Fixture {
    fn empty() -> Self {
        Self {
            home: tempdir().unwrap(),
            catalog: Catalog::bundled().unwrap(),
        }
    }

    /// A home whose agents file holds these bytes.
    fn with(bytes: impl AsRef<[u8]>) -> Self {
        let fixture = Self::empty();
        fs::write(fixture.path(), bytes).unwrap();
        fixture
    }

    fn path(&self) -> PathBuf {
        self.home.path().join("agents.json")
    }

    fn roster(&self) -> Roster<'_> {
        Roster::new(&self.catalog, self.path())
    }

    /// What each read refuses with, in the order `list`, `agent_row`, `preferences`.
    fn refusals(&self) -> [Refusal; 3] {
        let roster = self.roster();
        [
            roster.list().unwrap_err(),
            roster.agent_row("nova").unwrap_err(),
            roster.preferences().unwrap_err(),
        ]
    }

    /// The row `agent_row` finds, as JSON text.
    fn row_text(&self, name: &str) -> Option<String> {
        let row = self.roster().agent_row(name).unwrap()?;
        Some(serde_json::to_string(&row).unwrap())
    }
}

/// The sentence a file that cannot be used is said with.
fn sentence(path: &Path, why: &str) -> String {
    format!(
        "Your agents file {} {why}: fix it or move it away. ConsensFlow left it as it is.",
        path.display()
    )
}

const FILE_REFUSAL: (&str, u16) = ("agents-file-unreadable", 400);

#[test]
fn a_missing_file_is_an_empty_roster_for_every_read_and_nothing_is_made() {
    let fixture = Fixture::empty();
    let roster = fixture.roster();
    assert_eq!(
        roster.list().unwrap().len(),
        fixture.catalog.presets().len()
    );
    assert_eq!(
        roster.preferences().unwrap(),
        Preferences {
            own_harness_only: false
        }
    );
    assert!(roster.agent_row("thoth").unwrap().is_some());
    assert!(roster.agent_row("nobody").unwrap().is_none());
    assert_eq!(fs::read_dir(fixture.home.path()).unwrap().count(), 0);
}

#[test]
fn a_missing_folder_is_a_missing_file_and_is_not_made() {
    let fixture = Fixture::empty();
    let folder = fixture.home.path().join("consensflow");
    let roster = Roster::new(&fixture.catalog, folder.join("agents.json"));
    assert_eq!(
        roster.list().unwrap().len(),
        fixture.catalog.presets().len()
    );
    assert!(!folder.exists());
}

#[test]
fn a_directory_at_the_path_is_refused_by_every_read_as_node_refuses_it() {
    let fixture = Fixture::empty();
    fs::create_dir(fixture.path()).unwrap();
    for refusal in fixture.refusals() {
        assert_eq!(
            refusal.message,
            sentence(&fixture.path(), "cannot be read (EISDIR)")
        );
        assert_eq!((refusal.code, refusal.status), FILE_REFUSAL);
    }
    assert!(fixture.path().is_dir());
}

#[cfg(unix)]
#[test]
fn a_file_that_may_not_be_read_is_refused_by_every_read_with_its_errno() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::with("{}");
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads what it likes: there is nothing to refuse it.
    if fs::read(fixture.path()).is_ok() {
        return;
    }
    for refusal in fixture.refusals() {
        assert_eq!(
            refusal.message,
            sentence(&fixture.path(), "cannot be read (EACCES)")
        );
        assert_eq!((refusal.code, refusal.status), FILE_REFUSAL);
    }
}

#[test]
fn a_file_that_is_no_json_is_refused_by_every_read_whatever_is_wrong_with_it() {
    let texts: [&[u8]; 5] = [
        b"",
        b"\xEF\xBB\xBF{}",
        br#"{ "agents": [1,] }"#,
        b"not json",
        b"{\"agents\": []",
    ];
    for text in texts {
        let fixture = Fixture::with(text);
        for refusal in fixture.refusals() {
            assert_eq!(
                refusal.message,
                sentence(&fixture.path(), "is not valid JSON"),
                "{text:?}"
            );
            assert_eq!((refusal.code, refusal.status), FILE_REFUSAL);
        }
        assert_eq!(fs::read(fixture.path()).unwrap(), text, "left as it is");
    }
}

#[test]
fn json_that_is_no_object_is_refused_by_every_read_as_no_agents_file() {
    for text in [
        "null",
        "[]",
        r#"[{"id":"nova"}]"#,
        "5",
        r#""agents""#,
        "true",
    ] {
        let fixture = Fixture::with(text);
        for refusal in fixture.refusals() {
            assert_eq!(
                refusal.message,
                sentence(&fixture.path(), "is not an agents file"),
                "{text}"
            );
            assert_eq!((refusal.code, refusal.status), FILE_REFUSAL);
        }
    }
}

/// Stricter than Node, decided and kept on purpose: Node failed with a
/// TypeError, or passed the value through, in whichever function met it.
#[test]
fn a_row_of_the_wrong_shape_is_refused_by_every_read_the_preferences_among_them() {
    let rows = [
        // No object.
        "null",
        "5",
        r#""nova""#,
        "[]",
        // An id, a kind or a model that is no text, `null` included.
        r#"{"id":null}"#,
        r#"{"id":5,"kind":"codex"}"#,
        r#"{"id":"nova","kind":null}"#,
        r#"{"id":"nova","kind":5}"#,
        r#"{"id":"nova","kind":"codex","model":false}"#,
        r#"{"id":"nova","kind":"codex","model":["m"]}"#,
        // A preset, a harness, an effort or a thinking that is set and no text.
        r#"{"id":"nova","preset":{}}"#,
        r#"{"id":"nova","harness":true}"#,
        r#"{"id":"nova","kind":"codex","effort":3}"#,
        r#"{"id":"nova","kind":"pi","thinking":[]}"#,
    ];
    for row in rows {
        let text = format!(r#"{{"schemaVersion":1,"agents":[{row}]}}"#);
        let fixture = Fixture::with(&text);
        for refusal in fixture.refusals() {
            assert_eq!(
                refusal.message,
                sentence(&fixture.path(), "is not an agents file"),
                "{row}"
            );
            assert_eq!((refusal.code, refusal.status), FILE_REFUSAL, "{row}");
        }
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), text);
    }
}

#[test]
fn a_tier_no_longer_known_fails_the_list_alone() {
    let fixture = Fixture::with(
        json!({ "agents": [{ "id": "nova", "kind": "codex", "model": "gpt-6-astra", "workTier": "huge" }] })
            .to_string(),
    );
    let roster = fixture.roster();
    let refusal = roster.list().unwrap_err();
    assert_eq!(
        refusal.message,
        "Work tier must be critical, complex, standard or light"
    );
    assert_eq!((refusal.code, refusal.status), ("work-tier", 400));
    let row = roster.agent_row("nova").unwrap().unwrap();
    assert_eq!(row.get("workTier"), Some(&json!("huge")));
    assert_eq!(
        roster.preferences().unwrap(),
        Preferences {
            own_harness_only: false
        }
    );
}

#[test]
fn a_row_is_found_by_its_name_with_one_leading_at_taken_off() {
    let fixture = Fixture::with(
        r#"{"agents":[{"id":"nova","model":"first"},{"id":"nova","model":"second"},{"id":"@odd","model":"at"}]}"#,
    );
    let roster = fixture.roster();
    let model = |name: &str| {
        roster
            .agent_row(name)
            .unwrap()
            .and_then(|row| row.model().map(str::to_owned))
    };
    assert_eq!(model("nova").as_deref(), Some("first"), "the first of two");
    assert_eq!(model("@nova").as_deref(), Some("first"));
    assert_eq!(model("@@odd").as_deref(), Some("at"), "one `@` only");
    assert_eq!(model("@odd"), None);
    assert_eq!(model("odd"), None);
    assert_eq!(model("Nova"), None);
    assert_eq!(model("nova "), None);
}

#[test]
fn an_absent_name_is_the_empty_name_which_only_a_row_with_that_id_answers() {
    let nobody = Fixture::empty();
    assert!(nobody.roster().agent_row("").unwrap().is_none());
    assert!(nobody.roster().agent_row("@").unwrap().is_none());
    let nameless = Fixture::with(r#"{"agents":[{"id":"","model":"empty"},{"model":"no id"}]}"#);
    let row = nameless.roster().agent_row("").unwrap().unwrap();
    assert_eq!(row.model(), Some("empty"));
    let same = nameless.roster().agent_row("@").unwrap().unwrap();
    assert_eq!(same.model(), Some("empty"));
}

#[test]
fn a_custom_row_that_took_a_catalog_name_answers_for_it() {
    let fixture = Fixture::with(r#"{"agents":[{"id":"thoth","kind":"codex","model":"gpt-x"}]}"#);
    let row = fixture.roster().agent_row("thoth").unwrap().unwrap();
    assert_eq!(row.kind(), Some("codex"));
    assert_eq!(row.get("custom"), Some(&json!(true)));
}

#[test]
fn each_read_reads_the_file_afresh() {
    let fixture = Fixture::with(r#"{"agents":[{"id":"mine","kind":"codex","model":"first"}]}"#);
    let roster = fixture.roster();
    assert_eq!(
        roster.agent_row("mine").unwrap().unwrap().model(),
        Some("first")
    );
    fs::write(
        fixture.path(),
        r#"{"agents":[{"id":"mine","kind":"codex","model":"second"}],"preferences":{"ownHarnessOnly":true}}"#,
    )
    .unwrap();
    assert_eq!(
        roster.agent_row("mine").unwrap().unwrap().model(),
        Some("second")
    );
    assert!(roster.preferences().unwrap().own_harness_only);
    fs::write(fixture.path(), "{").unwrap();
    assert!(roster.list().is_err());
    fs::remove_file(fixture.path()).unwrap();
    assert!(roster.agent_row("mine").unwrap().is_none());
    assert_eq!(
        roster.list().unwrap().len(),
        fixture.catalog.presets().len()
    );
}

#[test]
fn the_row_of_a_custom_image_agent_is_a_codex_designer_and_custom_comes_last() {
    let fixture = Fixture::with(
        r#"{"agents":[{"id":"draw","kind":"image","model":"gpt-image-2","description":"d"}]}"#,
    );
    assert_eq!(
        fixture.row_text("draw").as_deref(),
        Some(
            r#"{"id":"draw","kind":"codex","model":"gpt-image-2","description":"d","designer":true,"custom":true}"#
        )
    );
}

#[test]
fn custom_goes_in_place_when_the_row_has_it_and_last_when_it_has_not() {
    let fixture = Fixture::with(
        r#"{"agents":[{"id":"a","custom":false,"kind":"codex"},{"id":"b","kind":"codex"},{"custom":"yes","id":"c"}]}"#,
    );
    assert_eq!(
        fixture.row_text("a").as_deref(),
        Some(r#"{"id":"a","custom":true,"kind":"codex"}"#)
    );
    assert_eq!(
        fixture.row_text("b").as_deref(),
        Some(r#"{"id":"b","kind":"codex","custom":true}"#)
    );
    assert_eq!(
        fixture.row_text("c").as_deref(),
        Some(r#"{"custom":true,"id":"c"}"#)
    );
}

#[test]
fn a_row_keeps_every_field_it_carries_with_the_index_keys_first_at_every_level() {
    let fixture = Fixture::with(
        r#"{"agents":[{"id":"x","9":1,"kind":"codex","designer":"no","workTier":"huge","description":{"b":1,"2":2},"name":7,"createdAt":null,"unknown":[{"z":0,"1":1}]}]}"#,
    );
    assert_eq!(
        fixture.row_text("x").as_deref(),
        Some(concat!(
            r#"{"9":1,"id":"x","kind":"codex","designer":"no","workTier":"huge","#,
            r#""description":{"2":2,"b":1},"name":7,"createdAt":null,"#,
            r#""unknown":[{"1":1,"z":0}],"custom":true}"#
        ))
    );
}

#[test]
fn the_list_says_hidden_last_and_only_for_the_rows_kept_out_of_sight() {
    let fixture = Fixture::with(
        r#"{"agents":[{"id":"relay","kind":"pi","model":"claude-x"},{"id":"own","kind":"claude-code","model":"claude-x"}],"preferences":{"ownHarnessOnly":true}}"#,
    );
    let listed = fixture.roster().list().unwrap();
    let text = |name: &str| {
        let view = listed
            .iter()
            .find(|view| view.name.as_deref() == Some(name))
            .unwrap();
        serde_json::to_string(view).unwrap()
    };
    assert_eq!(
        text("relay"),
        concat!(
            r#"{"name":"relay","harness":"pi","model":"claude-x","custom":true,"#,
            r#""profile":{"modelKey":"claude-x","modelLabel":"claude-x","routeLabel":"pi","workTier":"light"},"#,
            r#""hidden":true}"#
        )
    );
    assert!(!text("own").contains("hidden"));
}
