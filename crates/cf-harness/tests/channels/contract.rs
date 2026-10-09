//! One contract for every channel's answer to a send (the owner's rule,
//! 2026-10-02): every harness treats a message that may have been sent the same
//! way. Each channel has a handover point (Pi's inbox rename, a paste's first
//! byte, Codex's and OpenCode's POST):
//! - refused before it, nothing reached the harness: `admitted: false` with
//!   nothing written, and the dispatcher may send again at once;
//! - any error at or after it may have reached the harness: `admitted: null`,
//!   and the dispatcher waits for the harness's own record, sending again only
//!   if the record never shows the message in time;
//! - accepted: `admitted: true`.
//!
//! Codex's, OpenCode's and Pi's channels run every row here, against stand-ins
//! for the pane host and the harness side. The channels that are pasted into
//! their windows (Claude Code and Devin) have their rows in the library, through
//! the same `write_paste` and `Sent` the adapters read (`shared/pane/tests.rs`,
//! and `devin/channel/tests.rs` for Devin's own refusals), where the answer is
//! read as the adapters read it (`shared::admission`).
//!
//! A test is a way a channel has of answering in a row: its path names the
//! channel, the row and the way.

use std::fs;

use cf_base::env::Env;
use cf_harness::contract::PaneHost;
use cf_harness::opencode::{self, Bridge};
use cf_harness::seams::{SystemEntropy, SystemLoopback, SystemProcesses, SystemTime};
use cf_harness::testing::server::{Reply, Request, Server};
use cf_harness::testing::AnsweringHost;
use cf_harness::{codex, pi};
use serde_json::{json, Value};

use crate::pi_window::{Answers, Window};
use crate::support::{pane, run, wires};

const TEXT: &str = "the cache key is per conversation";

/// A harness's own token, as a launch draws it.
const TOKEN: &str = "tttttttttttttttttttttttttttttttt";

/// How a send is answered under the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// Refused before the handover point.
    Refused,
    /// An error at or after it.
    Uncertain,
    Accepted,
}

impl Row {
    const ALL: [Self; 3] = [Self::Refused, Self::Uncertain, Self::Accepted];

    /// What the contract says of `admitted` in this row.
    fn admitted(self) -> Option<bool> {
        match self {
            Self::Refused => Some(false),
            Self::Uncertain => None,
            Self::Accepted => Some(true),
        }
    }
}

/// What a channel's answer says under the contract: `admitted`, and whether
/// nothing was written.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    admitted: Option<bool>,
    zero_bytes: bool,
}

impl From<&codex::Answer> for Verdict {
    fn from(answer: &codex::Answer) -> Self {
        Self {
            admitted: answer.admitted,
            zero_bytes: answer.zero_bytes,
        }
    }
}

impl From<&pi::Answer> for Verdict {
    fn from(answer: &pi::Answer) -> Self {
        Self {
            admitted: answer.admitted,
            zero_bytes: answer.zero_bytes,
        }
    }
}

/// OpenCode's channel answers as an adapter reads a send: a refusal before the
/// handover wrote nothing.
impl From<&opencode::Sent> for Verdict {
    fn from(sent: &opencode::Sent) -> Self {
        Self {
            admitted: if sent.ok {
                Some(true)
            } else {
                sent.refused.then_some(false)
            },
            zero_bytes: sent.refused,
        }
    }
}

/// That `verdict` is what the contract has for `row`.
fn holds(row: Row, verdict: &Verdict) {
    assert_eq!(verdict.admitted, row.admitted(), "{verdict:?}");
    if row == Row::Refused {
        assert!(verdict.zero_bytes, "{verdict:?}");
    }
}

