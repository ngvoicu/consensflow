//! What the players of Node's recordings share when they hold a database this
//! ledger left, or the events it logged, to what Node's ledger did: what only
//! this one writes is held apart, so that what the two wrote in everything
//! else is compared, and nothing of what only this one keeps (a stop count, a
//! carrier, a claim, a shut door) moves a recording that predates it. That is
//! migration 0011's four columns, which Node's ledger never fills, and the
//! stop a pause counted, which it never says in the event that moved the task.

use serde_json::{Map, Value};

/// The columns migration 0011 added, by table, whose values only this ledger writes.
const NODE_NEVER_WRITES: [(&str, &str); 4] = [
    ("task", "stop_seq"),
    ("message", "carried_by"),
    ("message", "claimed_at"),
    ("message", "door_closed_at"),
];

/// `tables`, a database as the recorder dumps it (each table's `columns`
/// and its `rows`), without what Node's ledger never writes: the four
/// columns (their name and, in every row, the value at its place), and the
/// stop in the event rows that paused a task. A dump from before the
/// migration has none of them, and is left as it is.
pub fn hold_apart_what_node_never_writes(tables: &mut Map<String, Value>) {
    hold_apart_the_stop_of_logged_pauses(tables);
    for (table, column) in NODE_NEVER_WRITES {
        let Some(Value::Object(dump)) = tables.get_mut(table) else {
            continue;
        };
        let Some(at) = dump
            .get("columns")
            .and_then(Value::as_array)
            .and_then(|columns| columns.iter().position(|name| name == column))
        else {
            continue;
        };
        if let Some(Value::Array(columns)) = dump.get_mut("columns") {
            columns.remove(at);
        }
        if let Some(Value::Array(rows)) = dump.get_mut("rows") {
            for row in rows {
                if let Value::Array(values) = row {
                    values.remove(at);
                }
            }
        }
    }
}

/// The `event` table's pause rows, each value as SQLite quotes it, without the
/// `"stop"` that ends the quoted JSON of their data.
fn hold_apart_the_stop_of_logged_pauses(tables: &mut Map<String, Value>) {
    let Some(Value::Object(events)) = tables.get_mut("event") else {
        return;
    };
    let position = |name: &str| {
        events
            .get("columns")
            .and_then(Value::as_array)
            .and_then(|columns| columns.iter().position(|column| column == name))
    };
    let (Some(kind), Some(data)) = (position("kind"), position("data")) else {
        return;
    };
    let Some(Value::Array(rows)) = events.get_mut("rows") else {
        return;
    };
    for row in rows {
        let Value::Array(cells) = row else { continue };
        let paused = cells.get(kind).and_then(Value::as_str) == Some("'task.state'");
        let Some(Value::String(text)) = cells.get_mut(data) else {
            continue;
        };
        if paused && text.contains("\"to\":\"paused\"") {
            if let Some(without) = without_stop(text) {
                *text = without;
            }
        }
    }
}

/// What ends the quoted JSON of a pause's data: the stop it counted.
const STOP: &str = ",\"stop\":";

/// `text`, the quoted JSON of a pause's data, without the stop that ends it;
/// none where it does not end with one.
fn without_stop(text: &str) -> Option<String> {
    let at = text.find(STOP)?;
    let digits = text[at + STOP.len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    let end = at + STOP.len() + digits;
    text[end..]
        .starts_with('}')
        .then(|| format!("{}{}", &text[..at], &text[end..]))
}

/// `events`, as the recorder writes what a ledger logged (`kind` and `data`
/// among them), without the one thing this ledger adds to an event Node logs
/// too: the stop a pause counted, in the `data` of the event that moved the
/// task to paused. Nothing else of what it logs is held apart: an event
/// of a kind Node never logs is a difference.
pub fn hold_apart_what_node_never_logs(events: &mut [Value]) {
    for event in events {
        let paused = event["kind"] == "task.state" && event["data"]["to"] == "paused";
        if let (true, Some(Value::Object(data))) = (paused, event.get_mut("data")) {
            data.shift_remove("stop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn takes_the_stop_out_of_the_event_that_paused_a_task_and_nothing_else() {
        let mut events = [
            json!({ "kind": "task.state", "data": { "task": 1, "from": "working", "to": "paused", "by": "chief", "stop": 2 } }),
            json!({ "kind": "task.state", "data": { "task": 1, "from": "paused", "to": "queued", "stop": 2 } }),
            json!({ "kind": "message.read", "data": { "message": 4, "stop": 9 } }),
        ];
        hold_apart_what_node_never_logs(&mut events);
        assert_eq!(
            events,
            [
                json!({ "kind": "task.state", "data": { "task": 1, "from": "working", "to": "paused", "by": "chief" } }),
                json!({ "kind": "task.state", "data": { "task": 1, "from": "paused", "to": "queued", "stop": 2 } }),
                json!({ "kind": "message.read", "data": { "message": 4, "stop": 9 } }),
            ]
        );
    }

    #[test]
    fn takes_the_stop_out_of_the_pause_rows_of_the_event_table_as_sqlite_quotes_them() {
        let pause = r#"'{"task":1,"from":"working","to":"paused","by":null,"because":"it''s out","stop":12}'"#;
        let other = r#"'{"task":1,"from":"paused","to":"queued","stop":12}'"#;
        let mut tables = json!({
            "event": {
                "columns": ["id", "kind", "data"],
                "rows": [["1", "'task.state'", pause], ["2", "'task.state'", other], ["3", "'message.sent'", pause]],
            },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables["event"]["rows"],
            json!([
                [
                    "1",
                    "'task.state'",
                    r#"'{"task":1,"from":"working","to":"paused","by":null,"because":"it''s out"}'"#
                ],
                ["2", "'task.state'", other],
                ["3", "'message.sent'", pause],
            ])
        );
    }

    #[test]
    fn takes_the_four_columns_and_their_values_out_and_nothing_else() {
        let mut tables = json!({
            "task": { "columns": ["id", "stop_seq"], "rows": [["1", "0"], ["2", "3"]] },
            "message": {
                "columns": ["id", "carried_by", "state", "claimed_at", "door_closed_at"],
                "rows": [["5", "NULL", "'queued'", "NULL", "NULL"]],
            },
            "event": { "columns": ["id"], "rows": [["1"]] },
        });
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(
            tables,
            json!({
                "task": { "columns": ["id"], "rows": [["1"], ["2"]] },
                "message": { "columns": ["id", "state"], "rows": [["5", "'queued'"]] },
                "event": { "columns": ["id"], "rows": [["1"]] },
            })
        );
    }

    #[test]
    fn leaves_a_dump_from_before_the_migration_as_it_is() {
        let before = json!({
            "task": { "columns": ["id"], "rows": [["1"]] },
            "message": { "columns": ["id", "state"], "rows": [["5", "'queued'"]] },
        });
        let mut tables = before.clone();
        hold_apart_what_node_never_writes(tables.as_object_mut().unwrap());
        assert_eq!(tables, before);
    }
}
