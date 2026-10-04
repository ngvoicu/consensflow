//! What the daemon and a Codex window's supervisor (`cf codex-session`) say
//! to each other: how the daemon names the window's broker
//! (`CF_CODEX_SESSION_BRIDGE`), what the broker says of its window
//! (`GET /session`), and the message the daemon hands it (`POST /deliver`)
//! with the three things the broker may answer.
//!
//! Codex's own JSON-RPC messages are not here: the broker forwards them as the
//! JSON they are.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

/// The environment variable that holds a [`Bridge`] as JSON.
pub const BRIDGE_VARIABLE: &str = "CF_CODEX_SESSION_BRIDGE";

/// The shortest token a broker accepts.
const MIN_TOKEN: usize = 24;

/// How the daemon names a window's broker to its supervisor: the loopback
/// port to listen on (0 for any), the bearer token the daemon and the TUI
/// carry, and the launch the window belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    pub launch_id: String,
    pub port: u16,
    pub token: String,
}

/// A bridge that is missing, no JSON, or not what a broker is configured with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Invalid Codex broker configuration")]
pub struct InvalidBridge;

impl Bridge {
    /// The bridge `text` names. A port is a whole number from 0 to 65535 (the
    /// JSON `1234.0` is one, as it is in JavaScript), a token is any text of at
    /// least 24 characters (UTF-16 units, as JavaScript counted them), and a
    /// launch id is one or more letters, digits, `.`, `_` and `-`.
    pub fn parse(text: &str) -> Result<Self, InvalidBridge> {
        let value: Value = serde_json::from_str(text).map_err(|_| InvalidBridge)?;
        let launch_id = word(value.get("launchId")).ok_or(InvalidBridge)?;
        let token = value
            .get("token")
            .and_then(Value::as_str)
            .filter(|token| token.encode_utf16().count() >= MIN_TOKEN)
            .ok_or(InvalidBridge)?;
        let port = value
            .get("port")
            .and_then(Value::as_f64)
            .filter(|port| port.fract() == 0.0 && (0.0..=65535.0).contains(port))
            .ok_or(InvalidBridge)?;
        Ok(Self {
            launch_id: launch_id.to_string(),
            // A whole number in range, as checked.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            port: port as u16,
            token: token.to_string(),
        })
    }
}

/// `value` when it is a non-empty string of `[A-Za-z0-9._-]`.
fn word(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}

/// What a broker says of its window to `GET /session`: the thread the
/// window's TUI shows (none while it starts or switches threads), whether
/// that thread has had nothing said in it yet, and whether a delivery would
/// be taken now. The daemon holds a message while `available` is false
/// instead of spending its attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub launch_id: String,
    pub session_id: Option<String>,
    /// Counts every change of the window's thread, so a stale answer is told from the latest.
    pub revision: u64,
    pub empty: bool,
    pub available: bool,
}

/// What the daemon hands a broker at `POST /deliver`: the text for the
/// thread `session_id`, until `expires_at` (milliseconds since the epoch).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub launch_id: String,
    pub session_id: String,
    pub text: String,
    pub expires_at: f64,
}

/// Why a broker took nothing: what it answers when no byte went in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refusal {
    Unauthorized,
    InvalidRecord,
    Expired,
    /// No thread is shown now, or the broker cannot reach Codex's server.
    NativeSessionUnavailable,
    /// The window shows another thread than the one named.
    NativeSessionChanged,
}

/// A broker's answer to a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryReply {
    /// Codex took it.
    Admitted,
    /// Nothing was sent: the daemon may route the message again.
    Refused(Refusal),
    /// Codex may have it: never retried.
    Uncertain,
}

