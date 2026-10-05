//! The layouts of `tests/goldens/admin/tables.json`, each a path as a CLI is
//! found at, read as Node read it: the same on every system but for the
//! separator `join` makes, which the table spells `/`.

use cf_base::env::Env;
use cf_harness::admin::release_source;
use cf_proto::agents::Harness;
use serde_json::Value;

use crate::shape::{field, golden, pretty, source_json, text};

/// How many layouts there are at least: a table with fewer has lost some.
const AT_LEAST: usize = 50;

/// A source with every part of its command spelled with `/`.
fn plain(mut source: Value) -> Value {
    if let Some(update) = source["update"].as_array_mut() {
        for part in update {
            *part = Value::String(part.as_str().unwrap().replace('\\', "/"));
        }
    }
    source
}

#[test]
fn every_layout_that_is_a_text_is_read_as_node_read_it() {
    let table = golden("tables.json");
    let rows = table["sources"].as_array().unwrap();
    assert!(rows.len() >= AT_LEAST, "{} layouts", rows.len());
    let env = Env::from_vars([("HOME", "/no/such/home")]);
    let mut differing = Vec::new();
    for row in rows {
        let harness = Harness::from_name(field(row, "id")).unwrap();
        let executable = field(row, "executable");
        let source = plain(source_json(&release_source(harness, executable, &env)));
        if text(&source) != text(&row["source"]) {
            differing.push(format!(
                "{} at {executable}:\nRust:\n{}\nNode:\n{}",
                harness.as_str(),
                pretty(&source),
                pretty(&row["source"])
            ));
        }
    }
    assert!(
        differing.is_empty(),
        "{} layouts differ from Node's:\n\n{}",
        differing.len(),
        differing.join("\n\n")
    );
}
