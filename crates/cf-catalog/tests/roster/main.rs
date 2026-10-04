//! The tests of `tests/roster.test.mjs` that only read, ported with the
//! roster's reads: each keeps its sentence, as a name, and its assertions.
//! The tests that write wait for the roster's writes.
//!
//! A `describe` block of the JS is a module here, in a file of its own; the
//! tests outside one are in this file. Where a `describe` mixes reading and
//! writing, the reading half is a test of its own (the file the JS wrote
//! with `addAgent` or `setPreferences` is written by hand), named for what
//! it reads, and says which JS test its writing half is.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_catalog::{roster_path, AgentView, Catalog, Roster, WorkTier};
use tempfile::TempDir;

mod agents_defined_by_hand_are_stored_in_full_v1_shaped;
mod an_agents_file_that_cannot_be_read;
mod the_human_s_own_agent_and_a_catalog_that_takes_its_name_later;
mod the_roster_is_the_catalog_plus_what_is_the_human_s_own;
mod the_roster_keeps_what_the_human_chose_about_it;
mod what_older_builds_wrote_is_read_the_same_and_folded_at_start;

/// A throwaway home, as `tempEnv()` makes one: `CONSENSFLOW_HOME` is
/// `<root>/consensflow`, and nothing is in it until a test puts it there.
struct Home {
    root: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn env(&self) -> Env {
        Env::from_vars([
            ("HOME", self.root.path().join("home")),
            ("CONSENSFLOW_HOME", self.root.path().join("consensflow")),
        ])
    }

    /// The agents file: `rosterPath(env)`.
    fn path(&self) -> PathBuf {
        roster_path(&self.env()).unwrap()
    }

    /// The file's text, written by hand into its folder.
    fn write(&self, text: &str) {
        let path = self.path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// The file's text.
    fn text(&self) -> String {
        fs::read_to_string(self.path()).unwrap()
    }

    /// `seedSharedRoster`: the v1 fixture, put in the file's place.
    fn seed_the_v1_roster(&self) {
        self.write(&fs::read_to_string(v1_fixture()).unwrap());
    }

    /// The agents as the page and the CLI list them, by name (`byName`):
    /// of two of one name, the later.
    fn by_name(&self, catalog: &Catalog) -> HashMap<String, AgentView> {
        Roster::new(catalog, self.path())
            .list()
            .unwrap()
            .into_iter()
            .filter_map(|agent| Some((agent.name.clone()?, agent)))
            .collect()
    }
}

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

fn v1_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("v1-agents.json")
}

#[test]
fn reports_current_tiers_from_legacy_rows_without_writing_during_discovery() {
    let home = Home::new();
    let original = r#"{"agents":[{"id":"renamed","kind":"claude-code","model":"claude-fable-5-1","effort":"max","skillsPolicy":"default","profile":{"categories":["coding","chief","pm"]}}]}"#;
    home.write(original);
    let catalog = catalog();
    let agents = home.by_name(&catalog);
    let agent = &agents["renamed"];
    assert_eq!(agent.profile.work_tier, WorkTier::Critical);
    assert!(agent.custom);
    let profile = serde_json::to_value(&agent.profile).unwrap();
    assert!(
        profile.get("categories").is_none(),
        "stale pills are dropped"
    );
    assert_eq!(home.text(), original);
}
