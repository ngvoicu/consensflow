//! What the pane host says of a window's end and of its screen, beyond which
//! pane it is: the exit code a program ended with and the last lines its
//! screen showed. The host sends them with `pane.exit`, and answers them to a
//! `pane.snapshot` that asks; the daemon keeps them for the failure note of a
//! window that did not come up.
//!
//! Every field is optional on the wire, and a body without one is a host that
//! does not say (an older one, or a test's): "does not say" is not "said
//! nothing". A screen that showed nothing is an empty `tail`.
//!
//! The host also says what a window's program started and left running: the
//! processes of the window's tree that began after the turn did and that it can
//! tell are the turn's work, which `pane.mark` fixes the start of
//! ([`MarkRequest`]) and `pane.background` answers ([`BackgroundProcess`]), and
//! which `pane.exit` lists for a window that closed with some of it running. It
//! ends them without closing the window when asked (`pane.end_background`:
//! [`EndRequest`], answered by [`Ending`]), and chooses what to end itself, from
//! the whole of what it can tell: the daemon names no process, so no number crosses
//! from one look to the next. A process it cannot tell the turn's from the
//! harness's it leaves alone, and says nothing of; nor does it end what it cannot
//! prove the turn's by a chain of processes that run to a shell the harness started.
//! What it ends it chooses once, when the ending begins, and answers within
//! [`ENDING_BOUND`]. It says too which commands of the window's harness run in its tree
//! now, where it can tell them (`pane.commands`, answered by [`Background`] or by nothing).

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The most lines of a screen the host sends as its tail, and so the most a
/// request for it gets. Empty lines are not counted: they are not sent.
pub const TAIL_LINES: usize = 12;

/// The most lines of a screen as drawn (`bottom`) a snapshot sends.
pub const BOTTOM_LINES: usize = 40;

/// The most the host takes to end what a turn left, and to answer that it has: the
/// moment it waits before it looks again at what it found (two seconds), the grace a
/// close gives what it asks to end (ten), what it waits after it has killed what
/// ignored the request (two) and its looks between, with room; on Windows the whole of
/// its passes is held to less than this. The daemon waits for the answer by this and a
/// margin, and holds the window until it has it.
pub const ENDING_BOUND: Duration = Duration::from_secs(20);

/// The most processes the host names when it says what a window left running
/// (it says how many there are besides): a note quotes a few, and a frame is
/// no place for a build's hundred processes.
pub const BACKGROUND_PROCESSES: usize = 16;
/// The most characters of a process's command line the host sends, a mark of
/// where it was cut at the end: a frame is no place for a build's whole one.
pub const COMMAND_CHARS: usize = 512;

/// How a window's harness keeps its own processes (its MCP servers, its helpers)
/// apart from the commands its agent starts, which the host cannot tell by
/// anything the processes are: a server and a recording run are both children of
/// the harness. The harness's adapter says it of its windows and every
/// `pane.mark` carries it. Each shape says what is the harness's, and so no turn's
/// work, however late it began, and the host ends none of it: what it lists and
/// ends as the turn's is what a shell the harness started started, and what that
/// started, and these are told apart from it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Servers {
    /// In the process group of the window's program, which leads it: the harness
    /// starts them whenever it likes (Claude Code, as it takes its first message),
    /// and runs every command its agent starts in a group of its own. What the
    /// group holds besides the window's program, and what any process in it starts
    /// (a browser a server launches, in a group of its own too), is the harness's.
    /// What the window's program itself starts is the agent's.
    InGroup,
    /// What the window had outside that group when its first turn began: the
    /// harness starts them with the window, each in a group of its own (Codex's
    /// plugins), and the agent's commands are started by the harness's own
    /// process, which is in the window's group. What any of them starts later is
    /// the harness's.
    AtLaunch,
    /// Not known: no process is told for the harness's by where it is, so only what
    /// the host can prove the harness's by its program (a program named for it
    /// that is beside its own binary, and what descends from one) is left alone, and
    /// only what it can prove the turn's (a shell the harness started after the
    /// mark and what the shell started, by a chain of processes that run) is the
    /// turn's: what it can tell neither it leaves alone, and does not name. What a
    /// harness is until someone has looked at what a window's tree holds with a
    /// command of its agent running; and what a word this host does not know reads
    /// as.
    #[default]
    #[serde(other)]
    Unknown,
}

