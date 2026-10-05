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
        let outcome = admission(&Sent::from_reply(&sent), refusal.unwrap_or(""), queued);
        let mut written = written(&outcome);
        // Every adapter names its refusal: a failure with no words of its
        // own and none named said no reason in JavaScript, and says "" here.
        if refusal.is_none() && row["outcome"].get("reason").is_none() {
            if let Some(fields) = written.as_object_mut() {
                if fields.get("reason") == Some(&json!("")) {
                    fields.remove("reason");
                }
            }
        }
        assert_eq!(written, row["outcome"], "{row}");
    }
}
