//! A harness brought to its latest release the way it was installed: its own
//! updater, Homebrew or npm, then a look again, and what happened: updated,
//! unchanged, failed (with the tool's last lines) or unsupported when the
//! install method is not recognized.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::js;
use cf_process::Limits;
use cf_proto::agents::Harness;

use super::record;
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
        Ok(self.update_stamped(id).await?.0)
    }

    /// [`update`](Self::update), with the stamp of the look made after the install. One
    /// update of a harness runs at a time, whichever page asks: a second is refused in words
    /// while the first runs, and the daemon says it is running (`observe`'s `updating`).
    pub async fn update_stamped(&self, id: &str) -> Result<(Outcome, u64), String> {
        let harness = Harness::from_name(id).ok_or("Unknown harness")?;
        let _running = Running::begin(&self.inner, harness)?;
        self.update_running(harness).await
    }

    async fn update_running(&self, harness: Harness) -> Result<(Outcome, u64), String> {
        let inner = &self.inner;
        let (first, row) = self.look(harness, false).await;
        if !row.installed {
            return Err(format!("{} is not installed", name(harness)));
        }
        let executable = row.path.clone().unwrap_or_default();
        let source = release_source(harness, &executable, &inner.env);
        let argv = source.update.unwrap_or_default();
        let Some((run, arguments)) = argv.split_first() else {
            return Ok((
                Outcome::Unsupported {
                    id: harness,
                    state: Ended::Unsupported,
                    reason: format!(
                        "ConsensFlow does not recognize how {} was installed here: update it the way you installed it.",
                        name(harness)
                    ),
                    harness: row,
                },
                first,
            ));
        };
        let before = row.version.value().map(str::to_owned);
        let command = argv.join(" ");
        let started = program(&inner.env, PathBuf::from(run), arguments.to_vec());
        let installer = {
            let env = inner.env.clone();
            Box::new(move |pid| record::write(&env, harness, pid))
        };
        let captured = inner
            .capture
            .capture_started(started, UPDATE, installer)
            .await;
        record::remove(&inner.env, harness);
        let (written, failure) = match captured {
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
        let (stamp, after) = self.look(harness, true).await;
        // A look that was cut keeps nothing: what is kept is from before the install.
        let mut cache = inner.cache.borrow_mut();
        if cache.get(&harness).is_some_and(|(kept, _)| *kept < stamp) {
            cache.remove(&harness);
        }
        drop(cache);
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
        Ok((
            Outcome::Ran {
                id: harness,
                state,
                before,
                after: now,
                command,
                output: last_of(&written),
                reason: failure,
                harness: after,
            },
            stamp,
        ))
    }
}

/// An update of one harness, running: it is let go of when the update ends or its call is
/// dropped, and while it holds, no other update of that harness begins.
struct Running<'a> {
    inner: &'a super::Inner,
    harness: Harness,
}

impl<'a> Running<'a> {
    fn begin(inner: &'a super::Inner, harness: Harness) -> Result<Self, String> {
        let refused = || format!("an update of {} is already running", name(harness));
        // An installer a daemon that is gone began, and that still runs, is as running.
        if record::running(&inner.env, harness) || !inner.updating.borrow_mut().insert(harness) {
            return Err(refused());
        }
        Ok(Self { inner, harness })
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        record::remove(&self.inner.env, self.harness);
        *self
            .inner
            .ended
            .borrow_mut()
            .entry(self.harness)
            .or_insert(0) += 1;
        self.inner.updating.borrow_mut().remove(&self.harness);
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