impl Servers {
    /// Whether it is not known how the harness keeps them: what a request says
    /// by saying nothing.
    pub fn is_unknown(&self) -> bool {
        *self == Servers::Unknown
    }
}

/// A `pane.mark` request: a turn is about to begin in the pane, and what its
/// tree has now came before it. `servers` says how the window's harness keeps its
/// own processes, and `harness` which harness it is (`claude`, `codex`…), by whose
/// name the host knows the programs of its own; a request without either (an older
/// daemon's) is one that does not say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkRequest {
    pub id: String,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Servers::is_unknown")]
    pub servers: Servers,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
}

/// A process the window's program started and left running past its turn: its
/// number, and the command line it was started with, as far as the host could
/// read it (`COMMAND_CHARS` at most; the program alone where it could read no more).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcess {
    pub pid: u32,
    pub command: String,
}

/// `pane.background`'s answer: what the window's tree has running that began
/// after the window's last `pane.mark` and that the host can tell is the turn's
/// work, `total` of it and the first `processes` of it by how long ago they began.
/// A window with no mark has none: nothing says where its turn began.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    pub total: usize,
    pub processes: Vec<BackgroundProcess>,
}

/// `pane.commands`' answer, under `commands`: the commands of the window's harness that
/// run in its tree now, listed as `pane.background` lists its answer, where the host can
/// tell them; `null` where it cannot, which is not "none". A command is what the window's
/// program started in a process group of its own: Claude Code runs each command of its
/// agent so (a Bash call, a Monitor's, a background job's, in the foreground or not, and
/// what one `exec`s keeps the group), and its hooks and status line, a moment each; its
/// MCP servers and helpers stay in the window's group. The host tells them only of a
/// window whose harness keeps its servers in the window's group ([`Servers::InGroup`]),
/// on a platform with groups: nowhere else does a group say what a process is. It is
/// `null` too where a process of the window cannot be read (another user's, as a command
/// that `exec`s `sudo` becomes), which may be a command: the answer says so then
/// ([`CommandsAnswer`]). What ran at the rest the request
/// names (`since`, a number `pane.rest` answered) is left out, and what the host proves
/// began before the time it names (`after`; the host's clock watch): a server or helper
/// that left the window's group.
pub type Commands = Option<Background>;

/// A `pane.commands` request: the pane, the rest whose processes are no command (`since`),
/// where the daemon names one, and the time before which a process began is no command
/// (`after`, milliseconds since the epoch), where it names one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandsRequest {
    pub id: String,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<i64>,
}

/// `pane.commands`' answer: the commands ([`Commands`]), and, where they are `null`
/// because a process of the window cannot be read (another user's), `unreadable`, or
/// because the window's processes kept changing while the host read them, `unsettled`
/// (Calliope on 136a7158, Astraeus on 02a6cd4f: on a platform that tells them, neither is a
/// "cannot tell", and a later look may read them all).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandsAnswer {
    pub commands: Commands,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unreadable: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsettled: bool,
}

/// A `pane.rest` request: the pane, and the rests the daemon still names (`keep`), which
/// the host keeps however many come after them (Astraeus on 136a7158).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestRequest {
    pub id: String,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep: Vec<u64>,
}

/// `pane.rest`'s answer, under `rest`: the number the host keeps the window's processes
/// under, now that the daemon says the window is at rest, for a later `pane.commands` to
/// name; `null` where it tells no commands of the window.
pub type Rest = Option<u64>;

/// A `pane.end_background` request: the pane's turn is over and the daemon has
/// taken its answer for the task's result, so what the turn left running is
/// to end, the window staying open. It names no process: the host chooses, from all
/// of what it can tell is the turn's, and keeps what it chose by what each process
/// is (its number and when it began), not by a number a later look could find
/// another process at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndRequest {
    pub id: String,
    pub generation: u64,
}

/// `pane.end_background`'s answer: what the host ended of the turn's work, and what
/// of it it could not end. Each is listed as `pane.background` lists its answer:
/// how many, and the first. A process the host could not tell the turn's from the
/// harness's is left alone and is in neither.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ending {
    pub ended: Background,
    pub left: Background,
}

