//! The bridge between the pane host and the daemon: JSON lines on a pair of
//! streams, one frame per line, `{v, id, kind, op, body}`. Each end mints the
//! ids of the requests and events it starts, under its own prefix, so a
//! response is matched to the request that asked and a stray id is refused.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The protocol a frame speaks; a frame of any other version is refused.
pub const PROTOCOL_VERSION: u8 = 1;

/// One line on the bridge. `kind` is `req`, `res` or `evt`; anything else
/// makes the frame malformed, which [`Frame::is_well_formed`] tells.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Frame {
    pub v: u8,
    pub id: String,
    pub kind: String,
    pub op: String,
    pub body: Value,
}

impl Frame {
    /// Whether the frame speaks this protocol, names itself and is of a known kind.
    pub fn is_well_formed(&self) -> bool {
        self.v == PROTOCOL_VERSION
            && !self.id.is_empty()
            && matches!(self.kind.as_str(), "req" | "res" | "evt")
    }
}

/// Which end of the bridge a program is. The pane host mints `r-` ids and the
/// daemon `n-` ones, whatever language each is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Daemon,
}

impl Role {
    /// The prefix of the ids this end mints.
    pub fn prefix(self) -> &'static str {
        match self {
            Role::Host => "r-",
            Role::Daemon => "n-",
        }
    }

    /// The other end.
    pub fn peer(self) -> Role {
        match self {
            Role::Host => Role::Daemon,
            Role::Daemon => Role::Host,
        }
    }
}

/// The answer to a frame too large to carry, in or out.
pub fn too_large_body() -> Value {
    json!({ "ok": false, "error": "too-large" })
}

/// The answer to a request for an op this end does not handle.
pub fn unknown_op_body() -> Value {
    json!({ "ok": false, "error": "unknown-op" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(v: u8, id: &str, kind: &str) -> Frame {
        Frame {
            v,
            id: id.into(),
            kind: kind.into(),
            op: "pane.open".into(),
            body: json!({}),
        }
    }

    #[test]
    fn a_frame_is_well_formed_only_in_this_version_with_an_id_and_a_known_kind() {
        assert!(frame(1, "r-1", "req").is_well_formed());
        assert!(frame(1, "n-7", "res").is_well_formed());
        assert!(frame(1, "n-7", "evt").is_well_formed());
        assert!(!frame(2, "r-1", "req").is_well_formed());
        assert!(!frame(1, "", "req").is_well_formed());
        assert!(!frame(1, "r-1", "event").is_well_formed());
    }

    #[test]
    fn each_end_mints_its_own_prefix_and_knows_the_other() {
        assert_eq!(Role::Host.prefix(), "r-");
        assert_eq!(Role::Daemon.prefix(), "n-");
        assert_eq!(Role::Host.peer(), Role::Daemon);
        assert_eq!(Role::Daemon.peer(), Role::Host);
    }

    #[test]
    fn a_frame_reads_and_writes_as_the_node_end_writes_it() {
        let line =
            r#"{"v":1,"id":"n-3","kind":"evt","op":"state.changed","body":{"reason":"core"}}"#;
        let frame: Frame = serde_json::from_str(line).unwrap();
        assert_eq!(frame.op, "state.changed");
        assert_eq!(serde_json::to_string(&frame).unwrap(), line);
    }
}
