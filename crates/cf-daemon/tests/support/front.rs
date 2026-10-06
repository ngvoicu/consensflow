//! The daemon's front over real sockets: what the API under test and the
//! screens are served from ([`Front`]), the screens as the daemon mounts them
//! in front of the API ([`screens`]), and a client of it ([`send`]) that
//! writes one request as the trace has it. A client that normalized the target
//! (`/api\whoami`, `//host/api/whoami`, a fragment) or the header (`Bearer`
//! with nothing after it) would not send what Node was sent.
//!
//! The request is written while the answer is read. A server that refuses a
//! request before it has read its body (a body over 2 MiB, a route that is the
//! chief's alone) answers and closes while the client is still writing, and a
//! client that wrote first and read after could lose the answer to the reset.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_daemon::api::context::{AgentRows, Closing, Context};
use cf_daemon::api::credentials::Credentials;
use cf_daemon::api::Api;
use cf_daemon::roster::Agents;
use cf_daemon::screens::Screens;
use cf_daemon::seams::DaemonSpawn;
use cf_harness::admin::HarnessAdmin;
use cf_harness::testing::{ManualTime, ScriptedCapture, ScriptedLatest, EPOCH_MS};
use cf_ledger::Ledger;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::support::daemon::{executor, Kicks};

/// What a handler of the API is given, over `ledger` and a roster, with the
/// executor its requests run on, and the wake-ups it asked for counted.
pub struct Front {
    pub context: Rc<Context>,
    pub spawn: Rc<DaemonSpawn>,
    kicks: Kicks,
}

impl Front {
    /// The front's parts, with the daemon's log and trace in `folder`.
    pub fn new(folder: &Path, ledger: Rc<RefCell<Ledger>>, roster: Rc<dyn AgentRows>) -> Self {
        let (spawn, log, trace) = executor(folder);
        let kicks = Kicks::new();
        let context = Rc::new(Context {
            ledger,
            credentials: Rc::new(Credentials::new()),
            kick: kicks.waker(),
            closing: Closing::new(),
            roster,
            log,
            trace,
        });
        Self {
            context,
            spawn,
            kicks,
        }
    }

    /// How many times the dispatcher was woken since this was last asked.
    pub fn take_kicks(&self) -> usize {
        self.kicks.take()
    }
}

/// The screens as the daemon mounts them in front of the API (`api::serve`),
/// which `token` opens, over `env` and the saved `agents`; `on_roster_change`
/// is what a change to the roster tells. The harness admin asks no feed and
/// runs no program: its seams are scripted, with nothing scripted, so a request
/// that reached for either would fail and not reach out.
pub fn screens(
    token: &str,
    env: Env,
    agents: Rc<Agents>,
    on_roster_change: Rc<dyn Fn() -> Result<(), String>>,
) -> Rc<Screens> {
    Rc::new(Screens {
        token: token.to_owned(),
        on_roster_change,
        agents,
        admin: HarnessAdmin::new(
            env.clone(),
            Rc::new(ManualTime::new(EPOCH_MS)),
            Rc::new(ScriptedLatest::default()),
            Rc::new(ScriptedCapture::default()),
        ),
        env,
    })
}

/// `127.0.0.1:<port>`, where `api` listens: what a client connects to.
pub fn address(api: &Api) -> String {
    api.url()
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("an address: {}", api.url()))
        .to_owned()
}

/// How long an exchange may take before it is a failure of the front under
/// test: no trace waits for anything for long.
const WAIT: Duration = Duration::from_secs(60);

/// A request as the trace recorded it.
pub struct Request<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub authorization: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub body: Option<&'a [u8]>,
}

/// What came back: its status, the type it carried, and its bytes.
pub struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl Reply {
    /// Why the answer is not the status and the type of Node's `response`.
    pub fn head_differs(&self, response: &Value) -> Option<String> {
        let status = response["status"].as_u64().expect("a status");
        if u64::from(self.status) != status {
            return Some(format!(
                "answered {}, Node {status}: {}",
                self.status,
                String::from_utf8_lossy(&self.body)
            ));
        }
        let kind = response["contentType"].as_str();
        (self.content_type.as_deref() != kind)
            .then(|| format!("answered as {:?}, Node as {kind:?}", self.content_type))
    }
}

/// Sends `request` to `address` (`127.0.0.1:<port>`) on a connection of its
/// own, and reads the answer to its end.
pub async fn send(address: &str, request: Request<'_>) -> io::Result<Reply> {
    let stream = TcpStream::connect(address).await?;
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {address}\r\n",
        request.method, request.target
    );
    if let Some(authorization) = request.authorization {
        head.push_str(&format!("Authorization: {authorization}\r\n"));
    }
    if let Some(content_type) = request.content_type {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    // As `fetch` writes a request: a body has its length, and a verb that takes
    // one says it has none.
    match (request.body, request.method) {
        (Some(body), _) => head.push_str(&format!("Content-Length: {}\r\n", body.len())),
        (None, "POST" | "PUT" | "PATCH") => head.push_str("Content-Length: 0\r\n"),
        (None, _) => {}
    }
    head.push_str("Connection: close\r\n\r\n");
    let body = request.body.map(<[u8]>::to_vec).unwrap_or_default();
    let writing = tokio::task::spawn_local(async move {
        // The server may stop listening before it has all of it: that is
        // what some of the traces are about.
        let _ = writer.write_all(head.as_bytes()).await;
        let _ = writer.write_all(&body).await;
        let _ = writer.flush().await;
        writer
    });
    let mut answered = Vec::new();
    let reading = tokio::time::timeout(WAIT, async {
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => return None,
                Ok(count) => answered.extend_from_slice(&chunk[..count]),
                Err(error) => return Some(error),
            }
        }
    })
    .await;
    writing.abort();
    let broke = match reading {
        Ok(broke) => broke,
        Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in time")),
    };
    // What came before a reset is an answer, if it is a whole one.
    parse(&answered, request.method == "HEAD").ok_or_else(|| {
        broke.unwrap_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no whole answer"))
    })
}

/// The answer in `bytes`, when it is whole: its head, and as many bytes of
/// body as it says (a `HEAD` says how many and sends none).
fn parse(bytes: &[u8], head_only: bool) -> Option<Reply> {
    let end = bytes.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&bytes[..end]);
    let mut lines = head.lines();
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let (mut content_type, mut length) = (None, None);
    for line in lines {
        let (name, value) = line.split_once(':')?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-type" => content_type = Some(value.to_owned()),
            "content-length" => length = value.parse::<usize>().ok(),
            "transfer-encoding" => panic!("chunked, where the front sends a length: {value}"),
            _ => {}
        }
    }
    let rest = &bytes[end + 4..];
    let body = match (head_only, length) {
        (true, _) => Vec::new(),
        (false, Some(length)) if rest.len() >= length => rest[..length].to_vec(),
        (false, Some(_)) => return None,
        (false, None) => rest.to_vec(),
    };
    Some(Reply {
        status,
        content_type,
        body,
    })
}
