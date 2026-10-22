//! Live: what Devin writes of a message pasted into its window, and when, which the
//! daemon's word that Devin took a paste, or holds it queued, rests on. It is opt-in
//! (`cargo xtask live devin-wire`, `npm run live:devin-wire`): it spends the machine's
//! quota on a window of Devin's cheap model (or the one `--models devin=…` names), and
//! needs Devin installed and logged in, so it is ignored unless asked for and is no part
//! of `check` or of CI. It runs wherever the pane host does.
//!
//! The probe of 2026-10-21 (T-102, Devin 3000.11.92, and 3000.11.3, 3000.10.31 and
//! 3000.6.14 alike) found that Devin's wire log holds no request of its TUI's: a prompt
//! the TUI puts to its agent is not in it, so a word read from one is never had. What
//! Devin does write: its store's history row of what the TUI submitted, at the Enter
//! (3000.11.92), a paste queued behind a turn at work too; and the message's row, when
//! the turn takes it in. And the app's Stop, pressed while a paste waits queued, cancels
//! the turn and keeps the paste in the conversation, unanswered and never run, which the
//! agent reads in its next turn (3 of 3 runs): the daemon takes it for delivered (T-107).
//!
//! One window, as the daemon opens a member's (the adapter's launch, its first message
//! in its prompt file), and each message pasted as the daemon pastes it (a bracketed
//! paste, then Enter, through the pane host). It checks, and fails where Devin no longer
//! does so:
//! - a message pasted at rest: its history row, and the daemon's word that Devin took it
//!   (`harness-ask devin_took`, the adapter's own reading), within seconds;
//! - a message pasted while a turn is at work: its history row at the Enter, while its
//!   record does not have it, and the daemon's word that Devin took it then; its record
//!   when the turn takes it in;
//! - the app's Stop while a message waits queued: that message the head of the
//!   conversation's main chain, unanswered, never put to the agent (no update of the
//!   wire's of it), and the parent of the next message, so in the next turn's context;
//! - the wire log holds no request of the TUI's (`session/prompt`): a Devin that starts
//!   writing them shows here, and the daemon's reading may be made of them again.
//!
//! The words of the messages are the check's own; what Devin wrote of them is said in
//! times and counts, never in its words.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::devin_store;
use cf_e2e::live::members::Account;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Member, Open, Window};
use cf_e2e::live::{self, agents, ask, calls, secs, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

/// The stand-in for Claude, which a rig has, and the stand-in `cf` its windows run.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");
const FAKE_CF: &str = env!("CARGO_BIN_EXE_fake-cf");

/// The folder of the check, below the live folders.
const FOLDER: &str = "devin-wire";

/// The role the window's sessions start with.
const ROLE: &str =
    "# A check of ConsensFlow's\n\nDo exactly what each message says, and nothing more.";

/// How often the places Devin writes in are read, how long a turn may take, and how
/// soon after its Enter a message's history row must be there.
const LOOK: Duration = Duration::from_millis(200);
const TURN: Duration = Duration::from_secs(180);
const AT_ENTER: Duration = Duration::from_secs(3);

/// What a long turn is asked to do: a command of its shell that runs half a minute, which
/// a message pasted meanwhile waits behind (Devin takes it in when the command returns:
/// the field report of 2026-10-13, a 60 s command). How long it is let run before a message
/// is pasted behind it, and before the Stop is pressed behind that.
const LONG: &str = "Run this exact command with your shell tool: sleep 30\nThen reply with exactly SLEPT and nothing else.";
const AT_WORK: Duration = Duration::from_secs(5);
const QUEUED: Duration = Duration::from_secs(3);

/// A message of the check's, as the daemon pastes one: its header, then its words.
fn message(number: u32, words: &str) -> String {
    format!("[ConsensFlow m-{number} \u{b7} T-1 \u{b7} note from @chief]\n{words}")
}

/// The start of message `number`'s header, as its record shows it.
fn marker(number: u32) -> String {
    format!("[ConsensFlow m-{number} ")
}

/// The window and the places Devin writes in for it.
struct Watched<'a> {
    window: &'a Window<'a>,
    store: PathBuf,
    wire: PathBuf,
    env: Vec<(String, String)>,
}

/// A message's row in the store: its node, its parent, and who said it.
struct Node {
    node: String,
    parent: Option<String>,
    role: String,
    text: String,
}

