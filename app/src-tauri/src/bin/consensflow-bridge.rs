use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;

use app_lib::arbiter::InputArbiter;
use app_lib::bridge::{stdin_is_pipe, BridgeBuilder};
use app_lib::pty::{PaneKey, PaneTable};
use portable_pty::PtySize;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const ENTER_DELAY_MS: u64 = 10;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenRequest {
    cwd: PathBuf,
    argv: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    size: SizeRequest,
    backlog_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SizeRequest {
    rows: u16,
    cols: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaneRequest {
    id: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasteRequest {
    id: String,
    generation: u64,
    epoch: u64,
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckRequest {
    id: String,
    generation: u64,
    seq: u64,
}

fn main() {
    if !stdin_is_pipe() {
        return;
    }
    if let Err(error) = run() {
        eprintln!("consensflow-bridge: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let table = Arc::new(PaneTable::new());
    let (event_sender, _event_receiver) = mpsc::channel();
    let arbiter = Arc::new(InputArbiter::new(ENTER_DELAY_MS, event_sender));
    let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);

    register_open(&mut builder, Arc::clone(&table), Arc::clone(&arbiter));
    register_write_paste(&mut builder, Arc::clone(&table), Arc::clone(&arbiter));
    register_ack(&mut builder, Arc::clone(&table));
    register_list(&mut builder, Arc::clone(&table));
    register_kill(&mut builder, Arc::clone(&table));
    builder.on_error(|error| eprintln!("consensflow-bridge: {error}"));

    let bridge = builder
        .serve(
            std::io::stdin(),
            std::io::stdout(),
            &json!({"v":1,"kind":"consensflow-bridge"}),
        )
        .map_err(|error| error.to_string())?;
    bridge
        .wait_launches_closed()
        .map_err(|error| error.to_string())?;
    reap_all(&table)?;
    bridge.wait_closed().map_err(|error| error.to_string())
}

fn reap_all(table: &PaneTable) -> Result<(), String> {
    let mut first_error = None;
    for pane in table.list().map_err(|error| error.to_string())? {
        if let Err(error) = table.kill(&PaneKey::new(pane.id, pane.generation)) {
            first_error.get_or_insert_with(|| error.to_string());
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn register_open(builder: &mut BridgeBuilder, table: Arc<PaneTable>, arbiter: Arc<InputArbiter>) {
    builder.on_launch("pane.open", move |bridge, body| {
        let request: OpenRequest = parse_body(body)?;
        let streamed = table
            .open_streamed(
                &request.cwd,
                &request.argv,
                &request.env,
                PtySize {
                    rows: request.size.rows,
                    cols: request.size.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                request.backlog_bytes,
            )
            .map_err(|error| error.to_string())?;
        if let Err(error) = arbiter.register(&streamed.key) {
            let _ = table.kill(&streamed.key);
            return Err(error.to_string());
        }

        let key = streamed.key;
        let response_key = key.clone();
        thread::spawn(move || {
            for output in streamed.output {
                match bridge.event(
                    "pane.output",
                    json!({
                        "id":output.key.id,
                        "generation":output.key.generation,
                        "seq":output.seq,
                        "bytes":output.bytes,
                    }),
                ) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => break,
                }
            }
        });
        Ok(json!({
            "ok":true,
            "id":response_key.id,
            "generation":response_key.generation,
        }))
    });
}

fn register_write_paste(
    builder: &mut BridgeBuilder,
    table: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
) {
    builder.on("pane.write_paste", move |_bridge, body| {
        let request: PasteRequest = parse_body(body)?;
        let key = PaneKey::new(request.id, request.generation);
        arbiter
            .write_paste(&table, &key, request.epoch, request.body.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });
}

fn register_ack(builder: &mut BridgeBuilder, table: Arc<PaneTable>) {
    builder.on("pane.ack", move |_bridge, body| {
        let request: AckRequest = parse_body(body)?;
        table
            .ack(&PaneKey::new(request.id, request.generation), request.seq)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });
}

fn register_list(builder: &mut BridgeBuilder, table: Arc<PaneTable>) {
    builder.on("pane.list", move |_bridge, body| {
        let _: EmptyBody = parse_body(body)?;
        let panes = table
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|pane| {
                json!({
                    "id":pane.id,
                    "generation":pane.generation,
                    "alive":pane.alive,
                    "idleMs":pane.idle_ms,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"ok":true,"panes":panes}))
    });
}

fn register_kill(builder: &mut BridgeBuilder, table: Arc<PaneTable>) {
    builder.on("pane.kill", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        table
            .kill(&PaneKey::new(request.id, request.generation))
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

fn parse_body<T: DeserializeOwned>(body: Value) -> Result<T, String> {
    serde_json::from_value(body).map_err(|error| format!("invalid-body: {error}"))
}