/// `pane.exit`'s body: the pane that ended, and what the host kept of its end.
/// The pane must be named; a field of the rest that is not what it should be
/// is a field the host did not say, and the exit is the exit all the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneExit {
    pub id: String,
    pub generation: u64,
    /// The program's exit code, when the host could read it before it sent
    /// this. A program ended by a signal has the code 1 and its `signal`.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub exit_code: Option<u32>,
    /// The signal that ended the program, by the name the system gives it.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub signal: Option<String>,
    /// The last lines the window's screen showed, the empty ones left out, the
    /// oldest first.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub tail: Option<Vec<String>>,
    /// What the window's program started and left running when the window
    /// closed, which the close ended: the processes of its tree that began after
    /// its last `pane.mark`. Empty for a window with none, and for a host that
    /// does not say.
    #[serde(
        default,
        deserialize_with = "or_none_said",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub background: Vec<BackgroundProcess>,
    /// The window's program, by its process number, where the host could not prove it
    /// gone when it sent this, though it had begun to end it and waited a while for the
    /// end (0 where its number was not known): it may still run. None where it has gone.
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub survivor: Option<u32>,
}

/// A list as it should be, or an empty one: what is not a list of `T` was not said.
fn or_none_said<'de, D, T>(field: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(or_unsaid(field)?.unwrap_or_default())
}

/// A field as it should be, or none: what is not a `T` was not said.
fn or_unsaid<'de, D, T>(field: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(field)?;
    Ok(serde_json::from_value(value).ok())
}

/// A `pane.snapshot` request. Without `tail` the answer is what it has always
/// been; with it, the answer also holds the screen's last `tail` lines, which
/// a snapshot polled every few moments does not carry unasked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRequest {
    pub id: String,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<usize>,
    /// The screen's last lines as drawn, its rules and empty prompt among
    /// them: the answer's `bottom`, where an adapter reads its harness's
    /// input box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<usize>,
}

