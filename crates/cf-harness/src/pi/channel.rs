//! The channel to ConsensFlow's extension inside a Pi window: the folders a
//! launch's messages and evidence go through, how the window is told of them,
//! and which conversation the extension says the window shows. A message is a
//! file in the extension's inbox (`send`).

mod send;

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::read_file;
use cf_base::json::from_slice_lossy;
use cf_base::path;

use crate::contract::{LaunchId, Pane, PaneHost};
use crate::shared::launch_files::launch_folder;

pub use send::{send, Target};
// For the cases that call the channel (`test-support`).
#[cfg(feature = "test-support")]
pub use send::Answer;

/// How long the extension has to give its verdict on a message, from the
/// moment the message was written: the message's whole life.
const ACK_TIMEOUT_MS: u64 = 30_000;

/// A launch's channel: the folders of its launch, in `integrations/pi/<launch>`
/// in ConsensFlow's folder, and the launch's id.
#[derive(Debug, Clone)]
pub(super) struct Channel {
    pub(super) launch_id: String,
    pub(super) inbox: String,
    pub(super) ack: String,
    /// Where the extension writes its evidence of the window's turns and of
    /// the conversation it shows.
    pub(super) settled: String,
}

/// Bytes as lowercase hex, as `Buffer.toString('hex')` writes them.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// How a Pi window is opened on its launch's channel.
pub(super) struct Launched {
    pub(super) args: Vec<String>,
    /// The environment the extension is told its folders by, in the order
    /// JavaScript built it.
    pub(super) env: Vec<(String, String)>,
    pub(super) channel: Channel,
}

/// The arguments and environment that make Pi load the extension at
/// `extension` and tell it where its launch's folders are. No folder is made
/// here: the extension and the channel's send make their own.
///
/// Kept from Node on purpose: the workspace is only checked for being
/// something. Node also made it whole against the working folder
/// (`path.resolve`), which fails only where that folder is gone, and used it
/// for no harness but Codex.
pub(super) fn launch_configuration(
    env: &Env,
    launch: &LaunchId,
    workspace: &str,
    extension: &str,
) -> Result<Launched, String> {
    if workspace.is_empty() {
        return Err("launch configuration needs a workspace".to_owned());
    }
    let root = launch_folder("pi", env, launch)?;
    let folder = |name: &str| path::join(&[&root, name]);
    let channel = Channel {
        launch_id: launch.as_str().to_owned(),
        inbox: folder("inbox"),
        ack: folder("ack"),
        settled: folder("settled"),
    };
    let environment = [
        ("CF_DELIVERY_INBOX", channel.inbox.clone()),
        ("CF_DELIVERY_ACK", channel.ack.clone()),
        ("CF_DELIVERY_QUARANTINE", folder("quarantine")),
        ("CF_DELIVERY_SETTLED", channel.settled.clone()),
        ("CF_DELIVERY_EXPIRED", folder("expired")),
        ("CF_DELIVERY_LAUNCH_ID", channel.launch_id.clone()),
    ];
    Ok(Launched {
        args: vec!["--extension".to_owned(), extension.to_owned()],
        env: environment
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        channel,
    })
}

impl Channel {
    /// What a send of a message to the extension is addressed by: this
    /// channel, the conversation the message is for, and the pane it is
    /// claimed through.
    pub(super) fn target<'a>(
        &'a self,
        session: &'a str,
        pane: &'a Pane,
        host: &'a dyn PaneHost,
    ) -> Target<'a> {
        Target {
            launch_id: &self.launch_id,
            inbox: &self.inbox,
            ack: &self.ack,
            ack_timeout_ms: ACK_TIMEOUT_MS,
            session,
            pane,
            host,
        }
    }

    /// The conversation the extension says the window shows (it writes it
    /// whenever it starts on one), or none before it has said
    /// (`shownSession`). A file that is not there, is no JSON, or is another
    /// launch's says nothing; one the system will not read is a failure, in
    /// Node's words.
    pub(super) fn shown_session(&self) -> Result<Option<String>, String> {
        let file = path::join(&[&self.settled, &format!("{}.shown.json", self.launch_id)]);
        match read_file(Path::new(&file)) {
            Ok(bytes) => Ok(from_slice_lossy(&bytes).ok().and_then(|shown| {
                let ours = shown.get("launchId")?.as_str()? == self.launch_id;
                let session = shown.get("sessionId")?.as_str()?;
                ours.then(|| session.to_owned())
            })),
            Err(failed) if failed.code() == "ENOENT" => Ok(None),
            Err(failed) => Err(failed.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