/// Every way a channel has of answering a send, in a module for each channel
/// and for each row of the contract, a test for each way. A channel that leaves
/// a row out does not compile: that is what `expresses_every_row` asks.
macro_rules! channel {
    (
        $channel:ident:
        refused { $($refused:ident => $refusal:expr),+ $(,)? }
        uncertain { $($uncertain:ident => $doubt:expr),+ $(,)? }
        accepted { $($accepted:ident => $acceptance:expr),+ $(,)? }
    ) => {
        mod $channel {
            use super::*;

            #[test]
            fn expresses_every_row() {
                let ways = [
                    (Row::Refused, [$(stringify!($refused)),+].len()),
                    (Row::Uncertain, [$(stringify!($uncertain)),+].len()),
                    (Row::Accepted, [$(stringify!($accepted)),+].len()),
                ];
                assert_eq!(ways.map(|(row, _)| row), Row::ALL);
                assert!(ways.iter().all(|(_, count)| *count > 0));
            }

            mod refused_before_its_handover_point {
                use super::*;
                $(
                    #[test]
                    fn $refused() {
                        holds(Row::Refused, &run($refusal));
                    }
                )+
            }

            mod an_error_at_or_after_its_handover_point {
                use super::*;
                $(
                    #[test]
                    fn $uncertain() {
                        holds(Row::Uncertain, &run($doubt));
                    }
                )+
            }

            mod accepted {
                use super::*;
                $(
                    #[test]
                    fn $accepted() {
                        holds(Row::Accepted, &run($acceptance));
                    }
                )+
            }
        }
    };
}

