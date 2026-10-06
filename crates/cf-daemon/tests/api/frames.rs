//! How what a connection carries is cut into the requests a client wrote and
//! the answers the API gave, each whole: a head, and as many bytes of body as
//! its `Content-Length` says (an answer with the status 204 or 304 has none,
//! whatever its head says). A connection may carry several of either (`ureq`
//! keeps them alive), so they are told apart by this framing.
//!
//! What cannot be framed is a failure and not a guess: a body sent in chunks,
//! a length that is no number, an answer with no length that is not bodiless
//! (it ends where its connection does), an interim answer, a `HEAD` request
//! (its answer has a length and no body; `cf` makes none). A guess would be
//! held to Node's recording as if it were what had passed.

use crate::front::Reply;

/// A request as the client wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub method: String,
    /// The request line's target, with its query, as written.
    pub target: String,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// The head at the front of some bytes: its first line, its headers (names in
/// lower case) and where its body begins.
struct Head {
    start: String,
    headers: Vec<(String, String)>,
    body_at: usize,
}

impl Head {
    /// The head whole at the front of `pending`: none while it is not.
    fn of(pending: &[u8]) -> Result<Option<Self>, String> {
        let Some(end) = pending.windows(4).position(|window| window == b"\r\n\r\n") else {
            return Ok(None);
        };
        let text = String::from_utf8_lossy(&pending[..end]).into_owned();
        let mut lines = text.split("\r\n");
        let start = lines.next().unwrap_or_default().to_owned();
        let mut headers = Vec::new();
        for line in lines {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| format!("a header with no colon: {line}"))?;
            headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
        }
        Ok(Some(Self {
            start,
            headers,
            body_at: end + 4,
        }))
    }

    /// What the header `name` says: the last, if it is said twice.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .rev()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }

    /// How many bytes of body the head says follow, if it says.
    fn length(&self) -> Result<Option<usize>, String> {
        if let Some(coding) = self.header("transfer-encoding") {
            return Err(format!(
                "a body sent as {coding}, which the relay does not read"
            ));
        }
        self.header("content-length")
            .map(|length| {
                length
                    .parse::<usize>()
                    .map_err(|_| format!("a content length that is no number: {length}"))
            })
            .transpose()
    }
}

/// The requests whole in `pending`, taken off its front: a head, and a body of
/// as many bytes as the head says.
pub fn requests(pending: &mut Vec<u8>) -> Result<Vec<Sent>, String> {
    let mut taken = Vec::new();
    while let Some(head) = Head::of(pending)? {
        let mut start = head.start.splitn(3, ' ');
        let (method, target) = (
            start.next().unwrap_or_default(),
            start.next().unwrap_or_default(),
        );
        if method == "HEAD" {
            return Err(
                "a HEAD request, whose answer has a length and no body: the relay does not read it"
                    .to_owned(),
            );
        }
        let whole = head.body_at + head.length()?.unwrap_or(0);
        if pending.len() < whole {
            break;
        }
        taken.push(Sent {
            method: method.to_owned(),
            target: target.to_owned(),
            authorization: head.header("authorization").map(str::to_owned),
            content_type: head.header("content-type").map(str::to_owned),
            body: pending[head.body_at..whole].to_vec(),
        });
        pending.drain(..whole);
    }
    Ok(taken)
}

