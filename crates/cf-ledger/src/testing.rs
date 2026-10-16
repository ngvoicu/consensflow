//! What the players of Node's recordings share when they hold a database this
//! ledger left, or the events it logged, to what Node's ledger did: what only
//! this one writes is held apart, so that what the two wrote in everything
//! else is compared, and nothing of what only this one keeps (a stop count, a
//! carrier, a claim, a shut door) moves a recording that predates it. That is
//! migration 0011's four columns, which Node's ledger never fills, and the
//! stop a pause counted, which it never says in the event that moved the task.
//! And what only this one's schema has: migration 0012's tester in the checks
//! of two tables, and the version it brings (a recording of Node's database
//! says 11, and its checks do not name a tester). And migration 0014's three
//! columns of the `project` table (the wake-up calls), which Node's schema never
//! had, in its rows and in the text of its schema.
//!
//! And migration 0015's three columns of the `project` table (what a project
//! says of specs), which Node's projects never had: in the rows of that table
//! and, as SQLite writes them into the table's text, in its schema. And
//! migration 0017's one (the line a human writes under a project's name),
//! held apart as the wake-up columns are.
//!
//! And migration 0016's column of the `conversation` table (the role text a
//! conversation was last given), which Node's conversations never had: in the
//! rows of that table and in the text of its schema.
//!
//! And migration 0018's column of the `message` table (the role text a
//! replacement of a role says), which Node's messages never had: in the rows of
//! that table and in the text of its schema.
//!
//! And migration 0019's column of the `task` table (the mark that a project's
//! suspension paused a task), which Node's tasks never had: in the rows of that
//! table and in the text of its schema.
//!
//! What only Node's ledger had is held apart the other way round, on both
//! sides: the mark that a project comes back by itself after a restart
//! (`resumeOnStart` in what the ledger answers and logs, `resume_on_start` in
//! its database), which migration 0013 took away, is taken out of what Node
//! recorded, where this ledger's has none to take out.

mod described;
mod paused_with_project;
mod role_note;
mod role_text;
mod specs;

use serde_json::{Map, Value};

/// The last schema version Node's ledger had.
const NODE_LAST_VERSION: i64 = 11;

/// What Node's ledger said of a project that came back by itself after a
/// restart, in an answer or an event, and the column that kept it.
const RESUME_MARK: &str = "resumeOnStart";
const RESUME_COLUMN: &str = "resume_on_start";

/// The lines that declare that column in the schema of the `project` table,
/// as Node's migration 0006 wrote them.
const RESUME_COLUMN_LINES: [&str; 2] = [
    "    resume_on_start INTEGER NOT NULL DEFAULT 0,\n",
    "    CONSTRAINT project_resume_on_start_check CHECK (resume_on_start IN (0, 1)),\n",
];

/// The tester, as migration 0012 put it in the role check of `participant` and
/// the pool check of `task`, and the words Node's checks had in its place.
const TESTER_IN_THE_CHECKS: (&str, &str) =
    ("'reviewer', 'tester', 'designer'", "'reviewer', 'designer'");

/// The columns migration 0011 added, by table, whose values only this ledger writes.
const NODE_NEVER_WRITES: [(&str, &str); 4] = [
    ("task", "stop_seq"),
    ("message", "carried_by"),
    ("message", "claimed_at"),
    ("message", "door_closed_at"),
];

/// The columns migrations 0014 and 0017 added to the `project` table, each as
/// the migration says it (`wake_up_on INTEGER NOT NULL DEFAULT 0 CONSTRAINT …`,
/// `description TEXT`, one to a line there): what Node's schema never had.
fn added_columns() -> impl Iterator<Item = &'static str> {
    [
        include_str!("../migrations/0014.sql"),
        include_str!("../migrations/0017.sql"),
    ]
    .into_iter()
    .flat_map(str::lines)
    .filter_map(|line| {
        line.trim()
            .strip_prefix("ALTER TABLE project ADD COLUMN ")?
            .strip_suffix(';')
    })
}