/// What the pane host answers a claim with.
#[derive(Debug, Clone, Copy)]
enum Claim {
    Granted,
    Refused(&'static str),
}

impl Claim {
    fn answer(self) -> Value {
        match self {
            Self::Granted => json!({ "ok": true }),
            Self::Refused(error) => json!({ "ok": false, "error": error }),
        }
    }
}

/// What a harness's own server does with a message posted to it (Codex's
/// broker, OpenCode's plugin): it reads the POST, then answers or drops the
/// connection.
#[derive(Debug, Clone, Copy)]
enum Harness {
    Takes,
    Refuses,
    Drops,
}

impl Harness {
    async fn start(self) -> Server {
        Server::start(move |_: &Request| match self {
            Self::Takes => Reply::json(200, &json!({ "ok": true, "admitted": true })),
            Self::Refuses => Reply::json(
                200,
                &json!({
                    "ok": false,
                    "admitted": false,
                    "bytesWritten": 0,
                    "error": "native-session-changed",
                }),
            ),
            Self::Drops => Reply::Drop,
        })
        .await
    }
}

/// A message sent through Codex's channel to a broker that does as `harness`
/// says, from a pane whose host answers as `claim` says.
async fn through_codex(claim: Claim, harness: Harness) -> Verdict {
    let server = harness.start().await;
    let channel = codex::Channel::new("launch-codex", server.endpoint(), TOKEN.to_owned());
    let pane = pane("p1-diana", 3);
    let host = AnsweringHost::new(move |_| Ok(claim.answer()));
    let target = codex::Target {
        channel: &channel,
        thread: Some("01a0817b-e6b0-7f32-8e11-370dc000cbc0"),
        pane: &pane,
        host: &host as &dyn PaneHost,
    };
    let answer = codex::send(&SystemTime, &SystemLoopback, &target, TEXT)
        .await
        .unwrap();
    Verdict::from(&answer)
}

/// A message sent through OpenCode's channel to a plugin that does as `harness`
/// says, from a pane whose host answers as `claim` says.
async fn through_opencode(claim: Claim, harness: Harness) -> Verdict {
    let server = harness.start().await;
    let channel = opencode::Channel {
        launch_id: "launch-opencode".to_owned(),
        endpoint: String::new(),
        password: String::new(),
        bridge: Bridge {
            endpoint: server.endpoint(),
            token: TOKEN.to_owned(),
        },
    };
    let pane = pane("p1-hera", 3);
    let host = AnsweringHost::new(move |_| Ok(claim.answer()));
    let target = opencode::Target {
        session: "ses_contract1",
        pane: &pane,
        host: &host as &dyn PaneHost,
    };
    let processes = SystemProcesses::new(Env::default());
    let sent = opencode::send(wires(&processes), &channel, &target, TEXT)
        .await
        .unwrap();
    Verdict::from(&sent)
}

/// A message sent through Pi's channel to an extension that does as `answers`
/// says, which has `ack_timeout_ms` to give its verdict; its inbox is a file
/// where it `cannot_be_written`.
struct Pi {
    claim: Claim,
    answers: Answers,
    ack_timeout_ms: u64,
    cannot_be_written: bool,
}

impl Default for Pi {
    fn default() -> Self {
        Self {
            claim: Claim::Granted,
            answers: Answers::Nothing,
            ack_timeout_ms: 5_000,
            cannot_be_written: false,
        }
    }
}

async fn through_pi(case: Pi) -> Verdict {
    let window = Window::open(case.answers);
    let inbox = if case.cannot_be_written {
        let file = window.root().join("not-a-folder");
        fs::write(&file, "").unwrap();
        file
    } else {
        window.inbox.clone()
    };
    let pane = pane("p1-zeus", 3);
    let claim = case.claim;
    let host = AnsweringHost::new(move |_| Ok(claim.answer()));
    let target = pi::Target {
        launch_id: "launch-pi",
        inbox: inbox.to_str().unwrap(),
        ack: window.ack.to_str().unwrap(),
        ack_timeout_ms: case.ack_timeout_ms,
        session: "cf-1-zeus-0000abcd",
        pane: &pane,
        host: &host as &dyn PaneHost,
    };
    let answer = pi::send(&SystemTime, &SystemEntropy, &target, TEXT)
        .await
        .unwrap();
    window.finish();
    Verdict::from(&answer)
}

channel! {
    codex_through_its_broker:
    refused {
        the_pane_host_refuses_its_claim =>
            through_codex(Claim::Refused("the pane is gone"), Harness::Takes),
        the_broker_refuses_it => through_codex(Claim::Granted, Harness::Refuses),
    }
    uncertain {
        the_broker_drops_the_connection_after_the_post =>
            through_codex(Claim::Granted, Harness::Drops),
    }
    accepted {
        the_broker_takes_it => through_codex(Claim::Granted, Harness::Takes),
    }
}

channel! {
    opencode_through_its_plugin:
    refused {
        the_pane_host_refuses_its_claim =>
            through_opencode(Claim::Refused("the pane is gone"), Harness::Takes),
        the_plugin_refuses_it => through_opencode(Claim::Granted, Harness::Refuses),
    }
    uncertain {
        the_plugin_drops_the_connection_after_the_post =>
            through_opencode(Claim::Granted, Harness::Drops),
    }
    accepted {
        the_plugin_takes_it => through_opencode(Claim::Granted, Harness::Takes),
    }
}

channel! {
    pi_through_its_extension_inbox:
    refused {
        the_pane_host_refuses_its_claim => through_pi(Pi {
            claim: Claim::Refused("pane p1-zeus is gone"),
            ..Pi::default()
        }),
        its_inbox_cannot_be_written => through_pi(Pi {
            cannot_be_written: true,
            ..Pi::default()
        }),
        the_extension_refuses_it_before_sending => through_pi(Pi {
            answers: Answers::Acknowledges(
                json!({ "admitted": false, "bytesWritten": 0, "reason": "chief busy" }),
            ),
            ..Pi::default()
        }),
    }
    uncertain {
        // The case a loaded CI runner hit (2026-10-02): something failed while
        // the channel waited for the acknowledgement, after the record was in.
        an_error_after_the_inbox_rename => through_pi(Pi {
            answers: Answers::MakesAFolder,
            ..Pi::default()
        }),
        no_acknowledgement_before_the_record_expires => through_pi(Pi {
            ack_timeout_ms: 100,
            ..Pi::default()
        }),
    }
    accepted {
        the_extension_shows_it_in_pi => through_pi(Pi {
            answers: Answers::Acknowledges(json!({ "admitted": true, "mode": "tui" })),
            ..Pi::default()
        }),
    }
}
