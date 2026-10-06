//! A client of the daemon's API, as bytes on a socket: the test writes a
//! request's head and body in the pieces it chooses and reads the reply.

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::api::Api;

/// A client on a connection of its own.
pub struct Client {
    stream: TcpStream,
    /// What was read beyond the last reply: the start of the next.
    read: Vec<u8>,
}

impl Client {
    /// Connects to the API's address.
    pub async fn connect(api: &Api) -> Self {
        let address = api.url().strip_prefix("http://").expect("an address");
        let stream = TcpStream::connect(address).await.expect("a connection");
        stream.set_nodelay(true).expect("no delay");
        Self {
            stream,
            read: Vec::new(),
        }
    }

    /// Writes `bytes` to the connection: what the daemon's reader finds
    /// when it is next polled.
    pub async fn send(&mut self, bytes: &[u8]) {
        self.stream
            .write_all(bytes)
            .await
            .expect("the daemon reads");
    }

    /// The reply: its status and its body, by its length.
    pub async fn reply(&mut self) -> (u16, String) {
        let mut read = std::mem::take(&mut self.read);
        let mut chunk = [0; 4096];
        let end = loop {
            if let Some(at) = read.windows(4).position(|window| window == b"\r\n\r\n") {
                break at + 4;
            }
            let count = self.stream.read(&mut chunk).await.expect("a reply");
            assert!(count > 0, "the connection ended with no reply");
            read.extend_from_slice(&chunk[..count]);
        };
        let head = String::from_utf8_lossy(&read[..end]).into_owned();
        let status = head.split(' ').nth(1).and_then(|code| code.parse().ok());
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(": ")?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.parse::<usize>().ok())?
            })
            .unwrap_or(0);
        while read.len() < end + length {
            let count = self.stream.read(&mut chunk).await.expect("a body");
            assert!(count > 0, "the connection ended in the middle of a body");
            read.extend_from_slice(&chunk[..count]);
        }
        self.read = read.split_off(end + length);
        (
            status.expect("a status"),
            String::from_utf8_lossy(&read[end..]).into_owned(),
        )
    }
}

/// The head of a request with a body of `length` bytes.
pub fn head(path: &str, length: usize) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: t\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n"
    )
}

/// A whole request to `path` about `project`.
pub fn request(path: &str, project: i64) -> String {
    let body = json!({ "project": project }).to_string();
    format!("{}{body}", head(path, body.len()))
}
