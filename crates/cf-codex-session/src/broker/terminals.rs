//! The commands Codex keeps running for the window's thread, as the daemon asks for them
//! (`GET /terminals`) before it closes a window that did not stop for a pause: Codex runs
//! a command with no shell, and keeps one a pause interrupted as a background terminal, so
//! that the host's tree names none of them as the turn's work, and a close would end them
//! unsaid (T-1137). Codex's own list of them is its server's
//! (`thread/backgroundTerminals/list`, what its TUI counts as "N background terminals
//! running"); its rollout records a command only once it has ended.

use cf_proto::codex::Terminals;
use serde_json::{json, Value};

use super::control::Asked;
use super::{now_ms, Shared};

/// How long Codex's server is given to answer.
const ANSWER_MS: f64 = 3000.0;
/// The most terminals asked for: more than any window runs, and what a note quotes is fewer.
const LIMIT: u32 = 100;

impl Shared {
    /// The commands Codex keeps running for the thread the window shows: none while it shows
    /// none; nothing where Codex's server could not be asked, or answered what is no list.
    pub(super) async fn terminals(&self) -> Option<Terminals> {
        let thread = self.state.borrow().session_id().map(str::to_owned);
        let launch_id = self.bridge.launch_id.clone();
        let Some(thread) = thread else {
            return Some(Terminals {
                launch_id,
                commands: Vec::new(),
            });
        };
        let asked = self
            .request(
                "thread/backgroundTerminals/list",
                json!({ "threadId": thread, "limit": LIMIT }),
                now_ms() + ANSWER_MS,
            )
            .await;
        let Asked::Answered(answer) = asked else {
            return None;
        };
        let commands = answer
            .get("result")?
            .get("data")?
            .as_array()?
            .iter()
            .map(|terminal| {
                terminal
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Terminals {
            launch_id,
            commands,
        })
    }
}