/// The answers whole in `pending`, taken off its front: a head, and a body of
/// as many bytes as the head says, none for a status that has none.
pub fn replies(pending: &mut Vec<u8>) -> Result<Vec<Reply>, String> {
    let mut taken = Vec::new();
    while let Some(head) = Head::of(pending)? {
        let status: u16 = head
            .start
            .split(' ')
            .nth(1)
            .and_then(|status| status.parse().ok())
            .ok_or_else(|| format!("a status line the relay cannot read: {}", head.start))?;
        let length = match status {
            100..=199 => {
                return Err(format!(
                    "an interim answer, {status}, which the relay does not read"
                ));
            }
            204 | 304 => 0,
            _ => head.length()?.ok_or_else(|| {
                format!("an answer of {status} with no length, which ends where its connection does: the relay does not read it")
            })?,
        };
        let whole = head.body_at + length;
        if pending.len() < whole {
            break;
        }
        taken.push(Reply {
            status,
            content_type: head.header("content-type").map(str::to_owned),
            body: pending[head.body_at..whole].to_vec(),
        });
        pending.drain(..whole);
    }
    Ok(taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &[u8] = b"POST /api/notes?to=human HTTP/1.1\r\nHost: x\r\nAUTHORIZATION: Bearer abc\r\ncontent-type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
    const WHOAMI: &[u8] = b"GET /api/whoami HTTP/1.1\r\nHost: x\r\n\r\n";

    fn note() -> Sent {
        Sent {
            method: "POST".to_owned(),
            target: "/api/notes?to=human".to_owned(),
            authorization: Some("Bearer abc".to_owned()),
            content_type: Some("application/json".to_owned()),
            body: b"{\"a\":1}".to_vec(),
        }
    }

    #[test]
    fn a_request_is_taken_when_its_head_and_its_body_are_whole_and_not_before() {
        let mut pending = NOTE[..NOTE.len() - 3].to_vec();
        assert_eq!(requests(&mut pending), Ok(Vec::new()));
        assert_eq!(pending.len(), NOTE.len() - 3, "what is not whole is kept");
        pending.extend_from_slice(&NOTE[NOTE.len() - 3..]);
        assert_eq!(requests(&mut pending), Ok(vec![note()]));
        assert!(pending.is_empty());
    }

    #[test]
    fn a_connection_that_carries_requests_one_after_the_other_gives_each() {
        let mut pending = [NOTE, WHOAMI, &NOTE[..20]].concat();
        let taken = requests(&mut pending).unwrap();
        let whoami = Sent {
            method: "GET".to_owned(),
            target: "/api/whoami".to_owned(),
            authorization: None,
            content_type: None,
            body: Vec::new(),
        };
        assert_eq!(taken, vec![note(), whoami]);
        assert_eq!(pending, &NOTE[..20], "the one still coming is kept");
    }

    #[test]
    fn a_body_it_cannot_frame_is_a_failure_and_not_a_guess() {
        for head in [
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked",
            "POST / HTTP/1.1\r\nContent-Length: many",
            "POST / HTTP/1.1\r\nno colon here",
            "HEAD / HTTP/1.1\r\nContent-Length: 0",
        ] {
            let mut pending = format!("{head}\r\n\r\n").into_bytes();
            assert!(requests(&mut pending).is_err(), "{head}");
        }
    }

    /// The answer the daemon gives: its type and its length, and a body.
    fn answer(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nContent-Length: {}\r\ndate: Tue, 06 Oct 2026 10:00:00 GMT\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            content_type: Some("application/json".to_owned()),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn an_answer_is_taken_when_its_head_and_its_body_are_whole_and_not_before() {
        let whole = answer("201 Created", "{\"a\":1}");
        let mut pending = whole[..whole.len() - 3].to_vec();
        assert_eq!(replies(&mut pending), Ok(Vec::new()));
        assert_eq!(pending.len(), whole.len() - 3, "what is not whole is kept");
        pending.extend_from_slice(&whole[whole.len() - 3..]);
        assert_eq!(replies(&mut pending), Ok(vec![reply(201, "{\"a\":1}")]));
        assert!(pending.is_empty());
    }

    #[test]
    fn answers_one_after_the_other_are_each_taken_and_a_status_with_no_body_takes_none() {
        let nothing = b"HTTP/1.1 204 No Content\r\ndate: x\r\n\r\n";
        let none = Reply {
            status: 204,
            content_type: None,
            body: Vec::new(),
        };
        let mut pending = [
            &answer("200 OK", "{}")[..],
            nothing,
            &answer("404 Not Found", "{\"error\":\"x\"}")[..],
            &answer("200 OK", "{\"half\":true}")[..20],
        ]
        .concat();
        assert_eq!(
            replies(&mut pending),
            Ok(vec![
                reply(200, "{}"),
                none,
                reply(404, "{\"error\":\"x\"}")
            ])
        );
        assert_eq!(pending.len(), 20, "the one still coming is kept");
    }

    #[test]
    fn an_answer_it_cannot_frame_is_a_failure_and_not_a_guess() {
        for head in [
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked",
            "HTTP/1.1 200 OK\r\nContent-Length: many",
            "HTTP/1.1 200 OK\r\ncontent-type: application/json",
            "HTTP/1.1 100 Continue\r\nContent-Length: 0",
            "HTTP/1.1 OK\r\nContent-Length: 0",
            "HTTP/1.1 200 OK\r\nno colon here",
        ] {
            let mut pending = format!("{head}\r\n\r\n").into_bytes();
            assert!(replies(&mut pending).is_err(), "{head}");
        }
    }
}
