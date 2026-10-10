//! A server of a case's own, on loopback: what a case stands up to be asked by
//! the program under test. A daemon's agents API with a fault in it, to hold the
//! proof of the agents screens to failing; the board's API, with a question it
//! never answers, to hold a supervisor to ending while it waits. HTTP/1.1, one
//! request to a connection; each answer says it closes the connection.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

/// How often a connection that is held open looks at whether the server is gone.
const HELD: Duration = Duration::from_millis(50);

/// What a client asked.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path and query as the request line has them.
    pub target: String,
    /// The header names in lowercase, and their values.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    /// The path of the target, without its query.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }

    /// The value of the query parameter `name`, if the target has one.
    pub fn query(&self, name: &str) -> Option<&str> {
        let (_, query) = self.target.split_once('?')?;
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(given, _)| *given == name)
            .map(|(_, value)| value)
    }

    /// The value of the header `name` (in lowercase).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.as_str())
    }
}

/// What the server answers.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Response {
    /// A JSON answer.
    pub fn json(status: u16, body: &Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
        }
    }

    /// An HTML page.
    pub fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html",
            body,
        }
    }

    /// An answer with a status and nothing else.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: String::new(),
        }
    }
}

/// What the server does with a request.
#[derive(Debug, Clone)]
pub enum Answer {
    /// Answers it.
    Respond(Response),
    /// Never answers it, and keeps the connection open: a door that waits.
    Hold,
}

/// A server on loopback, until it is dropped.
pub struct Server {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts a server on a port it picks, answering each request as `handler`
    /// says.
    pub fn start(handler: impl Fn(&Request) -> Answer + Send + Sync + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let acceptor = {
            let stopped = Arc::clone(&stopped);
            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    if stopped.load(Ordering::SeqCst) {
                        break;
                    }
                    let (handler, stopped) = (Arc::clone(&handler), Arc::clone(&stopped));
                    thread::spawn(move || {
                        let _ = serve(stream, handler.as_ref(), &stopped);
                    });
                }
            })
        };
        Ok(Self {
            address,
            stopped,
            acceptor: Some(acceptor),
        })
    }

    /// Where it listens, as the daemon's handle line says it: with the path.
    pub fn url(&self) -> String {
        format!("http://{}/", self.address)
    }

    /// Where it listens, with no path.
    pub fn origin(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for Server {
    /// The server goes: its acceptor is woken to see it, and the connections it
    /// holds are let go.
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
    }
}

/// Reads one request off `stream`, and answers it.
fn serve(
    stream: TcpStream,
    handler: &(dyn Fn(&Request) -> Answer + Send + Sync),
    stopped: &AtomicBool,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = read_request(&mut reader)? else {
        return Ok(());
    };
    match handler(&request) {
        Answer::Respond(response) => {
            let mut stream = stream;
            write_response(&mut stream, &response)?;
            stream.shutdown(Shutdown::Both)
        }
        Answer::Hold => {
            // Open, and silent, until the client leaves or the server does.
            stream.set_read_timeout(Some(HELD))?;
            let mut one = [0_u8; 1];
            while !stopped.load(Ordering::SeqCst) {
                match reader.get_mut().read(&mut one) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(failed)
                        if matches!(
                            failed.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
            Ok(())
        }
    }
}

/// The request on `reader`: its line, its headers, and a body as long as its
/// `content-length` says. None when the client left before a request.
fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut words = line.split_whitespace();
    let (method, target) = (
        words.next().unwrap_or_default().to_owned(),
        words.next().unwrap_or_default().to_owned(),
    );
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }))
}

/// Writes `response` as an HTTP/1.1 answer that closes the connection.
fn write_response(stream: &mut TcpStream, response: &Response) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {} {reason}\r\n", response.status);
    if response.status != 204 {
        head.push_str(&format!(
            "content-type: {}\r\ncontent-length: {}\r\n",
            response.content_type,
            response.body.len()
        ));
    }
    head.push_str("connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use serde_json::json;

    fn echo() -> Server {
        Server::start(|request| match (request.method.as_str(), request.path()) {
            ("GET", "/hello") => Answer::Respond(Response::json(
                200,
                &json!({ "token": request.query("token"), "bearer": request.header("authorization") }),
            )),
            ("POST", "/items") => {
                Answer::Respond(Response::json(201, &json!({ "got": request.body })))
            }
            ("DELETE", "/items/1") => Answer::Respond(Response::status(204)),
            ("GET", "/page") => Answer::Respond(Response::html("<title>Page</title>".into())),
            ("GET", "/silent") => Answer::Hold,
            _ => Answer::Respond(Response::status(404)),
        })
        .unwrap()
    }

    #[test]
    fn a_request_is_read_whole_and_answered_as_the_handler_says() {
        let server = echo();
        let http = Http::new();
        let hello = http
            .get(
                &format!("{}hello?token=abc&x=1", server.url()),
                Some("secret"),
            )
            .unwrap();
        assert_eq!(hello.status, 200);
        assert_eq!(
            hello.json().unwrap(),
            json!({ "token": "abc", "bearer": "Bearer secret" })
        );
        let created = http
            .send(
                "POST",
                &format!("{}items", server.url()),
                None,
                Some(&json!({ "a": [1] })),
            )
            .unwrap();
        assert_eq!(created.status, 201);
        assert_eq!(created.json().unwrap(), json!({ "got": "{\"a\":[1]}" }));
        let gone = http
            .send("DELETE", &format!("{}items/1", server.url()), None, None)
            .unwrap();
        assert_eq!((gone.status, gone.text.as_str()), (204, ""));
        let page = http.get(&format!("{}page", server.url()), None).unwrap();
        assert_eq!(page.content_type.as_deref(), Some("text/html"));
        assert_eq!(page.text, "<title>Page</title>");
        assert_eq!(
            http.get(&format!("{}nothing", server.url()), None)
                .unwrap()
                .status,
            404
        );
    }

    #[test]
    fn a_request_the_handler_holds_is_never_answered_and_the_server_still_goes() {
        let server = echo();
        let mut held = TcpStream::connect(server.origin().trim_start_matches("http://")).unwrap();
        held.write_all(b"GET /silent HTTP/1.1\r\nhost: x\r\n\r\n")
            .unwrap();
        held.set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut buffer = [0_u8; 16];
        let failed = held.read(&mut buffer).unwrap_err();
        assert!(
            matches!(
                failed.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ),
            "{failed}"
        );
        // Dropped with the connection still held: the server does not wait on it.
        drop(server);
    }
}
