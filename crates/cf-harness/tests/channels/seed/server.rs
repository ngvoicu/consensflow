//! A stand-in for an OpenCode window's own server, as the first message meets
//! it: it answers the health poll (`503` until it is told it is ready), reads a
//! conversation's own settings, and takes the task at `prompt_async`, for the
//! password `test-secret`. How it fails is the `Mode`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_harness::opencode::{Bridge, Channel};
use cf_harness::testing::server::{Reply, Request, Server};
use serde_json::{json, Value};
use tokio::task::spawn_local;

/// How the server fails, if it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ok,
    /// Refuses every request.
    Unauthorized,
    /// Takes the first health poll and never answers it.
    HealthHang,
    /// Answers the first health poll's head, then a body that never ends.
    HealthBodyHang,
    /// Cannot read a conversation's settings.
    NativeReadFailure,
    /// Takes the request for a conversation's settings and never answers it.
    NativeReadHang,
    /// Reads a conversation whose model is none.
    InvalidNativeModel,
    /// Reads a conversation on its model's default effort.
    NativeDefault,
    /// Ends the connection of the task it is posted.
    Disconnect,
    /// Takes the task it is posted and never answers it.
    Hang,
}

const PASSWORD: &str = "test-secret";

pub struct Window {
    server: Server,
    healthy: Rc<Cell<bool>>,
}

impl Window {
    pub async fn start(mode: Mode) -> Self {
        let healthy = Rc::new(Cell::new(false));
        let up = Rc::clone(&healthy);
        let probes = Cell::new(0);
        let want = format!("Basic {}", STANDARD.encode(format!("opencode:{PASSWORD}")));
        let server = Server::start(move |request: &Request| {
            if request.header("authorization") != Some(want.as_str()) || mode == Mode::Unauthorized
            {
                return Reply::status(401);
            }
            let path = request.path();
            if path == "/global/health" {
                probes.set(probes.get() + 1);
                let first = probes.get() == 1;
                return match mode {
                    Mode::HealthHang if first => Reply::Hang,
                    Mode::HealthBodyHang if first => Reply::stall(200, "{"),
                    _ => Reply::json(if up.get() { 200 } else { 503 }, &json!({})),
                };
            }
            if request.method == "GET" && path.starts_with("/session/") {
                return match mode {
                    Mode::NativeReadFailure => Reply::json(503, &json!({})),
                    Mode::NativeReadHang => Reply::Hang,
                    _ => Reply::json(200, &conversation(path, mode)),
                };
            }
            match mode {
                Mode::Disconnect => Reply::Drop,
                Mode::Hang => Reply::Hang,
                _ => Reply::status(204),
            }
        })
        .await;
        Self { server, healthy }
    }

    /// From now on the server says it is up.
    pub fn ready(&self) {
        self.healthy.set(true);
    }

    /// The server says it is up once `wait` has passed.
    pub fn ready_in(&self, wait: Duration) {
        let healthy = Rc::clone(&self.healthy);
        spawn_local(async move {
            tokio::time::sleep(wait).await;
            healthy.set(true);
        });
    }

    /// The channel of a window on this server.
    pub fn channel(&self) -> Channel {
        Channel {
            launch_id: String::new(),
            endpoint: self.server.endpoint(),
            password: PASSWORD.to_owned(),
            bridge: Bridge {
                endpoint: String::new(),
                token: String::new(),
            },
        }
    }

    /// The tasks it was posted.
    pub fn posts(&self) -> Vec<Request> {
        self.of(|call| call.method == "POST")
    }

    /// The health polls it was sent.
    pub fn polls(&self) -> Vec<Request> {
        self.of(|call| call.path() == "/global/health")
    }

    /// The reads of a conversation's settings it was sent.
    pub fn reads(&self) -> Vec<Request> {
        self.of(|call| call.method == "GET" && call.path().starts_with("/session/"))
    }

    /// What it was asked, in order, of the kind `wanted` says.
    fn of(&self, wanted: impl Fn(&Request) -> bool) -> Vec<Request> {
        let mut calls = self.server.calls();
        calls.retain(wanted);
        calls
    }
}

/// The settings a conversation of the window's own has: its model and agent.
fn conversation(path: &str, mode: Mode) -> Value {
    let model = match mode {
        Mode::InvalidNativeModel => json!({}),
        Mode::NativeDefault => json!({ "id": "native-model", "providerID": "openrouter" }),
        _ => json!({ "id": "native-model", "providerID": "openrouter", "variant": "medium" }),
    };
    json!({
        "id": path.rsplit('/').next().unwrap_or_default(),
        "agent": "review",
        "model": model,
    })
}
