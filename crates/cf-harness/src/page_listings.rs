//! The page's recorded listings: what `models.get` answers of each harness, as each
//! adapter reads the answer its harness was recorded giving (CHF-03's fixtures, under
//! `tests/fixtures/`). The page's tests pick a member's model from these files
//! (`app/tests/fixtures/models/<harness>.json`, T-1143), so they hold what a harness
//! really lists, not words a test made up. A parser that changes what it reads of a
//! recording fails here until the files are written again: `CF_WRITE_PAGE_LISTINGS=1`
//! writes them.

use std::path::PathBuf;

use cf_proto::models::ModelListing;
use serde_json::Value;

/// When the recorded lists are said to have been read.
const AT: i64 = 1_793_000_000_000;

fn recorded(path: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// Each harness's listing, by its program's name, as its adapter reads its recording.
fn listings() -> Vec<(&'static str, ModelListing)> {
    vec![
        (
            "claude",
            crate::claude::models::parse(&recorded("models/claude-initialize.json"), AT).unwrap(),
        ),
        (
            "codex",
            crate::codex::models::parse(
                &[recorded("models/codex-model-list.json")],
                Some(AT - 60_000),
                AT,
            )
            .unwrap(),
        ),
        (
            "pi",
            crate::pi::models::parse(&recorded("models/pi-get-available-models.json"), AT)
                .unwrap(),
        ),
        (
            "devin",
            crate::devin::models::parse(&recorded("models/devin-models-list.json"), AT).unwrap(),
        ),
        (
            "muse",
            crate::muse::models::parse(&recorded("muse/msp-model-list.json")["result"], AT)
                .unwrap(),
        ),
    ]
}

#[test]
fn the_page_tests_hold_each_harness_s_recorded_listing() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/tests/fixtures/models");
    let write = cf_base::env::Env::from_process()
        .text("CF_WRITE_PAGE_LISTINGS")
        .is_some();
    for (harness, listing) in listings() {
        let path = folder.join(format!("{harness}.json"));
        let text = format!("{}\n", serde_json::to_string_pretty(&listing).unwrap());
        if write {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(&path, &text).unwrap();
            continue;
        }
        let kept = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            kept == text,
            "{} is not what {harness}'s adapter reads of its recording: CF_WRITE_PAGE_LISTINGS=1 writes it",
            path.display()
        );
    }
}
