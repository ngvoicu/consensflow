//! The harnesses that keep a SQLite store (OpenCode, Devin): each fixture's
//! rows written a few at a time into a store of the shape its harness has.

use std::collections::HashMap;
use std::fs;

use cf_proto::agents::Harness;
use rusqlite::Connection;
use serde_json::{json, Value};

use super::rig::{append, fixture_json, half, pieces, upsert, Rig, LATE_MS, STEP_MS};
use crate::records::Options;

/// What OpenCode's growth writes with an event: the row it names, and its table.
type Write = Option<(&'static str, Value)>;

/// OpenCode's events in order, each with the row it names as that event leaves
/// it: the fixture's own row at the row's last event, the event's own account
/// of it before (`opencodeGrowth`).
fn opencode_growth(fixture: &Value, events: &[Value]) -> Vec<(Value, Write)> {
    let session = fixture["session"][0]["id"].as_str().unwrap();
    let mut rows = HashMap::new();
    for table in ["message", "part"] {
        for row in fixture[table].as_array().unwrap() {
            rows.insert(row["id"].as_str().unwrap().to_owned(), (table, row.clone()));
        }
    }
    let mut ordered: Vec<Value> = Vec::new();
    for event in events
        .iter()
        .filter(|event| event["aggregate_id"] == session)
    {
        match ordered.iter().position(|seen| seen["id"] == event["id"]) {
            Some(at) => ordered[at] = event.clone(),
            None => ordered.push(event.clone()),
        }
    }
    ordered.sort_by(|left, right| {
        left["seq"]
            .as_f64()
            .partial_cmp(&right["seq"].as_f64())
            .unwrap()
    });
    // What an event is about: its message's or its part's own account.
    let account = |event: &Value| -> Value {
        let data: Value = serde_json::from_str(event["data"].as_str().unwrap()).unwrap();
        if data["info"].is_object() {
            data["info"].clone()
        } else {
            data["part"].clone()
        }
    };
    let named = |event: &Value| account(event)["id"].as_str().map(str::to_owned);
    let last: HashMap<String, Value> = ordered
        .iter()
        .filter_map(|event| named(event).map(|id| (id, event["id"].clone())))
        .collect();
    ordered
        .into_iter()
        .map(|event| {
            let write = named(&event).and_then(|id| {
                let (table, row) = rows.get(&id)?;
                if last[&id] == event["id"] {
                    return Some((*table, row.clone()));
                }
                let mut told = account(&event);
                let told = told.as_object_mut().unwrap();
                for column in ["id", "sessionID", "messageID"] {
                    told.shift_remove(column);
                }
                let mut earlier = row.clone();
                earlier["data"] = Value::String(Value::Object(told.clone()).to_string());
                Some((*table, earlier))
            });
            (event, write)
        })
        .collect()
}

/// OpenCode's store with a fixture's conversation, and a look after each of
/// its events: how many looks.
pub(super) async fn opencode(name: &str) -> usize {
    let fixture = fixture_json(&format!("opencode/{name}.json"));
    let native = fixture_json("opencode/native-events.json");
    let mut events: Vec<Value> = fixture["event"].as_array().cloned().unwrap_or_default();
    events.extend(native["event"].as_array().unwrap().iter().cloned());
    let session = fixture["session"][0].clone();
    let session_id = session["id"].as_str().unwrap().to_owned();
    let mut rig = Rig::new(name, |root| vec![("XDG_DATA_HOME", root.to_path_buf())]);
    let folder = rig.path(&["opencode"]);
    fs::create_dir_all(&folder).unwrap();
    let writer = Connection::open(folder.join("opencode.db")).unwrap();
    let columns: Vec<String> = session
        .as_object()
        .unwrap()
        .keys()
        .map(|column| {
            format!(
                "\"{column}\" {}",
                if column == "id" {
                    "text primary key"
                } else {
                    ""
                }
            )
        })
        .collect();
    writer
        .execute_batch(&format!(
            "pragma journal_mode = WAL;
            create table session ({});
            create table message (id text primary key, session_id text not null,
              time_created integer not null, time_updated integer not null, data text not null);
            create index message_session_idx on message (session_id, time_created, id);
            create table part (id text primary key, message_id text not null, session_id text not null,
              time_created integer not null, time_updated integer not null, data text not null);
            create index part_session_idx on part (session_id);
            create table event (id text primary key, aggregate_id text not null,
              seq integer not null, type text not null, data text not null);
            create unique index event_aggregate_seq_idx on event (aggregate_id, seq);",
            columns.join(", ")
        ))
        .unwrap();
    upsert(&writer, "session", &session);
    for (event, write) in opencode_growth(&fixture, &events) {
        writer.execute_batch("begin immediate").unwrap();
        if let Some((table, row)) = write {
            upsert(&writer, table, &row);
        }
        upsert(&writer, "event", &event);
        writer.execute_batch("commit").unwrap();
        rig.tick(STEP_MS);
        rig.look(Harness::Opencode, &session_id, &Options::default())
            .await;
    }
    rig.tick(LATE_MS);
    rig.look(Harness::Opencode, &session_id, &Options::default())
        .await;
    writer.close().unwrap();
    rig.finish()
}

/// Devin's store, one row at a time with its main chain following, beside its
/// wire log in line pieces, a look after each, and last the fixture's own
/// main chain: how many looks.
async fn devin(name: &str, session: &str, head: &Value, rows: &[Value], wire: &[Value]) -> usize {
    let mut rig = Rig::new(name, |root| {
        vec![
            ("HOME", root.to_path_buf()),
            ("XDG_DATA_HOME", root.join("data")),
            ("APPDATA", root.join("data")),
            ("CONSENSFLOW_HOME", root.join("home")),
        ]
    });
    let store = rig.path(&["data", "devin", "cli"]);
    fs::create_dir_all(&store).unwrap();
    let launch = rig.path(&["home", "integrations", "devin", "launch-1"]);
    fs::create_dir_all(&launch).unwrap();
    let log = launch.join("wire.jsonl");
    fs::write(&log, "").unwrap();
    let writer = Connection::open(store.join("sessions.db")).unwrap();
    writer
        .execute_batch(
            "create table sessions (id text primary key, main_chain_id integer);
            create table message_nodes (row_id integer primary key autoincrement, session_id text not null,
              node_id integer not null, parent_node_id integer, chat_message text not null,
              created_at integer not null, unique(session_id, node_id));",
        )
        .unwrap();
    writer
        .execute(
            "insert into sessions (id, main_chain_id) values (?, null)",
            [session],
        )
        .unwrap();
    let set_head = |node: &Value| {
        let node = node.as_f64().unwrap();
        writer
            .execute(
                "update sessions set main_chain_id = ? where id = ?",
                rusqlite::params![node, session],
            )
            .unwrap();
    };
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| {
        left["row_id"]
            .as_f64()
            .partial_cmp(&right["row_id"].as_f64())
            .unwrap()
    });
    let lines: Vec<String> = wire.iter().map(Value::to_string).collect();
    let wire_pieces = pieces(&lines);
    let count = ordered.len().max(wire_pieces.len());
    let (mut row, mut piece) = (0, 0);
    for step in 1..=count {
        while row < (step * ordered.len()).div_ceil(count) {
            // The columns the store has: a fixture's row may say more.
            let stored = json!({
                "row_id": ordered[row]["row_id"],
                "session_id": session,
                "node_id": ordered[row]["node_id"],
                "parent_node_id": ordered[row]["parent_node_id"],
                "chat_message": ordered[row]["chat_message"],
                "created_at": ordered[row]["created_at"],
            });
            upsert(&writer, "message_nodes", &stored);
            set_head(&ordered[row]["node_id"]);
            row += 1;
        }
        while piece < (step * wire_pieces.len()).div_ceil(count) {
            append(&log, &wire_pieces[piece]);
            piece += 1;
        }
        rig.tick(STEP_MS);
        rig.look(Harness::Devin, session, &Options::default()).await;
    }
    set_head(head);
    rig.tick(STEP_MS);
    rig.look(Harness::Devin, session, &Options::default()).await;
    writer.close().unwrap();
    rig.finish()
}

