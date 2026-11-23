//! What a Codex window is launched with: the native queue its CLI must have,
//! the port and token of the broker its supervisor serves (the `codex` branch
//! of `launchConfiguration`), and the supervisor that opens the window
//! (`withNativeBridge`).

use std::future::Future;
use std::path::Path;

use cf_base::js;
use serde_json::json;

use super::channel::Channel;
use crate::contract::LaunchId;
use crate::launch::{Check, Refused, Said};
use crate::seams::processes::{probe, Asking, Probed, Unanswered};
use crate::seams::{Bundle, Services};
use crate::shared::token;

/// How a Codex window is opened on its launch's broker.
pub(super) struct Launched {
    /// The environment the supervisor is told its broker by.
    pub(super) env: Vec<(String, String)>,
    pub(super) channel: Channel,
}

/// What the CLI at `path` answers to `args`.
async fn ask(asking: &Asking<'_>, path: &Path, args: &[&str]) -> Result<Probed, Unanswered> {
    probe(asking.probes, asking.processes, path, args, asking.env).await
}

/// A Codex window runs under ConsensFlow's supervisor, which needs the native
/// queue (and the app-server and remote TUI that came with it): a Codex
/// without it cannot be reached in its window, so it is refused by its
/// version. Detection asks the resolved executable, never a second CLI from
/// PATH.
///
/// The check a launch makes and the Admin page shows. A Codex that passes is asked no
/// version. One that is refused is asked its version, through `version`, to name it in
/// the sentence: `version` is the question the caller means to ask (a launch's own
/// `probe`, or the page's, which it asks beside this check and shares with it, so that
/// one look asks it once, failure included).
pub(crate) async fn native_queue<V>(asking: &Asking<'_>, executable: &str, version: V) -> Check
where
    V: Future<Output = Result<Probed, Unanswered>>,
{
    let path = Path::new(executable);
    if !path.is_absolute() {
        return unread("a Codex launch needs the absolute path of its CLI".to_owned());
    }
    let help = match ask(asking, path, &["queue", "--help"]).await {
        Ok(help) => help,
        // Node threw this one where it asked, before any run.
        Err(Unanswered::Unread(sentence)) => return unread(sentence),
        Err(Unanswered::Failed(failed)) => {
            return unread(format!(
                "could not ask Codex whether it has its native queue: {}",
                if failed.killed {
                    "it did not answer in time"
                } else {
                    &failed.message
                }
            ))
        }
    };
    if ends_a_word(&help.stdout, "--thread") && ends_a_word(&help.stdout, "--message") {
        return Check {
            said: Said::NotAsked,
            verdict: Ok(()),
        };
    }
    let (said, number) = match version.await {
        Ok(asked) => {
            let number = named_version(&asked.stdout).map(str::to_owned);
            (Said::Read(asked.stdout), number)
        }
        Err(Unanswered::Unread(sentence)) => return unread(sentence),
        Err(Unanswered::Failed(failed)) => (
            Said::Unanswered(if failed.killed {
                crate::launch::TIMED_OUT.to_owned()
            } else {
                failed.message
            }),
            None,
        ),
    };
    let codex = number.map_or_else(
        || "This Codex".to_owned(),
        |number| format!("Codex {number}"),
    );
    Check {
        said,
        verdict: Err(Refused {
            sentence: format!(
                "{codex} has no native queue, which ConsensFlow needs to reach its window: update Codex."
            ),
            read: true,
        }),
    }
}

/// A check that could not read the CLI: the launch's sentence.
fn unread(sentence: String) -> Check {
    Check {
        said: Said::NotAsked,
        verdict: Err(Refused {
            sentence,
            read: false,
        }),
    }
}

/// The check a launch makes of the Codex at `executable`.
pub(crate) async fn check(asking: &Asking<'_>, executable: &str) -> Check {
    let version = ask(asking, Path::new(executable), &["--version"]);
    native_queue(asking, executable, version).await
}

/// The launch's own call of [`check`]: the sentence it fails with.
async fn require_native_queue(services: &Services, executable: &str) -> Result<(), String> {
    check(&services.asking(), executable)
        .await
        .verdict
        .map_err(|refused| refused.sentence)
}

/// The version a refused Codex is named by, on its row and in its sentence alike: the
/// release its own line proves, else the first version number the answer holds.
pub(crate) fn named_version(said: &str) -> Option<&str> {
    super::declared::release(said).or_else(|| version_in(said))
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
    let token = token::draw(&*services.entropy)?;
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
