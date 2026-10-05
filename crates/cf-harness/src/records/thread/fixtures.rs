//! The thread against the readers it serves, over the fixtures of the
//! completion suite (`tests/engine/fixtures/completion/`): each one staged as
//! its harness keeps its record, grown a piece at a time as the harness writes
//! it, the clock moving, and looked at after every piece through a [`Thread`]
//! and through [`LocalRecords`](crate::testing::LocalRecords), the one's
//! reading equal to the other's. The pieces are those of the goldens'
//! sequences (`tests/goldens/records`); that the readers read them as Node did
//! is the goldens' player's.

mod jsonl;
mod rig;
mod stores;

use std::collections::BTreeSet;
use std::fs;
use std::sync::Arc;

use cf_proto::agents::Harness;
use serde_json::json;

use super::support::on_engine;
use crate::contract::Records;
use crate::records::{Options, Quota, Reading};
use jsonl::{jsonl, jsonl_file, jsonl_vars, Way};
use rig::{append, fixture_lines, fixtures, Rig, STEP_MS};
use stores::{devin_file_link, devin_fixture, opencode};

/// Each JSONL fixture, and the session it is the record of. Its harness is its
/// folder's.
#[rustfmt::skip]
const JSONL: [(&str, &str); 20] = [
    ("codex/completed.jsonl", "01a074ec-7aff-74b0-8cf6-aa00d8e451cb"),
    ("codex/errored-task-complete.jsonl", "01a074ec-7aff-74b0-8cf6-aa00d8e451cb"),
    ("codex/interrupted.jsonl", "01a077f6-6663-7bc2-81cd-e287ccaabdbd"),
    ("codex/forked.jsonl", "01a077fa-5968-7b62-8fdd-043410a3d4b9"),
    ("codex/big-answer.jsonl", "01a074ec-7aff-74b0-8cf6-aa00d8e451cb"),
    ("claude-code/fragments.jsonl", "15fba934-d727-4777-8791-123675a63649"),
    ("claude-code/frontier-history.jsonl", "15fba934-d727-4777-8791-123675a63649"),
    ("claude-code/queued-turn.jsonl", "1b09fb15-feb1-4595-9f47-5eb9ff768191"),
    ("claude-code/queue-pop-all.jsonl", "1b09fb15-feb1-4595-9f47-5eb9ff768191"),
    ("claude-code/interrupted.jsonl", "1b09fb15-feb1-4595-9f47-5eb9ff768191"),
    ("claude-code/provider-429.jsonl", "33383216-87a0-4e6d-a273-07c4b229cdb1"),
    ("claude-code/compaction.jsonl", "1b09fb15-feb1-4595-9f47-5eb9ff768191"),
    ("claude-code/v263-tool-loop.jsonl", "5cbf8973-f472-448a-8763-59fb4268a9d7"),
    ("claude-code/v265-tool-loop.jsonl", "17499106-8778-48e1-a306-87bd186c9f7e"),
    ("claude-code/v266-tool-loop.jsonl", "47b1090f-b1c7-4d19-95b9-24c09a7f164a"),
    ("claude-code/v268-clear.jsonl", "fb561379-bcab-4045-92d2-d460bb19ed36"),
    ("claude-code/v268-late-ancestors.jsonl", "4e761651-511b-4065-8a65-6ff21582faad"),
    ("pi/between-tool-steps.jsonl", "hazy-ridge"),
    ("pi/tool-loop.jsonl", "hazy-ridge"),
    ("pi/provider-429.jsonl", "triton-jade-fern"),
];

/// OpenCode's fixtures, each a store of rows, which `native-events.json` adds
/// the events of its conversation to.
const OPENCODE: [&str; 5] = [
    "completion-window",
    "finish-length",
    "api-error",
    "tool-result",
    "v130-tool-loop",
];

/// Every fixture there is, by its path under the fixtures folder: the files
/// of conversations, and not the notes about them.
fn every_fixture() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for folder in fs::read_dir(fixtures()).unwrap() {
        let folder = folder.unwrap().path();
        if !folder.is_dir() {
            continue;
        }
        for file in fs::read_dir(&folder).unwrap() {
            let file = file.unwrap().path();
            if file
                .extension()
                .is_some_and(|extension| extension == "jsonl" || extension == "json")
            {
                found.insert(format!(
                    "{}/{}",
                    folder.file_name().unwrap().to_string_lossy(),
                    file.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    found
}

#[test]
fn every_fixture_reads_through_the_thread_as_it_reads_beside_it() {
    let mut staged = BTreeSet::new();
    let looks = on_engine(async {
        let mut looks = 0;
        for (name, session) in JSONL {
            staged.insert(name.to_owned());
            let kind = name.split('/').next().unwrap();
            let harness = Harness::from_kind(kind).unwrap();
            for way in Way::of(harness) {
                looks += jsonl(harness, name, session, *way).await;
            }
        }
        staged.insert("opencode/native-events.json".to_owned());
        for name in OPENCODE {
            staged.insert(format!("opencode/{name}.json"));
            looks += opencode(name).await;
        }
        for name in ["native-tui", "worker-tui"] {
            staged.insert(format!("devin/{name}.json"));
            looks += devin_fixture(name).await;
        }
        for name in ["file-link", "file-link-windows"] {
            staged.insert(format!("devin/{name}.json"));
            looks += devin_file_link(name).await;
        }
        looks
    });
    assert_eq!(
        staged,
        every_fixture(),
        "a fixture no case reads, or one that is gone"
    );
    assert!(looks > 300, "{looks} looks");
}

/// The refusal a reading holds.
fn refusal(reading: &Reading) -> Arc<Quota> {
    match reading {
        Reading::Known(record) => Arc::clone(record.quota.as_ref().expect("a refusal")),
        Reading::Unknown(reason) => panic!("no record was read: {reason}"),
    }
}

#[test]
fn a_refusal_that_has_not_changed_is_the_same_quota_look_after_look() {
    let (harness, name, session) = (
        Harness::Claude,
        "claude-code/provider-429.jsonl",
        "33383216-87a0-4e6d-a273-07c4b229cdb1",
    );
    let rig = Rig::new(name, jsonl_vars(harness));
    let file = rig.root.path().join(jsonl_file(harness, session));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    for line in fixture_lines(name) {
        append(&file, &format!("{line}\n"));
    }
    let records = [
        // A record that says nothing, and a turn that goes on after the refusal.
        json!({ "type": "mode", "mode": "normal", "sessionId": session }),
        json!({
            "type": "user", "uuid": "u-2", "sessionId": session, "isSidechain": false,
            "timestamp": "2026-08-24T12:51:00.000Z", "message": { "role": "user", "content": "go on" },
        }),
    ];
    let options = Options::default();
    on_engine(async {
        let first = rig.through.look(harness, session, &options).await;
        let again = rig.through.look(harness, session, &options).await;
        assert!(
            Arc::ptr_eq(&first, &again),
            "a record that did not change: the reading of the look before"
        );
        let refused = refusal(&first);
        let mut before = first;
        for record in records {
            rig.tick(STEP_MS);
            append(&file, &format!("{record}\n"));
            let after = rig.through.look(harness, session, &options).await;
            assert!(
                !Arc::ptr_eq(&before, &after),
                "a record that grew: another reading"
            );
            assert!(
                Arc::ptr_eq(&refused, &refusal(&after)),
                "the refusal that did not change: one quota"
            );
            before = after;
        }
    });
}
