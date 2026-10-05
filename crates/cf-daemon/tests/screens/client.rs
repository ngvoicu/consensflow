//! A client of the API as small as it can be: one request, written as the
//! trace has it (its target exactly as recorded, its headers as given, its body
//! the bytes it was), on a connection that is its own and is read to its end.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// What the server answered.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// What one request is made of, as a trace records it.
pub struct Ask<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub authorization: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub body: Option<&'a [u8]>,
}

/// The answer of the server at `address` (`host:port`) to `ask`. The body is
/// written while the answer is read: a server that answers before it has all
/// of a body it refuses must be heard as it says it.
pub async fn send(address: &str, ask: &Ask<'_>) -> Reply {
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut read, mut write) = stream.into_split();
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {address}\r\n",
        ask.method, ask.target
    );
    if let Some(authorization) = ask.authorization {
        head.push_str(&format!("Authorization: {authorization}\r\n"));
    }
    if let Some(content_type) = ask.content_type {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    // As `fetch` writes a request: a body has its length, and a verb that takes one says none.
    match (ask.body, ask.method) {
        (Some(body), _) => head.push_str(&format!("Content-Length: {}\r\n", body.len())),
        (None, "POST" | "PUT" | "PATCH") => head.push_str("Content-Length: 0\r\n"),
        (None, _) => {}
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut request = head.into_bytes();
    request.extend_from_slice(ask.body.unwrap_or_default());
    let writing = tokio::task::spawn_local(async move {
        // A server that has answered and closed is no failure of the request.
        let _ = write.write_all(&request).await;
        write
    });
    let mut answered = Vec::new();
    let reading = read.read_to_end(&mut answered).await;
    drop(writing);
    // What came is the answer, whether or not the connection ended well.
    let _ = reading;
    parse(&answered)
}

fn parse(bytes: &[u8]) -> Reply {
    let at = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no head in {:?}", String::from_utf8_lossy(bytes)));
    let head = String::from_utf8_lossy(&bytes[..at]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    let header = |name: &str| {
        lines.clone().find_map(|line| {
            let (found, value) = line.split_once(':')?;
            found
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    assert!(
        header("transfer-encoding").is_none(),
        "an answer with its length is what the server writes: {head:?}"
    );
    Reply {
        status,
        content_type: header("content-type"),
        body: bytes[at + 4..].to_vec(),
    }
}
