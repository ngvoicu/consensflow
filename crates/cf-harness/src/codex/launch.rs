//! What a Codex window is launched with: the native queue its CLI must have
//! (`requireNativeQueue`, `src/channels.js`), the port and token of the broker
//! its supervisor serves (the `codex` branch of `launchConfiguration`), and
//! the supervisor that opens the window (`withNativeBridge`).

use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use cf_base::js;
use serde_json::json;

use super::channel::Channel;
use crate::contract::LaunchId;
use crate::seams::processes::{probe, Probed, Unanswered};
use crate::seams::{Bundle, Services};

/// How a Codex window is opened on its launch's broker.
pub(super) struct Launched {
    /// The environment the supervisor is told its broker by.
    pub(super) env: Vec<(String, String)>,
    pub(super) channel: Channel,
}

/// What the CLI at `path` answers to `args`.
async fn ask(services: &Services, path: &Path, args: &[&str]) -> Result<Probed, Unanswered> {
    probe(
        &services.probes,
        &services.processes,
        path,
        args,
        &services.env,
    )
    .await
}

/// A Codex window runs under ConsensFlow's supervisor, which needs the native
/// queue (and the app-server and remote TUI that came with it): a Codex
/// without it cannot be reached in its window, so it is refused by its
/// version. Detection asks the resolved executable, never a second CLI from
/// PATH.
async fn require_native_queue(services: &Services, executable: &str) -> Result<(), String> {
    let path = Path::new(executable);
    if !path.is_absolute() {
        return Err("a Codex launch needs the absolute path of its CLI".to_owned());
    }
    let help = ask(services, path, &["queue", "--help"])
        .await
        .map_err(|unanswered| match unanswered {
            // Node threw this one where it asked, before any run.
            Unanswered::Unread(sentence) => sentence,
            Unanswered::Failed(failed) => format!(
                "could not ask Codex whether it has its native queue: {}",
                if failed.killed {
                    "it did not answer in time"
                } else {
                    &failed.message
                }
            ),
        })?;
    if ends_a_word(&help.stdout, "--thread") && ends_a_word(&help.stdout, "--message") {
        return Ok(());
    }
    let version = match ask(services, path, &["--version"]).await {
        Ok(asked) => version_in(&asked.stdout).map(str::to_owned),
        Err(Unanswered::Unread(sentence)) => return Err(sentence),
        Err(Unanswered::Failed(_)) => None,
    };
    let codex = version.map_or_else(
        || "This Codex".to_owned(),
        |number| format!("Codex {number}"),
    );
    Err(format!(
        "{codex} has no native queue, which ConsensFlow needs to reach its window: update Codex."
    ))
}

/// Whether `text` holds `flag` at the end of a word, as `/flag\b/` finds it: a
/// word is what JavaScript's `\w` takes for one, ASCII letters and digits and
/// the underscore.
fn ends_a_word(text: &str, flag: &str) -> bool {
    text.match_indices(flag).any(|(at, _)| {
        let after = text[at + flag.len()..].chars().next();
        !after.is_some_and(|next| next.is_ascii_alphanumeric() || next == '_')
    })
}

/// The first version number in `text`, with what follows it up to a space, as
/// `/\d+\.\d+\.\d+\S*/` finds it: JavaScript's digits are ASCII and its
/// spaces are its own (`js::is_space`).
fn version_in(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .filter(|&at| bytes[at].is_ascii_digit())
        .find_map(|at| {
            let mut end = at;
            for part in 0..3 {
                if part > 0 {
                    if bytes.get(end) != Some(&b'.') {
                        return None;
                    }
                    end += 1;
                }
                let digits = bytes[end..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                if digits == 0 {
                    return None;
                }
                end += digits;
            }
            let stop = text[end..]
                .find(js::is_space)
                .map_or(text.len(), |space| end + space);
            Some(&text[at..stop])
        })
}

/// The broker a launch's window is given: a free port on loopback and a token
/// of 24 drawn bytes, and the environment that tells the supervisor of them.
/// The CLI is asked for its native queue first.
///
/// Kept from Node on purpose: the workspace is only checked for being
/// something. Node also made it whole against the working folder
/// (`path.resolve`), which fails only where that folder is gone, and kept it
/// in the channel, where nothing reads it.
pub(super) async fn configuration(
    services: &Services,
    launch: &LaunchId,
    workspace: &str,
    executable: &str,
) -> Result<Launched, String> {
    if workspace.is_empty() {
        return Err("launch configuration needs a workspace".to_owned());
    }
    require_native_queue(services, executable).await?;
    let port = services.ports.free_loopback()?;
    let mut drawn = [0; 24];
    services.entropy.fill(&mut drawn)?;
    let token = URL_SAFE_NO_PAD.encode(drawn);
    let bridge = js::stringify(&json!({
        "launchId": launch.as_str(),
        "port": port,
        "token": token,
    }));
    Ok(Launched {
        env: vec![("CF_CODEX_SESSION_BRIDGE".to_owned(), bridge)],
        channel: Channel::new(launch.as_str(), format!("http://127.0.0.1:{port}"), token),
    })
}

/// The command a Codex window opens on: the bundle's native `cf` as its
/// supervisor (`cf codex-session`), given Codex's executable and then Codex's
/// own arguments (`withNativeBridge`).
pub(super) fn with_native_bridge(
    bundle: &Bundle,
    executable: &str,
    args: Vec<String>,
) -> Vec<String> {
    let mut argv = vec![
        bundle.cf.to_string_lossy().into_owned(),
        "codex-session".to_owned(),
        executable.to_owned(),
    ];
    argv.extend(args);
    argv
}

#[cfg(test)]
mod tests;
