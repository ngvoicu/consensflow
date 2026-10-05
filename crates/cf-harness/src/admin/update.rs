//! A harness brought to its latest release the way it was installed
//! (`update`, `src/harness-admin.js`): its own updater, Homebrew or npm,
//! then a look again, and what happened: updated, unchanged, failed (with the
//! tool's last lines) or unsupported when the install method is not
//! recognized.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::js;
use cf_process::Limits;
use cf_proto::agents::Harness;

use super::row::{Ended, Outcome};
use super::source::{name, release_source};
use super::{program, HarnessAdmin};

/// The limits an update is held to: ten minutes and a megabyte a stream.
const UPDATE: Limits = Limits {
    timeout: Duration::from_secs(600),
    max_buffer: 1_000_000,
};

/// How many of the last lines of what an update wrote are kept.
const LINES: usize = 20;

/// How many of the last UTF-16 code units of them are kept.
const UNITS: usize = 2000;

impl HarnessAdmin {
    /// Updates the harness named `id` the way it was installed, and looks at
    /// it again. A CLI that is not installed, or a name that is no
    /// harness's, is an error in the words `Claude is not installed` and
    /// `Unknown harness`; one installed in a way that is not recognized is
    /// said `unsupported`, and nothing is run.
    pub async fn update(&self, id: &str) -> Result<Outcome, String> {
        let inner = &self.inner;
        let harness = Harness::from_name(id).ok_or("Unknown harness")?;
        let row = self.look(harness, false).await;
        if !row.installed {
            return Err(format!("{} is not installed", name(harness)));
        }
        let executable = row.path.clone().unwrap_or_default();
        let source = release_source(harness, &executable, &inner.env);
        let argv = source.update.unwrap_or_default();
        let Some((run, arguments)) = argv.split_first() else {
            return Ok(Outcome::Unsupported {
                id: harness,
                state: Ended::Unsupported,
                reason: format!(
                    "ConsensFlow does not recognize how {} was installed here: update it the way you installed it.",
                    name(harness)
                ),
                harness: row,
            });
        };
        let before = row.version.value().map(str::to_owned);
        let command = argv.join(" ");
        let started = program(&inner.env, PathBuf::from(run), arguments.to_vec());
        let (written, failure) = match inner.capture.capture(started, UPDATE).await {
            Ok(done) => (format!("{}{}", done.stdout, done.stderr), None),
            Err(failed) => {
                let reason = if failed.killed {
                    "the update ran for ten minutes and was stopped".to_owned()
                } else {
                    failed.message
                };
                (format!("{}{}", failed.stdout, failed.stderr), Some(reason))
            }
        };
        let after = self.look(harness, true).await;
        let now = after.version.value().map(str::to_owned);
        // `after.version.value !== before`, where an unrecognized version is
        // `undefined` after and `null` before: never equal to anything.
        let state = if failure.is_some() {
            Ended::Failed
        } else if now.is_some() && now == before {
            Ended::Unchanged
        } else {
            Ended::Updated
        };
        Ok(Outcome::Ran {
            id: harness,
            state,
            before,
            after: now,
            command,
            output: last_of(&written),
            reason: failure,
            harness: after,
        })
    }
}

/// The last lines of what the update wrote, the last of its characters: its
/// text with the white space around it taken off, the last 20 lines of it,
/// the last 2000 UTF-16 code units of those.
fn last_of(written: &str) -> String {
    let lines: Vec<&str> = js::trim(written).split('\n').collect();
    let kept = lines[lines.len().saturating_sub(LINES)..].join("\n");
    last_units(&kept, UNITS)
}

/// The last `units` UTF-16 code units of `text` (`.slice(-units)`), or all of
/// it when it is shorter.
///
/// Kept from Node on purpose: a cut through half of a pair leaves JavaScript
/// a lone surrogate, which no Rust text can hold; U+FFFD stands for it, as
/// `utf16_prefix` does at the other end.
fn last_units(text: &str, units: usize) -> String {
    let (mut taken, mut start) = (0, text.len());
    for (at, character) in text.char_indices().rev() {
        let width = character.len_utf16();
        if taken + width > units {
            let cut = if taken < units { "\u{FFFD}" } else { "" };
            return format!("{cut}{}", &text[start..]);
        }
        taken += width;
        start = at;
    }
    text.to_owned()
}

#[cfg(test)]
mod tests;