impl Serialize for DeliveryReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Admitted => {
                let mut reply = serializer.serialize_struct("DeliveryReply", 2)?;
                reply.serialize_field("ok", &true)?;
                reply.serialize_field("admitted", &true)?;
                reply.end()
            }
            Self::Refused(reason) => {
                let mut reply = serializer.serialize_struct("DeliveryReply", 4)?;
                reply.serialize_field("ok", &false)?;
                reply.serialize_field("admitted", &false)?;
                reply.serialize_field("bytesWritten", &0)?;
                reply.serialize_field("error", reason)?;
                reply.end()
            }
            Self::Uncertain => {
                let mut reply = serializer.serialize_struct("DeliveryReply", 3)?;
                reply.serialize_field("ok", &false)?;
                reply.serialize_field("admitted", &None::<bool>)?;
                reply.serialize_field("error", "uncertain")?;
                reply.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "private-launch-token-1234567890";

    fn bridge(extra: &Value) -> String {
        let mut value = json!({ "launchId": "launch-1", "port": 4100, "token": TOKEN });
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        value.to_string()
    }

    #[test]
    fn reads_the_bridge_the_daemon_writes() {
        assert_eq!(
            Bridge::parse(&bridge(&json!({}))),
            Ok(Bridge {
                launch_id: "launch-1".into(),
                port: 4100,
                token: TOKEN.into()
            })
        );
        // Any port, 0 and 65535 included; JSON's 4100.0 is a whole number too.
        for port in [json!(0), json!(65535), json!(4100.0), json!(4.1e3)] {
            assert!(Bridge::parse(&bridge(&json!({ "port": port }))).is_ok());
        }
    }

    #[test]
    fn refuses_a_bridge_that_is_missing_or_not_what_a_broker_needs() {
        let bad = [
            "".to_string(),
            "{}".to_string(),
            "null".to_string(),
            "[1]".to_string(),
            "{ not json".to_string(),
            bridge(&json!({ "port": -1 })),
            bridge(&json!({ "port": 65536 })),
            bridge(&json!({ "port": 41.5 })),
            bridge(&json!({ "port": "4100" })),
            bridge(&json!({ "port": null })),
            bridge(&json!({ "token": "short-token" })),
            bridge(&json!({ "token": 12 })),
            bridge(&json!({ "launchId": "" })),
            bridge(&json!({ "launchId": "has/slash" })),
            bridge(&json!({ "launchId": 7 })),
        ];
        for text in bad {
            assert_eq!(Bridge::parse(&text), Err(InvalidBridge), "{text}");
        }
        assert_eq!(
            InvalidBridge.to_string(),
            "Invalid Codex broker configuration"
        );
    }

    #[test]
    fn a_token_is_any_text_long_enough_as_javascript_checked_it() {
        // A launch id keeps to its letters; a token does not (base64's +, / and = among them).
        for token in [
            "q83vEjRWeJ+rze8SNFZ4mg/s3e8SNFZ4mg==".to_string(),
            format!("{TOKEN} with spaces"),
        ] {
            let parsed = Bridge::parse(&bridge(&json!({ "token": token }))).unwrap();
            assert_eq!(parsed.token, token);
        }
        // Counted as JavaScript counted it: in UTF-16 units, so 12 emoji are 24.
        let emoji = "\u{1F600}".repeat(12);
        assert!(Bridge::parse(&bridge(&json!({ "token": emoji }))).is_ok());
    }

    #[test]
    fn a_token_of_exactly_24_characters_is_enough() {
        let token = "a".repeat(24);
        assert!(Bridge::parse(&bridge(&json!({ "token": token }))).is_ok());
        let short = "a".repeat(23);
        assert!(Bridge::parse(&bridge(&json!({ "token": short }))).is_err());
    }

    #[test]
    fn the_session_view_is_written_with_its_fields_in_order() {
        let session = Session {
            launch_id: "launch-1".into(),
            session_id: None,
            revision: 3,
            empty: false,
            available: false,
        };
        assert_eq!(
            serde_json::to_string(&session).unwrap(),
            r#"{"launchId":"launch-1","sessionId":null,"revision":3,"empty":false,"available":false}"#
        );
        let shown = Session {
            session_id: Some("01a09094-938f-7fd1-a2d3-315cf92b4559".into()),
            empty: true,
            available: true,
            ..session
        };
        assert_eq!(
            serde_json::to_string(&shown).unwrap(),
            r#"{"launchId":"launch-1","sessionId":"01a09094-938f-7fd1-a2d3-315cf92b4559","revision":3,"empty":true,"available":true}"#
        );
    }

    #[test]
    fn reads_a_delivery_and_ignores_what_it_does_not_know() {
        let delivery: Delivery = serde_json::from_value(json!({
            "launchId": "launch-1",
            "sessionId": "01a09094-938f-7fd1-a2d3-315cf92b4559",
            "text": "complete\nworker result",
            "expiresAt": 1_780_000_000_000_u64,
            "extra": true,
        }))
        .unwrap();
        assert_eq!(delivery.text, "complete\nworker result");
        assert_eq!(delivery.expires_at, 1_780_000_000_000.0);
        for missing in ["launchId", "sessionId", "text", "expiresAt"] {
            let mut value = json!({
                "launchId": "l", "sessionId": "s", "text": "t", "expiresAt": 1,
            });
            value.as_object_mut().unwrap().remove(missing);
            assert!(
                serde_json::from_value::<Delivery>(value).is_err(),
                "{missing}"
            );
        }
        assert!(serde_json::from_value::<Delivery>(json!({
            "launchId": "l", "sessionId": "s", "text": "t", "expiresAt": "soon",
        }))
        .is_err());
    }

    #[test]
    fn answers_a_delivery_in_the_three_shapes_the_daemon_reads() {
        let written = |reply| serde_json::to_string(&reply).unwrap();
        assert_eq!(
            written(DeliveryReply::Admitted),
            r#"{"ok":true,"admitted":true}"#
        );
        assert_eq!(
            written(DeliveryReply::Refused(Refusal::NativeSessionChanged)),
            r#"{"ok":false,"admitted":false,"bytesWritten":0,"error":"native-session-changed"}"#
        );
        assert_eq!(
            written(DeliveryReply::Uncertain),
            r#"{"ok":false,"admitted":null,"error":"uncertain"}"#
        );
    }

    #[test]
    fn names_each_refusal_as_the_daemon_knows_it() {
        let named = |reason| serde_json::to_value(reason).unwrap();
        assert_eq!(named(Refusal::Unauthorized), json!("unauthorized"));
        assert_eq!(named(Refusal::InvalidRecord), json!("invalid-record"));
        assert_eq!(named(Refusal::Expired), json!("expired"));
        assert_eq!(
            named(Refusal::NativeSessionUnavailable),
            json!("native-session-unavailable")
        );
        assert_eq!(
            named(Refusal::NativeSessionChanged),
            json!("native-session-changed")
        );
    }
}