/// The part of a `pane.snapshot` answer a request for the tail adds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotTail {
    #[serde(
        default,
        deserialize_with = "or_unsaid",
        skip_serializing_if = "Option::is_none"
    )]
    pub tail: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_exit_of_an_older_host_names_its_pane_and_says_nothing_else() {
        let exit: PaneExit =
            serde_json::from_value(json!({ "id": "p1-zeus", "generation": 4 })).unwrap();
        assert_eq!(
            exit,
            PaneExit {
                id: "p1-zeus".to_owned(),
                generation: 4,
                exit_code: None,
                signal: None,
                tail: None,
                background: Vec::new(),
                survivor: None,
            }
        );
        assert_eq!(
            serde_json::to_value(&exit).unwrap(),
            json!({ "id": "p1-zeus", "generation": 4 }),
            "what is not said is not written"
        );
    }

    #[test]
    fn an_exit_carries_its_code_its_signal_and_the_lines_its_screen_ended_with() {
        let body = json!({
            "id": "p1-zeus",
            "generation": 4,
            "exitCode": 3,
            "signal": "Hangup: 1",
            "tail": ["No API key found", "Use /login"],
        });
        let exit: PaneExit = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(exit.exit_code, Some(3));
        assert_eq!(exit.signal.as_deref(), Some("Hangup: 1"));
        assert_eq!(
            exit.tail,
            Some(vec!["No API key found".to_owned(), "Use /login".to_owned()])
        );
        assert_eq!(serde_json::to_value(&exit).unwrap(), body);
    }

    #[test]
    fn an_exit_lists_what_the_window_left_running_and_an_older_host_lists_nothing() {
        let body = json!({
            "id": "p1-zeus",
            "generation": 4,
            "background": [{ "pid": 4242, "command": "python3 lock.py record --run 7" }],
        });
        let exit: PaneExit = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(
            exit.background,
            vec![BackgroundProcess {
                pid: 4242,
                command: "python3 lock.py record --run 7".to_owned()
            }]
        );
        assert_eq!(serde_json::to_value(&exit).unwrap(), body);
        // Not a list of processes: not said, and the exit stands.
        let unsaid: PaneExit = serde_json::from_value(
            json!({ "id": "p", "generation": 1, "background": [{ "pid": "4242" }] }),
        )
        .unwrap();
        assert_eq!(unsaid.background, Vec::new());
    }

    #[test]
    fn a_mark_says_how_the_harness_keeps_its_servers_only_when_it_is_known_and_an_older_daemons_says_nothing(
    ) {
        let older: MarkRequest =
            serde_json::from_value(json!({ "id": "p1-zeus", "generation": 4 })).unwrap();
        assert_eq!(
            older,
            MarkRequest {
                id: "p1-zeus".to_owned(),
                generation: 4,
                servers: Servers::Unknown,
                harness: None,
            }
        );
        assert_eq!(
            serde_json::to_value(&older).unwrap(),
            json!({ "id": "p1-zeus", "generation": 4 }),
            "the request is what it was for a harness that says no more"
        );

        for (word, servers) in [
            ("inGroup", Servers::InGroup),
            ("atLaunch", Servers::AtLaunch),
        ] {
            let body = json!({ "id": "p1-zeus", "generation": 4, "servers": word });
            let said: MarkRequest = serde_json::from_value(body.clone()).unwrap();
            assert_eq!(said.servers, servers);
            assert_eq!(serde_json::to_value(&said).unwrap(), body);
        }
    }

    #[test]
    fn a_mark_names_the_harness_only_where_the_daemon_knows_which_it_is() {
        let body =
            json!({ "id": "p1-zeus", "generation": 4, "servers": "atLaunch", "harness": "codex" });
        let said: MarkRequest = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(said.harness.as_deref(), Some("codex"));
        assert_eq!(serde_json::to_value(&said).unwrap(), body);

        let alone: MarkRequest =
            serde_json::from_value(json!({ "id": "p1-zeus", "generation": 4, "harness": "pi" }))
                .unwrap();
        assert_eq!(alone.servers, Servers::Unknown);
        assert_eq!(
            serde_json::to_value(&alone).unwrap(),
            json!({ "id": "p1-zeus", "generation": 4, "harness": "pi" })
        );
    }

    #[test]
    fn a_way_of_keeping_servers_this_host_does_not_know_is_not_known_and_the_mark_stands() {
        // A newer daemon's word: the window is marked, told by time alone.
        let said: MarkRequest = serde_json::from_value(
            json!({ "id": "p1-zeus", "generation": 4, "servers": "inTheMoon" }),
        )
        .unwrap();
        assert_eq!(said.servers, Servers::Unknown);
        assert!(said.servers.is_unknown());
        assert!(!Servers::InGroup.is_unknown() && !Servers::AtLaunch.is_unknown());
    }

    #[test]
    fn a_mark_names_its_pane_and_refuses_what_it_does_not_know() {
        for refused in [
            json!({ "generation": 4 }),
            json!({ "id": "p", "generation": "4" }),
            json!({ "id": "p", "generation": 4, "servers": 7 }),
            json!({ "id": "p", "generation": 4, "harness": 7 }),
            json!({ "id": "p", "generation": 4, "group": true }),
            // What the field was called before the second way was known.
            json!({ "id": "p", "generation": 4, "harnessGroup": true }),
        ] {
            assert!(
                serde_json::from_value::<MarkRequest>(refused.clone()).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn an_end_names_its_pane_and_no_process_and_refuses_what_it_does_not_know() {
        let body = json!({ "id": "p1-zeus", "generation": 4 });
        let asked: EndRequest = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(
            asked,
            EndRequest {
                id: "p1-zeus".to_owned(),
                generation: 4,
            }
        );
        assert_eq!(serde_json::to_value(&asked).unwrap(), body);

        for refused in [
            json!({ "generation": 4 }),
            json!({ "id": "p" }),
            json!({ "id": "p", "generation": "4" }),
            // The host chooses what it ends: a daemon that names processes asks for
            // what this one does not do.
            json!({ "id": "p", "generation": 4, "pids": [4242] }),
            json!({ "id": "p", "generation": 4, "all": true }),
        ] {
            assert!(
                serde_json::from_value::<EndRequest>(refused.clone()).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn an_ending_says_what_was_ended_and_what_was_left_and_a_word_that_is_missing_says_none() {
        let answer = json!({
            "ok": true,
            "ended": { "total": 2, "processes": [
                { "pid": 4242, "command": "python3 lock.py record --run 7" },
                { "pid": 4243, "command": "tail -f run.log" },
            ] },
            "left": { "total": 1, "processes": [{ "pid": 4300, "command": "node server.js" }] },
        });
        let ending: Ending = serde_json::from_value(answer).unwrap();
        assert_eq!(ending.ended.total, 2);
        assert_eq!(ending.ended.processes[1].command, "tail -f run.log");
        assert_eq!(ending.left.processes[0].pid, 4300);

        // A host that says no more than that it answered ended nothing and left nothing.
        let said: Ending = serde_json::from_value(json!({ "ok": true })).unwrap();
        assert_eq!(said, Ending::default());
        assert_eq!(said.ended.total, 0);
    }

    #[test]
    fn a_screen_that_showed_nothing_is_an_empty_tail_and_not_an_absent_one() {
        let said: PaneExit =
            serde_json::from_value(json!({ "id": "p", "generation": 1, "tail": [] })).unwrap();
        assert_eq!(said.tail, Some(Vec::new()));
        assert_eq!(serde_json::to_value(&said).unwrap()["tail"], json!([]));
    }

    #[test]
    fn a_field_that_is_not_what_it_should_be_was_not_said_and_the_exit_stands() {
        let exit: PaneExit = serde_json::from_value(json!({
            "id": "p1-zeus",
            "generation": 4,
            "exitCode": "3",
            "signal": 9,
            "tail": ["a", 7],
            "survivor": "4242",
        }))
        .unwrap();
        assert_eq!(
            exit,
            PaneExit {
                id: "p1-zeus".to_owned(),
                generation: 4,
                exit_code: None,
                signal: None,
                tail: None,
                background: Vec::new(),
                survivor: None,
            }
        );
        let survived: PaneExit = serde_json::from_value(json!({
            "id": "p1-zeus", "generation": 4, "survivor": 4242,
        }))
        .unwrap();
        assert_eq!(survived.survivor, Some(4242));
        // The pane is the one thing an exit must name.
        assert!(serde_json::from_value::<PaneExit>(json!({ "generation": 4 })).is_err());
        assert!(
            serde_json::from_value::<PaneExit>(json!({ "id": "p", "generation": "4" })).is_err()
        );
        let answer: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "tail": "not a list" })).unwrap();
        assert_eq!(answer, SnapshotTail::default());
    }

    #[test]
    fn a_snapshot_asks_for_the_tail_only_when_it_wants_it() {
        let plain = SnapshotRequest {
            id: "p1-zeus".to_owned(),
            generation: 2,
            tail: None,
            bottom: None,
        };
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({ "id": "p1-zeus", "generation": 2 }),
            "the request the adapters make is as it was"
        );
        let asking = SnapshotRequest {
            tail: Some(TAIL_LINES),
            ..plain
        };
        assert_eq!(
            serde_json::to_value(&asking).unwrap(),
            json!({ "id": "p1-zeus", "generation": 2, "tail": TAIL_LINES })
        );
        assert!(serde_json::from_value::<SnapshotRequest>(
            json!({ "id": "p", "generation": 1, "lines": 3 })
        )
        .is_err());
    }

    #[test]
    fn a_snapshot_asks_for_the_screen_as_drawn_only_when_it_wants_it() {
        let asking = SnapshotRequest {
            id: "p1-zeus".to_owned(),
            generation: 2,
            tail: None,
            bottom: Some(BOTTOM_LINES),
        };
        let body = json!({ "id": "p1-zeus", "generation": 2, "bottom": BOTTOM_LINES });
        assert_eq!(serde_json::to_value(&asking).unwrap(), body);
        assert_eq!(
            serde_json::from_value::<SnapshotRequest>(body).unwrap(),
            asking
        );
    }

    #[test]
    fn an_answer_without_a_tail_is_a_host_that_did_not_say() {
        let answer: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "unsent": false })).unwrap();
        assert_eq!(answer, SnapshotTail::default());
        let shown: SnapshotTail =
            serde_json::from_value(json!({ "ok": true, "tail": ["a"] })).unwrap();
        assert_eq!(shown.tail, Some(vec!["a".to_owned()]));
    }
}