impl Watched<'_> {
    fn session(&self) -> Option<String> {
        let _ = self.window.record();
        self.window.session()
    }

    /// The store, read-only, as the daemon reads it; none while it cannot be opened.
    fn db(&self) -> Option<Connection> {
        Connection::open_with_flags(&self.store, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
    }

    /// Whether the session's history has a row that holds `marker`.
    fn in_history(&self, marker: &str) -> bool {
        let (Some(db), Some(session)) = (self.db(), self.session()) else {
            return false;
        };
        db.query_row(
            "select count(*) from prompt_history where session_id = ? and instr(content, ?) > 0",
            (&session, marker),
            |row| row.get::<_, i64>(0),
        )
        .is_ok_and(|count| count > 0)
    }

    /// The session's messages, in the order Devin wrote them, and the node its main
    /// chain ends in.
    fn nodes(&self) -> (Vec<Node>, Option<String>) {
        let (Some(db), Some(session)) = (self.db(), self.session()) else {
            return (Vec::new(), None);
        };
        let head = db
            .query_row(
                "select cast(main_chain_id as text) from sessions where id = ?",
                [&session],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten();
        let mut nodes = Vec::new();
        if let Ok(mut statement) = db.prepare(
            "select cast(node_id as text), cast(parent_node_id as text), chat_message
             from message_nodes where session_id = ? order by row_id",
        ) {
            let rows = statement.query_map([&session], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            });
            for (node, parent, message) in rows.into_iter().flatten().flatten() {
                let message: Value = message
                    .and_then(|message| serde_json::from_str(&message).ok())
                    .unwrap_or_default();
                nodes.push(Node {
                    node: node.unwrap_or_default(),
                    parent,
                    role: message["role"].as_str().unwrap_or_default().to_owned(),
                    text: message["content"].to_string(),
                });
            }
        }
        (nodes, head)
    }

    /// Whether the record has the user's message that holds `marker`.
    fn recorded(&self, marker: &str) -> bool {
        self.nodes()
            .0
            .iter()
            .any(|node| node.role == "user" && node.text.contains(marker))
    }

    /// The wire log's records, each as JSON.
    fn wire(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.wire)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The daemon's word that Devin took `pasted`, handed over at `at_ms`.
    fn took(&self, pasted: &str, at_ms: i64) -> bool {
        self.session()
            .and_then(|session| ask::devin_took(&self.env, &session, pasted, at_ms).ok())
            .is_some_and(|read| read.took)
    }

    /// Pastes message `number` as the daemon does: the text it gave the window, and when.
    fn paste(&self, number: u32, words: &str) -> Result<(String, i64, Instant)> {
        let text = self.window.given(&message(number, words))?;
        let at_ms = jiff::Timestamp::now().as_millisecond();
        let at = Instant::now();
        self.window.paste(&text)?;
        Ok((text, at_ms, at))
    }

    /// How long after `from` `seen` held, looked at every [`LOOK`] for `within`.
    fn when(
        &self,
        from: Instant,
        within: Duration,
        mut seen: impl FnMut() -> bool,
    ) -> Option<Duration> {
        while from.elapsed() < within && !self.window.closed() {
            if seen() {
                return Some(from.elapsed());
            }
            sleep(LOOK);
        }
        None
    }

    /// Whether the session's last message is an answer: its turn over.
    fn answered(&self) -> bool {
        self.nodes()
            .0
            .iter()
            .rev()
            .find(|node| node.role == "user" || node.role == "assistant")
            .is_some_and(|node| node.role == "assistant")
    }

    /// Waits for the window to be at rest, its last message answered and its screen still.
    fn rest(&self) -> bool {
        self.when(Instant::now(), TURN, || self.answered())
            .is_some()
            && self.window.still(Duration::from_secs(30))
    }
}

