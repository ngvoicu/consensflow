//! One HTTP exchange over a socket of our own, the request line as a trace
//! has it: a client that normalized the target (`/api\whoami`,
//! `//host/api/whoami`, a fragment) or the header (`Bearer` with nothing after
//! it) would not send what Node was sent.
//!
//! The request is written while the answer is read. A server that refuses a
//! request before it has read its body (a body over 2 MiB, a route that is the
//! chief's alone) answers and closes while the client is still writing, and a
//! client that wrote first and read after could lose the answer to the reset.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long an exchange may take before it is a failure of the API under
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
    if let Some(body) = request.body {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
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
