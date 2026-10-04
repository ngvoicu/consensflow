//! A delivery: the daemon's message for a thread, checked once more against
//! what the window shows and put to Codex's server.
//!
//! An idle thread gets the message as its turn: Codex's queue drains only when
//! a running turn ends, and after an interrupt it kept a message for good (a
//! tell to a paused worker, 2026-09-26). While a turn runs, the queue is
//! right: the message goes in when the turn ends.

use cf_base::js;
use cf_base::json::from_slice_lossy;
use cf_proto::codex::{Delivery, DeliveryReply, Refusal};
use serde_json::json;

use super::selection::is_thread_id;
use super::{now_ms, Shared};

/// The most text a delivery may carry, in bytes.
const MAX_TEXT: usize = 64 * 1024;
/// The longest the broker takes to have Codex's answer, whatever the deadline.
const ADMISSION_MS: f64 = 3000.0;

/// The delivery in `body` for this window, when it is one: JSON of the launch's
/// own id, a thread id, some text of at most 64 KiB, and a deadline.
pub(super) fn read(body: &[u8], launch_id: &str) -> Option<Delivery> {
    let delivery: Delivery = serde_json::from_value(from_slice_lossy(body).ok()?).ok()?;
    (delivery.launch_id == launch_id
        && is_thread_id(&delivery.session_id)
        && !delivery.text.is_empty()
        && delivery.text.len() <= MAX_TEXT
        && delivery.expires_at.is_finite())
    .then_some(delivery)
}

impl Shared {
    /// Puts `delivery` to Codex's server if the window still shows the thread
    /// it names: taken, refused (nothing was sent, so the daemon may route it
    /// again), or uncertain (it may be in: never sent twice).
    pub(super) async fn deliver(&self, delivery: &Delivery) -> DeliveryReply {
        if delivery.expires_at <= now_ms() {
            return DeliveryReply::Refused(Refusal::Expired);
        }
        let control_open = self.control_open();
        // No `await` between this check and the request that follows it: a
        // switch of the window can only retire later deliveries, never
        // replay this one.
        let admission = match self
            .state
            .borrow_mut()
            .admit(&delivery.session_id, control_open)
        {
            Ok(admission) => admission,
            Err(refusal) => return DeliveryReply::Refused(refusal),
        };
        let input = json!([{ "type": "text", "text": delivery.text, "text_elements": [] }]);
        let deadline = delivery.expires_at.min(now_ms() + ADMISSION_MS);
        let queue = || {
            json!({
                "threadId": admission.thread,
                "input": input,
                "clientUserMessageId": uuid::Uuid::new_v4().to_string(),
            })
        };
        // Every request names the thread this delivery was checked against,
        // never whatever the window shows by the time an answer comes.
        let mut answer = if admission.idle {
            self.request(
                "turn/start",
                json!({ "threadId": admission.thread, "input": input }),
                deadline,
            )
            .await
        } else {
            self.request("thread/queue/add", queue(), deadline).await
        };
        // A turn the TUI started a moment before is an explicit refusal: queue
        // it, unless the window has moved to another thread meanwhile. Nothing
        // went in, so the daemon may route the message again.
        if admission.idle && js::truthy(answer.as_ref().and_then(|answer| answer.get("error"))) {
            if !self.state.borrow().is_current(&admission) {
                return DeliveryReply::Refused(Refusal::NativeSessionChanged);
            }
            answer = self.request("thread/queue/add", queue(), deadline).await;
        }
        if js::truthy(answer.as_ref().and_then(|answer| answer.get("result"))) {
            DeliveryReply::Admitted
        } else {
            DeliveryReply::Uncertain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";

    fn body(overrides: Value) -> Vec<u8> {
        let mut record = json!({
            "launchId": "launch-1",
            "sessionId": A,
            "text": "complete\nworker result",
            "expiresAt": 1_780_000_000_000_u64,
        });
        for (key, value) in overrides.as_object().unwrap() {
            record[key] = value.clone();
        }
        record.to_string().into_bytes()
    }

    #[test]
    fn reads_a_delivery_for_this_launch() {
        let delivery = read(&body(json!({})), "launch-1").unwrap();
        assert_eq!(delivery.session_id, A);
        assert_eq!(delivery.text, "complete\nworker result");
        // A deadline is any number: a fraction, the past, a far future.
        for expires in [json!(1.5), json!(-1), json!(0), json!(4e15)] {
            assert!(read(&body(json!({ "expiresAt": expires })), "launch-1").is_some());
        }
    }

    #[test]
    fn refuses_what_is_not_a_delivery_for_this_launch() {
        for overrides in [
            json!({ "launchId": "foreign" }),
            json!({ "launchId": null }),
            json!({ "sessionId": "not-a-uuid" }),
            json!({ "sessionId": null }),
            json!({ "sessionId": 7 }),
            json!({ "text": "" }),
            json!({ "text": 7 }),
            json!({ "text": null }),
            json!({ "text": "x".repeat(MAX_TEXT + 1) }),
            json!({ "expiresAt": "soon" }),
            json!({ "expiresAt": null }),
        ] {
            assert_eq!(
                read(&body(overrides.clone()), "launch-1"),
                None,
                "{overrides}"
            );
        }
        for text in [
            "",
            "{",
            "null",
            "[]",
            "5",
            "\"x\"",
            "{\"launchId\":\"launch-1\"}",
        ] {
            assert_eq!(read(text.as_bytes(), "launch-1"), None, "{text:?}");
        }
    }

    #[test]
    fn the_text_limit_is_in_utf8_bytes() {
        let at = "é".repeat(MAX_TEXT / 2);
        assert_eq!(at.len(), MAX_TEXT);
        assert!(read(&body(json!({ "text": at })), "launch-1").is_some());
        let over = format!("{}x", "é".repeat(MAX_TEXT / 2));
        assert_eq!(read(&body(json!({ "text": over })), "launch-1"), None);
        assert!(read(&body(json!({ "text": "x".repeat(MAX_TEXT) })), "launch-1").is_some());
    }

    #[test]
    fn a_half_character_and_bytes_that_are_no_utf8_read_as_the_replacement_character() {
        let lone = format!(
            r#"{{"launchId":"launch-1","sessionId":"{A}","text":"half {}ud83d","expiresAt":1}}"#,
            '\\'
        );
        assert_eq!(
            read(lone.as_bytes(), "launch-1").unwrap().text,
            "half \u{FFFD}"
        );
        let mut raw = body(json!({ "text": "ab" }));
        let at = raw.windows(2).position(|pair| pair == b"ab").unwrap();
        raw[at] = 0xFF;
        assert_eq!(read(&raw, "launch-1").unwrap().text, "\u{FFFD}b");
    }
}