/// The steps of the check, each saying what it saw and failing where Devin no longer
/// writes what the daemon rests on.
fn check(watched: &Watched, interrupt: &ask::Interrupt, said: &mut Account) -> Result<()> {
    let at = |seen: Option<Duration>| {
        seen.map_or_else(|| "never".to_owned(), |after| format!("{} s", secs(after)))
    };
    if !watched.rest() {
        said.fail("the window's first turn never came to rest");
        return Ok(());
    }

    // 1. A message pasted at rest.
    let (text, at_ms, pasted) =
        watched.paste(11, "Reply with exactly ACK-ONE and nothing else.")?;
    let history = watched.when(pasted, AT_ENTER, || watched.in_history(&marker(11)));
    let took = watched.when(pasted, AT_ENTER, || watched.took(&text, at_ms));
    let recorded = watched.when(pasted, TURN, || watched.recorded(&marker(11)));
    said.say(format!(
        "at rest: its history row after {}, the daemon's word that Devin took it after {}, its record after {}",
        at(history), at(took), at(recorded)
    ));
    if history.is_none() || took.is_none() {
        said.fail("a message pasted at rest had no history row of its own, or the daemon no word that Devin took it, within seconds");
    }
    if !watched.rest() {
        said.fail("the window did not come to rest after the first message");
        return Ok(());
    }

    // 2. A message pasted behind a turn at work.
    let (_, _, long) = watched.paste(12, LONG)?;
    if watched
        .when(long, TURN, || watched.recorded(&marker(12)))
        .is_none()
    {
        said.fail("the long turn never began");
        return Ok(());
    }
    sleep(AT_WORK);
    let (text, at_ms, pasted) =
        watched.paste(13, "Reply with exactly ACK-THREE and nothing else.")?;
    let history = watched.when(pasted, AT_ENTER, || watched.in_history(&marker(13)));
    let queued = history.is_some() && !watched.recorded(&marker(13));
    let took = watched.when(pasted, AT_ENTER, || watched.took(&text, at_ms));
    let recorded = watched.when(pasted, TURN, || watched.recorded(&marker(13)));
    said.say(format!(
        "queued behind a turn at work: its history row after {}, {} its record then; the daemon's word that Devin took it after {}; its record (taken in) after {}",
        at(history),
        if queued { "not yet in" } else { "already in" },
        at(took),
        at(recorded)
    ));
    if history.is_none() || took.is_none() {
        said.fail("a message pasted behind a turn at work had no history row at its Enter, or the daemon no word that Devin took it: the daemon's word that Devin holds it queued is gone");
    } else if !queued {
        said.say("the long turn took the message in at once: the queue was not seen (not a failure: the turn was short)");
    }
    if recorded.is_none() {
        said.fail("the turn never took in the message queued behind it");
    }
    if !watched.rest() {
        said.fail("the window did not come to rest after the queued message");
        return Ok(());
    }

    // 3. The app's Stop while a message waits queued.
    let (_, _, long) = watched.paste(14, LONG)?;
    if watched
        .when(long, TURN, || watched.recorded(&marker(14)))
        .is_none()
    {
        said.fail("the second long turn never began");
        return Ok(());
    }
    sleep(AT_WORK);
    watched.paste(15, "Reply with exactly ACK-FIVE and nothing else.")?;
    sleep(QUEUED);
    watched.window.interrupt(interrupt)?;
    sleep(Duration::from_secs(10));
    let _ = watched.window.still(Duration::from_secs(30));
    let (nodes, head) = watched.nodes();
    let queued = nodes
        .iter()
        .rfind(|node| node.role == "user" && node.text.contains(&marker(15)));
    let answered = queued.is_some_and(|queued| {
        nodes.iter().any(|node| {
            node.parent.as_deref() == Some(queued.node.as_str()) && node.role == "assistant"
        })
    });
    let put_to_the_agent = watched.wire().iter().any(|record| {
        record["update"]["sessionUpdate"] == "user_message_chunk"
            && record["update"]["content"]["text"]
                .as_str()
                .is_some_and(|text| text.contains(&marker(15)))
    });
    let head_is_it = queued.is_some_and(|queued| head.as_deref() == Some(queued.node.as_str()));
    said.say(format!(
        "the Stop with a message queued: the message {}; the head of the main chain {}; answered {answered}; put to the agent {put_to_the_agent}",
        if queued.is_some() { "in the store" } else { "nowhere in the store" },
        if head_is_it { "is it" } else { "is not it" }
    ));
    let ran = answered || put_to_the_agent;
    match queued {
        None => said.fail("a message the Stop cancelled while queued is not in the conversation: Devin drops it now, and the daemon, which takes it for delivered, must give it back (T-107)"),
        Some(_) if ran => said.fail("the message pasted behind the long turn ran: the Stop met no message queued (the turn took it in first), and what Devin does with one was not seen"),
        Some(_) if !head_is_it => said.fail("a message the Stop cancelled while queued is in the store, but not the head of the conversation"),
        Some(_) => {}
    }
    // The next message: is the one the Stop cancelled in its turn's context?
    let (_, _, next) = watched.paste(16, "Reply with exactly ACK-SIX and nothing else.")?;
    let shown = watched.when(next, TURN, || watched.recorded(&marker(16)));
    let (nodes, _) = watched.nodes();
    let parent = nodes
        .iter()
        .rfind(|node| node.role == "user" && node.text.contains(&marker(16)))
        .and_then(|node| node.parent.clone());
    let below = queued.is_some_and(|queued| parent.as_deref() == Some(queued.node.as_str()));
    said.say(format!(
        "the next message: recorded after {}, below the one the Stop cancelled {below}",
        at(shown)
    ));
    if queued.is_some() && !ran && shown.is_some() && !below {
        said.fail("the next message is not below the one the Stop cancelled: Devin keeps it out of the next turn's context now, and the daemon must give it back (T-107)");
    }
    let _ = watched.rest();

    // 4. The wire log holds no request of the TUI's.
    let wire = watched.wire();
    let prompts = wire
        .iter()
        .filter(|record| record["method"] == "session/prompt")
        .count();
    said.say(format!(
        "the wire log: {} records, {prompts} of them the TUI's prompts to its agent",
        wire.len()
    ));
    if prompts > 0 {
        said.fail("Devin's wire log holds the TUI's prompts now: the daemon's word that Devin took or holds a paste may be read from them again (T-102)");
    }
    Ok(())
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Devin; cargo xtask live devin-wire (npm run live:devin-wire)"]
fn what_devin_writes_of_a_paste_at_rest_queued_and_cut_by_the_stop_is_what_the_daemon_reads(
) -> Result {
    let log = Log::begin("live devin-wire")?;
    if let Some(other) = live::list("harness")
        .iter()
        .find(|word| word.as_str() != Harness::Devin.name())
    {
        return Err(Error::Daemon(format!(
            "this check runs Devin alone, not {other}"
        )));
    }
    ask::prepare()?;
    let install = log.found(Harness::Devin)?;
    let interrupt = ask::interrupt(Harness::Devin.kind())?;
    let stand_in = tempfile::tempdir().map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    calls::install(stand_in.path(), Path::new(FAKE_CF))?;
    let rig = Rig::start(install.rig(FAKE_AGENT).first_on_path(stand_in.path()))?;
    let mut said = Account::new();
    {
        let first = message(1, "Reply with exactly READY and nothing else.");
        let window = Open::new(&rig, &install, FOLDER, FOLDER)
            .agent(agents::agent(Harness::Devin))
            .member(Member::new("worker", ROLE).first_message(&first))
            .open(&log)?;
        let var = |name: &str| rig.var(name).map(str::to_owned);
        let home = rig
            .var("HOME")
            .or_else(|| rig.var("USERPROFILE"))
            .unwrap_or_default();
        let env: Vec<(String, String)> = ["HOME", "USERPROFILE", "APPDATA", "XDG_DATA_HOME"]
            .iter()
            .filter_map(|name| var(name).map(|value| ((*name).to_owned(), value)))
            .collect();
        let argv = window.argv();
        let wire = argv
            .iter()
            .position(|word| word == "--config")
            .and_then(|at| argv.get(at + 1))
            .map(|config| Path::new(config).with_file_name("wire.jsonl"))
            .unwrap_or_default();
        let watched = Watched {
            window: &window,
            store: devin_store::location(
                cfg!(windows),
                home,
                rig.var("APPDATA"),
                rig.var("XDG_DATA_HOME"),
            ),
            wire,
            env,
        };
        if let Err(failed) = check(&watched, &interrupt, &mut said) {
            said.fail(failed);
        }
        window.kill();
    }
    said.closed(rig.close());
    let held = said.held();
    log.block(
        &format!("== devin wire{}", if held { "" } else { " (failed)" }),
        &said.text(),
    );
    report::conclude(
        &log,
        "what Devin writes of a paste",
        &[Check::new(
            "devin",
            "wire",
            held,
            install.version().to_string(),
        )],
    )
}