/// A Devin session as its fixture holds it: its main chain, its rows, its
/// wire log.
pub(super) async fn devin_fixture(name: &str) -> usize {
    let fixture = fixture_json(&format!("devin/{name}.json"));
    let session = fixture["session"]["id"].as_str().unwrap();
    devin(
        name,
        session,
        &fixture["session"]["main_chain_id"],
        fixture["rows"].as_array().unwrap(),
        fixture["wire"].as_array().unwrap(),
    )
    .await
}

/// A Devin reply that links a file, as `file-link.json` and
/// `file-link-windows.json` hold it: the text its wire log streamed and the
/// text its store kept.
pub(super) async fn devin_file_link(name: &str) -> usize {
    let fixture = fixture_json(&format!("devin/{name}.json"));
    let streamed = fixture["streamed"].as_str().unwrap();
    let half = half(streamed);
    let chunk = |text: &str| {
        json!({
            "sessionId": "calm-river",
            "turnClientMessageId": "request-1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": text },
                "_meta": { "cognition.ai/streamingMessageId": "stream-1" },
            },
        })
    };
    let rows = [
        json!({
            "row_id": 1, "node_id": 1, "parent_node_id": null, "created_at": 1_789_243_781,
            "chat_message": json!({
                "message_id": "u-1", "role": "user", "content": "Review T-1",
                "metadata": { "extensions": { "chisel/client-message-id": "request-1" } },
            }).to_string(),
        }),
        json!({
            "row_id": 2, "node_id": 2, "parent_node_id": 1, "created_at": 1_789_243_782,
            "chat_message": json!({
                "message_id": "a-1", "role": "assistant", "content": fixture["stored"],
            }).to_string(),
        }),
    ];
    let wire = [
        chunk(&streamed[..half]),
        chunk(&streamed[half..]),
        json!({ "sessionId": "calm-river", "turnClientMessageId": "request-1", "cause": "complete" }),
    ];
    devin(name, "calm-river", &json!(2), &rows, &wire).await
}
