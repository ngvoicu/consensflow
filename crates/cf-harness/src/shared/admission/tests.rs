//! The admissions of `tests/goldens/launch/tables.json`, each as Node's
//! `admission` read the answer.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::*;

/// An outcome as Node wrote it.
fn written(admission: &Admission) -> Value {
    match admission {
        Admission::Admitted { queued: false } => json!({ "admitted": true }),
        Admission::Admitted { queued: true } => json!({ "admitted": true, "queued": true }),
        Admission::Refused { reason } => json!({ "admitted": false, "reason": reason }),
        Admission::Uncertain { reason } => json!({ "admitted": null, "reason": reason }),
    }
}

#[test]
fn every_answer_reads_as_the_outcome_node_read() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch/tables.json");
    let tables: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    let rows = tables["admission"].as_array().unwrap();
    assert_eq!(rows.len(), 120);
    let undefined = json!({ "undefined": true });
    for row in rows {
        let sent = if row["sent"] == undefined {
            Value::Null
        } else {
            row["sent"].clone()
        };
        let refusal = row["refusal"].as_str();
        let queued = row["options"]["queued"] == Value::Bool(true);
        let read = Sent::from_reply(&sent);
        let outcome = admission(&read, refusal.unwrap_or(""), queued);
        let mut written = written(&outcome);
        // Every adapter names its refusal, so a failure with no words of its
        // own reads as that. Node was also asked with none named, and said no
        // reason (undefined, which JSON leaves out): Rust is never asked so.
        if refusal.is_none() && !read.ok && read.cause.is_none() && read.error.is_none() {
            if let Some(fields) = written.as_object_mut() {
                fields.remove("reason");
            }
        }
        assert_eq!(written, row["outcome"], "{row}");
    }
}

#[test]
fn reads_a_refusal_only_where_the_channel_says_nothing_reached_the_harness() {
    let read = |reply: Value| admission(&Sent::from_reply(&reply), "refused", false);
    let refused = |reason: &str| Admission::Refused {
        reason: reason.to_owned(),
    };
    let uncertain = |reason: &str| Admission::Uncertain {
        reason: reason.to_owned(),
    };
    assert_eq!(
        read(json!({ "ok": true, "admitted": true })),
        Admission::Admitted { queued: false }
    );
    let stale = json!({
        "ok": false, "admitted": false, "bytesWritten": 0, "error": "stale-generation", "cause": "gone",
    });
    assert_eq!(read(stale), refused("gone"));
    assert_eq!(
        read(json!({ "ok": false, "admitted": false })),
        refused("refused")
    );
    let cut = json!({ "ok": false, "admitted": null, "error": "uncertain", "cause": "cut off" });
    assert_eq!(read(cut), uncertain("cut off"));
    // An answer that does not say may have reached the harness: sending it
    // again at once is how Pi got a message twice.
    let transport = json!({ "ok": false, "error": "transport", "cause": "EISDIR" });
    assert_eq!(read(transport), uncertain("EISDIR"));
}

#[test]
fn a_reason_that_is_not_words_is_written_as_a_template_writes_it() {
    // Kept from Node on purpose: the pane host's `cause` and `error` are its
    // words; Node passed any other value on as the reason itself (`cause:
    // 0` a reason of 0), which no host of ConsensFlow's sends.
    let read = |reply: Value| admission(&Sent::from_reply(&reply), "refused", false);
    let refused = |reason: &str| Admission::Refused {
        reason: reason.to_owned(),
    };
    assert_eq!(
        read(json!({ "ok": false, "admitted": false, "cause": 0 })),
        refused("0")
    );
    assert_eq!(
        read(json!({ "ok": false, "admitted": false, "cause": false })),
        refused("false")
    );
    assert_eq!(
        read(json!({ "ok": false, "admitted": false, "cause": [1, 2] })),
        refused("1,2")
    );
    assert_eq!(
        read(json!({ "ok": false, "admitted": false, "cause": {} })),
        refused("[object Object]")
    );
    assert_eq!(
        read(json!({ "ok": false, "admitted": false, "cause": null, "error": "" })),
        refused(""),
        "null is none, and an empty word is a word, as `??` took them"
    );
}
