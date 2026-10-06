//! The app's end of the bridge, by hand: a request is written as the line the
//! pane host writes, and the daemon's answer is read as the line it wrote, so a
//! reply is held to Node's as the bytes the bridge carried. A bridge that read
//! the frame into a value and gave the value back would write it again, as
//! compact JSON, whatever the daemon wrote: spaces, escapes and the spelling of
//! a number would not show.

use cf_proto::bridge::{Frame, Role, PROTOCOL_VERSION};
use serde_json::Value;
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

/// What the app reads of the daemon, by line, and what it writes to it.
pub struct Wire {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    output: WriteHalf<DuplexStream>,
    asked: u64,
}

impl Wire {
    pub fn new(input: ReadHalf<DuplexStream>, output: WriteHalf<DuplexStream>) -> Self {
        Self {
            lines: BufReader::new(input).lines(),
            output,
            asked: 0,
        }
    }

    /// Asks the daemon for `op` with `body`: the text of the body of its answer,
    /// as it wrote it. A line that is not the answer to this request is a
    /// failure: nothing but answers comes from a daemon that is asked nothing
    /// else.
    pub async fn ask(&mut self, op: &str, body: &Value) -> Result<String, String> {
        self.asked += 1;
        let id = format!("{}{}", Role::Host.prefix(), self.asked);
        let request = Frame {
            v: PROTOCOL_VERSION,
            id: id.clone(),
            kind: "req".to_owned(),
            op: op.to_owned(),
            body: body.clone(),
        };
        let mut line = serde_json::to_vec(&request).map_err(|why| why.to_string())?;
        line.push(b'\n');
        self.output
            .write_all(&line)
            .await
            .map_err(|why| format!("the bridge took no request: {why}"))?;
        let line = self
            .lines
            .next_line()
            .await
            .map_err(|why| format!("the bridge carried no line: {why}"))?
            .ok_or("the bridge ended before the answer")?;
        let head =
            format!(r#"{{"v":{PROTOCOL_VERSION},"id":"{id}","kind":"res","op":"{op}","body":"#);
        let body = line
            .strip_prefix(&head)
            .and_then(|rest| rest.strip_suffix('}'))
            .ok_or_else(|| format!("the bridge carried {line}, not the answer to {id}"))?;
        // The slice is the frame's body: the line read as a frame says the same.
        let frame: Frame = serde_json::from_str(&line)
            .map_err(|why| format!("the bridge carried {line}, which is no frame: {why}"))?;
        match serde_json::from_str::<Value>(body) {
            Ok(read) if read == frame.body => Ok(body.to_owned()),
            _ => Err(format!(
                "the body of {line} is not what is between its braces"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::{duplex, split};

    use super::*;
    use crate::support::trace::locally;

    /// What the app is told of the request `ping` by a daemon that writes
    /// `line` for it.
    fn asked_of(line: String) -> Result<String, String> {
        locally(async move {
            let (daemon_end, app_end) = duplex(1024);
            let (daemon_input, mut daemon_output) = split(daemon_end);
            let (app_input, app_output) = split(app_end);
            tokio::task::spawn_local(async move {
                let mut requests = BufReader::new(daemon_input).lines();
                let request = requests.next_line().await.unwrap().unwrap();
                let head = r#"{"v":1,"id":"r-1","kind":"req","op":"ping","body":"#;
                assert!(request.starts_with(head), "{request}");
                daemon_output.write_all(line.as_bytes()).await.unwrap();
                daemon_output.write_all(b"\n").await.unwrap();
                std::future::pending::<()>().await;
            });
            Wire::new(app_input, app_output)
                .ask("ping", &json!({}))
                .await
        })
    }

    #[test]
    fn the_answer_is_the_text_of_the_body_as_the_daemon_wrote_it() {
        let frame =
            |body: &str| format!(r#"{{"v":1,"id":"r-1","kind":"res","op":"ping","body":{body}}}"#);
        // What a value read off the bridge and written again would lose: spaces,
        // the spelling of a number, an escape, the order of keys.
        for body in [
            r#"{"ok":true}"#,
            r#" { "ok" : true }"#,
            r#"{"b":1.0,"a":"é\/"}"#,
        ] {
            assert_eq!(asked_of(frame(body)), Ok(body.to_owned()));
        }
    }

    #[test]
    fn a_line_that_is_not_the_answer_to_the_request_is_a_failure() {
        for line in [
            r#"{"v":1,"id":"r-2","kind":"res","op":"ping","body":{}}"#,
            r#"{"v":1,"id":"r-1","kind":"evt","op":"ping","body":{}}"#,
            r#"{"v":1,"id":"r-1","kind":"res","op":"other","body":{}}"#,
            "not a frame",
        ] {
            assert!(asked_of(line.to_owned()).is_err(), "{line}");
        }
    }
}