/// `tables`, a database as the recorder dumps it (each table's `columns`
/// and its `rows`), without what Node's ledger never writes: the four
/// columns of migration 0011, the three wake-up columns of migration 0014, the
/// three of migration 0015, the one of migration 0016, the description of
/// migration 0017, the one of migration 0018 and the one of migration 0019 (their
/// name and, in every row, the value at its place), and the stop in the event rows
/// that paused a task. A dump from before the migrations has none of them, and is
/// left as it is.
pub fn hold_apart_what_node_never_writes(tables: &mut Map<String, Value>) {
    hold_apart_the_stop_of_logged_pauses(tables);
    for (table, column) in NODE_NEVER_WRITES
        .into_iter()
        .chain(specs::COLUMNS)
        .chain(role_text::COLUMNS)
        .chain(role_note::COLUMNS)
        .chain(paused_with_project::COLUMNS)
    {
        take_column_out(tables, table, column);
    }
    for column in added_columns().filter_map(|definition| definition.split_whitespace().next()) {
        take_column_out(tables, "project", column);
    }
}

/// `column` of `table`, as the recorder dumps a table (its `columns` and its
/// `rows`): its name and, in every row, the value at its place. A dump with no
/// such table or column is left as it is.
fn take_column_out(tables: &mut Map<String, Value>, table: &str, column: &str) {
    let Some(Value::Object(dump)) = tables.get_mut(table) else {
        return;
    };
    let Some(at) = dump
        .get("columns")
        .and_then(Value::as_array)
        .and_then(|columns| columns.iter().position(|name| name == column))
    else {
        return;
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

/// `dump`, a database as the recorder dumps it (`userVersion`, `schema` and
/// `tables`), with the schema as Node's ledger had it: no version past the
/// last Node's ledger had, no tester in the checks of the two tables that
/// name the roles and the pools, none of the columns of a project's wake-up
/// calls, of what it says of specs or of its description in the table of the
/// projects, and not the column of the role text a conversation was last given,
/// that of the role text a message says, nor that of the mark a project's
/// suspension leaves on the tasks it paused. A dump of Node's is left as it is.
pub fn hold_apart_what_node_never_had_in_its_schema(dump: &mut Value) {
    if let Some(version) = dump.get_mut("userVersion") {
        if version.as_i64().is_some_and(|at| at > NODE_LAST_VERSION) {
            *version = Value::from(NODE_LAST_VERSION);
        }
    }
    let Some(Value::Array(schema)) = dump.get_mut("schema") else {
        return;
    };
    for entry in schema {
        let name = entry["name"].as_str().map(str::to_owned);
        let Some(Value::String(sql)) = entry.get_mut("sql") else {
            continue;
        };
        match name.as_deref() {
            Some("participant") => {
                *sql = sql.replace(TESTER_IN_THE_CHECKS.0, TESTER_IN_THE_CHECKS.1);
            }
            Some("task") => {
                *sql = sql
                    .replace(TESTER_IN_THE_CHECKS.0, TESTER_IN_THE_CHECKS.1)
                    .replace(paused_with_project::COLUMN_LINES, "");
            }
            Some("project") => {
                // SQLite wrote each column after the last one, as a comma and the
                // column's text: the wake-up calls' columns and the description
                // one by one, and the three of specs, which `specs` holds as the
                // one piece they are.
                for column in added_columns() {
                    *sql = sql.replace(&format!(", {column}"), "");
                }
                *sql = sql.replace(specs::COLUMN_LINES, "");
            }
            Some("conversation") => {
                *sql = sql.replace(role_text::COLUMN_LINES, "");
            }
            Some("message") => {
                *sql = sql.replace(role_note::COLUMN_LINES, "");
            }
            _ => {}
        }
    }
}

/// The `event` table's pause rows, each value as SQLite quotes it, without the
/// `"stop"` that ends the quoted JSON of their data.
fn hold_apart_the_stop_of_logged_pauses(tables: &mut Map<String, Value>) {
    edit_logged_data(tables, "'task.state'", |text| {
        if text.contains("\"to\":\"paused\"") {
            if let Some(without) = without_stop(text) {
                *text = without;
            }
        }
    });
}

/// The quoted JSON of the `data` of each row of the `event` table whose `kind`
/// is `kind` (quoted as SQLite quotes it), `edit`ed in place.
fn edit_logged_data(tables: &mut Map<String, Value>, kind: &str, edit: impl Fn(&mut String)) {
    let Some(Value::Object(events)) = tables.get_mut("event") else {
        return;
    };
    let position = |name: &str| {
        events
            .get("columns")
            .and_then(Value::as_array)
            .and_then(|columns| columns.iter().position(|column| column == name))
    };
    let (Some(kind_at), Some(data)) = (position("kind"), position("data")) else {
        return;
    };
    let Some(Value::Array(rows)) = events.get_mut("rows") else {
        return;
    };
    for row in rows {
        let Value::Array(cells) = row else { continue };
        let is_kind = cells.get(kind_at).and_then(Value::as_str) == Some(kind);
        if let (true, Some(Value::String(text))) = (is_kind, cells.get_mut(data)) {
            edit(text);
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

/// `value`, an answer, an event or a whole recording as the recorder wrote it,
/// without the mark that a project comes back by itself: the `resumeOnStart` of
/// every object in it, which Node's projects had, and the event that suspended
/// one at a start.
pub fn hold_apart_the_resume_mark(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.shift_remove(RESUME_MARK);
            fields.values_mut().for_each(hold_apart_the_resume_mark);
        }
        Value::Array(items) => items.iter_mut().for_each(hold_apart_the_resume_mark),
        _ => {}
    }
}

/// `text`, as the recorder wrote it (JSON that holds an answer, an event or a
/// reply), without the mark that a project comes back by itself: each
/// `"resumeOnStart":false` or `:true`, with the comma that joins it to its
/// neighbour. For a player that holds text to text, bytes and all.
pub fn without_the_resume_mark(text: &str) -> String {
    let mut text = text.to_owned();
    for mark in ["\"resumeOnStart\":false", "\"resumeOnStart\":true"] {
        text = text
            .replace(&format!("{mark},"), "")
            .replace(&format!(",{mark}"), "");
    }
    text
}

/// `dump`, a database as the recorder dumps it (`userVersion`, `schema` and
/// `tables`), without what only Node's ledger held of that mark: its lines in
/// the schema of the `project` table, and what [`hold_apart_the_resume_mark_in_tables`]
/// takes out of the tables. A dump of this ledger's has none of them, and is
/// left as it is.
pub fn hold_apart_the_resume_mark_in_a_dump(dump: &mut Value) {
    if let Some(Value::Array(schema)) = dump.get_mut("schema") {
        for entry in schema.iter_mut().filter(|entry| entry["name"] == "project") {
            if let Some(Value::String(sql)) = entry.get_mut("sql") {
                for line in RESUME_COLUMN_LINES {
                    *sql = sql.replace(line, "");
                }
            }
        }
    }
    if let Some(Value::Object(tables)) = dump.get_mut("tables") {
        hold_apart_the_resume_mark_in_tables(tables);
    }
}

/// `tables`, a database as the recorder dumps it (each table's `columns` and
/// its `rows`), without the column of the `project` table that kept the mark
/// and the mark in the `event` rows that suspended a project at a start.
pub fn hold_apart_the_resume_mark_in_tables(tables: &mut Map<String, Value>) {
    take_column_out(tables, "project", RESUME_COLUMN);
    edit_logged_data(tables, "'project.state'", |text| {
        *text = text.replace(",\"resumeOnStart\":true", "");
    });
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
    fn takes_the_version_past_nodes_and_the_tester_out_of_the_two_checks_and_nothing_else() {
        let checks = |roles: &str| format!("CHECK (role IN ('human', {roles})) STRICT");
        let mut dump = json!({
            "userVersion": 12,
            "schema": [
                { "name": "participant", "sql": checks("'reviewer', 'tester', 'designer'") },
                { "name": "task", "sql": checks("'reviewer', 'tester', 'designer'") },
                { "name": "message", "sql": checks("'reviewer', 'tester', 'designer'") },
            ],
            "tables": {},
        });
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(
            dump,
            json!({
                "userVersion": 11,
                "schema": [
                    { "name": "participant", "sql": checks("'reviewer', 'designer'") },
                    { "name": "task", "sql": checks("'reviewer', 'designer'") },
                    { "name": "message", "sql": checks("'reviewer', 'tester', 'designer'") },
                ],
                "tables": {},
            })
        );
    }

    #[test]
    fn leaves_a_schema_of_nodes_as_it_is() {
        let before = json!({
            "userVersion": 10,
            "schema": [{ "name": "participant", "sql": "CHECK (role IN ('reviewer', 'designer'))" }],
            "tables": {},
        });
        let mut dump = before.clone();
        hold_apart_what_node_never_had_in_its_schema(&mut dump);
        assert_eq!(dump, before);
        let mut tables_only = json!({ "tables": {} });
        hold_apart_what_node_never_had_in_its_schema(&mut tables_only);
        assert_eq!(tables_only, json!({ "tables": {} }));
    }

    #[test]
    fn takes_the_resume_mark_out_of_every_object_of_an_answer_and_nothing_else() {
        let mut answer = json!({
            "id": 1,
            "resumeOnStart": true,
            "projects": [{ "name": "app", "resumeOnStart": false, "gate": false }],
            "data": { "from": "open", "to": "suspended", "resumeOnStart": true },
        });
        hold_apart_the_resume_mark(&mut answer);
        assert_eq!(
            answer,
            json!({
                "id": 1,
                "projects": [{ "name": "app", "gate": false }],
                "data": { "from": "open", "to": "suspended" },
            })
        );
        let mut without = json!({ "gate": true });
        hold_apart_the_resume_mark(&mut without);
        assert_eq!(without, json!({ "gate": true }));
    }

    #[test]
    fn takes_the_resume_mark_out_of_text_with_the_comma_that_joined_it_and_nothing_else() {
        let project = r#"{"id":1,"gate":false,"resumeOnStart":false,"createdAt":"then"}"#;
        assert_eq!(
            without_the_resume_mark(project),
            r#"{"id":1,"gate":false,"createdAt":"then"}"#
        );
        let event = r#"{"from":"open","to":"suspended","resumeOnStart":true}"#;
        assert_eq!(
            without_the_resume_mark(event),
            r#"{"from":"open","to":"suspended"}"#
        );
        let other = r#"{"resumeOnStartAfter":false,"said":"resumeOnStart"}"#;
        assert_eq!(without_the_resume_mark(other), other);
    }

    #[test]
    fn takes_the_resume_mark_out_of_a_dump_its_column_its_lines_of_schema_and_its_events() {
        let project = |lines: &str| {
            format!(
                "CREATE TABLE project (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n{lines}    gate INTEGER NOT NULL DEFAULT 0,\n    CONSTRAINT project_state_check CHECK (state IN ('open', 'suspended')),\n{}    CONSTRAINT project_gate_check CHECK (gate IN (0, 1))\n  ) STRICT",
                if lines.is_empty() { "" } else { RESUME_COLUMN_LINES[1] }
            )
        };
        let suspended = r#"'{"from":"open","to":"suspended","resumeOnStart":true}'"#;
        let opened = r#"'{"from":"suspended","to":"open"}'"#;
        let mut dump = json!({
            "userVersion": 11,
            "schema": [
                { "name": "project", "sql": project(RESUME_COLUMN_LINES[0]) },
                { "name": "task", "sql": project(RESUME_COLUMN_LINES[0]) },
            ],
            "tables": {
                "project": {
                    "columns": ["id", "resume_on_start", "gate"],
                    "rows": [["1", "0", "1"], ["2", "1", "0"]],
                },
                "event": {
                    "columns": ["id", "kind", "data"],
                    "rows": [
                        ["1", "'project.state'", suspended],
                        ["2", "'project.state'", opened],
                        ["3", "'task.state'", suspended],
                    ],
                },
            },
        });
        hold_apart_the_resume_mark_in_a_dump(&mut dump);
        assert_eq!(
            dump,
            json!({
                "userVersion": 11,
                "schema": [
                    { "name": "project", "sql": project("") },
                    { "name": "task", "sql": project(RESUME_COLUMN_LINES[0]) },
                ],
                "tables": {
                    "project": { "columns": ["id", "gate"], "rows": [["1", "1"], ["2", "0"]] },
                    "event": {
                        "columns": ["id", "kind", "data"],
                        "rows": [
                            ["1", "'project.state'", r#"'{"from":"open","to":"suspended"}'"#],
                            ["2", "'project.state'", opened],
                            ["3", "'task.state'", suspended],
                        ],
                    },
                },
            })
        );
        let held = dump.clone();
        hold_apart_the_resume_mark_in_a_dump(&mut dump);
        assert_eq!(dump, held, "a dump of this ledger's is left as it is");
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
