//! The one place the runner builds the programs that reach a machine, `ssh` and `scp`, so that
//! every connection it makes ends when its peer goes silent.
//!
//! A connection to a peer that has gone (a laptop that slept, a tunnel that dropped, a machine
//! that was switched off) gives ssh nothing to read, and nothing to fail: it waits until the
//! system's keepalive gives up, two hours and ten minutes on macOS. A run that waits holds the
//! machine's queue with it, and every run behind it (zeewin2, 2026-10-27: the reports of a run
//! that was refused, listed over a connection that had died at 11:50, held both machines until
//! 13:50). So every connection is given the bounds below, whatever it is for: the run, the end of
//! a run, the tree sent, the reports listed and brought back, and the machine asked.
//!
//! - `BatchMode=yes`: ssh asks nobody anything. A password, a passphrase or a host key that is not
//!   known yet would be a question waiting on a terminal that no one watches: the connection
//!   fails and says why (a `--host` never reached before fails on its host key, and is first
//!   reached by hand).
//! - `ConnectTimeout`: the time to make the connection, the banner and the key exchange included.
//! - `ServerAliveInterval` and `ServerAliveCountMax`: after `interval` seconds without a word
//!   from the peer ssh asks it, through the encrypted channel, whether it is there; the peer's
//!   answer, or any word from it, starts the count again; and when more than `count` questions
//!   have gone unanswered ssh ends the connection with 255 and says `Timeout, server … not
//!   responding.` So a peer that says nothing for `interval × (count + 1)` seconds is given up,
//!   and a peer that comes back before that is not.
//!
//! **A run's bounds** ([`RUN`]) are 15 s and 8: a silent peer is given up 135 s after its last
//! word, which is "within a few minutes" and is not the first minute: the laptop is reached
//! over a tunnel through Wi-Fi, and a drop of under a minute at times (the answers to the
//! questions arrive late, or the packets are sent again) must not end a long run that goes on
//! there. The connection takes up to 30 s to be made, a tunnel that must wake up included.
//!
//! **A probe's** ([`PROBE`]) are 5 s and 1: it asks for a short answer from a machine that may be
//! off (zeelin answers only while WSL runs), and is given up 10 s after the peer's last word.

use crate::context::Context;
use crate::process::Invocation;

/// How long a connection waits for the peer, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Bounds {
    /// To make the connection (`ConnectTimeout`).
    connect_s: u32,
    /// Of silence from the peer before ssh asks whether it is there (`ServerAliveInterval`).
    alive_s: u32,
    /// How many of those questions may go unanswered (`ServerAliveCountMax`).
    unanswered: u32,
}

/// What the runner's connections are given: a run, its end, the tree sent, the reports.
pub(super) const RUN: Bounds = Bounds {
    connect_s: 30,
    alive_s: 15,
    unanswered: 8,
};

/// What a short question to a machine that may be off is given.
pub(super) const PROBE: Bounds = Bounds {
    connect_s: 5,
    alive_s: 5,
    unanswered: 1,
};

impl Bounds {
    /// How long a peer may say nothing before the connection is ended, in seconds: the first
    /// question goes after `alive_s`, and the connection ends at the one after the last that may
    /// go unanswered.
    pub(super) const fn silence_s(self) -> u32 {
        self.alive_s * (self.unanswered + 1)
    }

    /// The words that give a program these bounds, to go before its host and its files.
    fn words(self) -> [String; 8] {
        [
            "-o".into(),
            format!("ConnectTimeout={}", self.connect_s),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            format!("ServerAliveInterval={}", self.alive_s),
            "-o".into(),
            format!("ServerAliveCountMax={}", self.unanswered),
        ]
    }
}

/// `ssh` to `host`, ended when the peer goes silent as `bounds` say. What it is to run there is
/// the caller's to add.
pub(super) fn ssh(context: &Context, host: &str, bounds: Bounds) -> Invocation {
    Invocation::new("ssh", &context.root)
        .args(bounds.words())
        .arg(host)
}

/// `scp`, quiet, ended when the peer goes silent as `bounds` say. The files are the caller's to add.
pub(super) fn scp(context: &Context, bounds: Bounds) -> Invocation {
    Invocation::new("scp", &context.root)
        .arg("-q")
        .args(bounds.words())
}

/// What ssh says, at the start of a line, when it gives a silent peer up.
pub(super) const SILENCE_MARK: &str = "Timeout, server ";

/// Whether `line` is what ssh says when it gives a silent peer up: `Timeout, server <host> not
/// responding.`
pub(super) fn says_silence(line: &str) -> bool {
    let line = line.trim_end();
    line.starts_with(SILENCE_MARK) && line.ends_with(" not responding.")
}

/// Whether `stderr`, all that a program said, says that ssh gave a silent peer up.
pub(super) fn went_silent(stderr: &str) -> bool {
    stderr.lines().any(says_silence)
}

/// How the connection to `host` that went silent is told.
pub(super) fn silent_words(host: &str, bounds: Bounds) -> String {
    format!(
        "the connection to {host} went silent for {} s",
        bounds.silence_s()
    )
}

#[cfg(test)]
mod tests;
