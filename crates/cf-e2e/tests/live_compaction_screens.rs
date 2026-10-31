//! Live: what a Claude Code's and a Pi's window draw while they compact, as the engine
//! reads a screen (`pane.snapshot`'s last lines). It is opt-in (`cargo xtask live
//! compaction-screens`): it spends the machine's quota on a real chief, and needs the
//! harness installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! The readers of a compaction's status line (`cf_harness`'s `shared::compacting`) were
//! written from the harnesses' own source for Claude Code and Pi, and from the field's
//! screen for Codex (T-901). Calliope's P3-2 of T-917 asked for one real screen of each:
//! a real harness is the chief of a project on the native daemon, as it ships, with the
//! stand-in as its one member; the human asks it something, to have an exchange to
//! compact, then types `/compact` into its window, and the run reads its screen every
//! fifth of a second until a line of it says it compacts. What it read is said whole in
//! the report, to be the readers' test (`shared/compacting/tests.rs`).
//!
//! ```text
//! cargo xtask live compaction-screens claude
//! cargo xtask live compaction-screens pi --pi-agent-dir <a folder holding a copy of Pi's login>
//! ```
//!
//! The run fails when the chief never answered, or its screen never said it compacts.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{find, member_as, stand_in_worker, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::{Error, Result};
use serde_json::{json, Value};

/// The stand-in for the member's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The human's keys: `/compact`, and the Enter that sends it.
const COMPACT: &[u8] = b"/compact";
const ENTER: &[u8] = b"\r";

/// The gap between keys as a person types them.
const KEYED: Duration = Duration::from_millis(600);

/// How long the chief has to answer, and its screen to say it compacts.
const ANSWER: Duration = Duration::from_secs(180);
const COMPACTING: Duration = Duration::from_secs(120);

/// How often the board and the screen are looked at.
const LOOK: Duration = Duration::from_millis(200);

/// The last lines of a window's screen, as the engine asks for them.
const TAIL: usize = 12;

/// The harnesses the run is asked for, Claude Code's and Pi's both where none is named.
fn harnesses() -> Result<Vec<Harness>> {
    let named = live::list("harness");
    if named.is_empty() {
        return Ok(vec![Harness::Claude, Harness::Pi]);
    }
    named
        .iter()
        .map(|name| match Harness::from_name(name) {
            Some(harness @ (Harness::Claude | Harness::Pi)) => Ok(harness),
            _ => Err(Error::Daemon(format!(
                "compaction-screens runs claude and pi, not {name}"
            ))),
        })
        .collect()
}

/// The last lines of the screen of `pane`, as the pane host draws it.
fn screen_of(members: &Members, pane: &Value) -> Vec<String> {
    let asked = json!({ "id": pane["id"], "generation": pane["generation"], "tail": TAIL });
    members
        .rig()
        .host("pane.snapshot", asked)
        .ok()
        .and_then(|answer| answer["tail"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|line| line.as_str().map(str::to_owned))
        .collect()
}

/// Whether one of `lines` says the harness compacts now, in any of the words either
/// harness says it in ("Compacting conversation…", "Compacting context...",
/// "Auto-compacting..."): the report is the judge of which. A compaction that failed
/// ("Compaction failed: Nothing to compact") is none.
fn says_it_compacts(lines: &[String]) -> bool {
    lines.iter().any(|line| line.contains("ompacting"))
}

/// Pi keeps the last 20,000 tokens of a conversation as they are (`compaction.
/// keepRecentTokens`), and says there is nothing to compact in a shorter one: the run's
/// own Pi folder (`--pi-agent-dir`) is told to keep one, so that one exchange compacts.
fn keep_one_token(folder: &Path) -> Result {
    let file = folder.join("settings.json");
    let mut settings: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    settings["compaction"] = json!({ "keepRecentTokens": 1 });
    cf_e2e::files::write(&file, format!("{settings}\n"))
}

fn check(harness: Harness, log: &Log) -> Result<Check> {
    let name = format!("{harness:?}").to_lowercase();
    if harness == Harness::Pi {
        let Some(folder) = live::option("pi-agent-dir") else {
            return Ok(Check::new(
                &name,
                "its screen while it compacts",
                false,
                "Pi runs on a folder of the run's own: --pi-agent-dir <a folder holding a copy of its login>",
            ));
        };
        keep_one_token(Path::new(&folder))?;
    }
    let plan = Plan::new(
        &format!("compaction-screens-{name}"),
        member_as(harness, "chief"),
    )
    .with(stand_in_worker());
    let members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let pane = members.lane("chief")?["pane"].clone();

    // An exchange to compact.
    members.paste(
        &pane,
        &ask::window_text("Reply with exactly one line: READY")?,
    )?;
    let started = Instant::now();
    let answered = find(log, "the chief to answer and be idle", ANSWER, LOOK, || {
        let idle = members.lane("chief")?["activity"]["state"] == "idle";
        Ok((idle && started.elapsed() > Duration::from_secs(5)).then_some(()))
    })?;
    let (ok, detail) = if answered.is_none() {
        (
            false,
            "the chief never answered its first message".to_owned(),
        )
    } else {
        for keys in [COMPACT, ENTER] {
            members.rig().types(&pane, keys)?;
            sleep(KEYED);
        }
        let seen = find(
            log,
            "its screen to say it compacts",
            COMPACTING,
            LOOK,
            || {
                let lines = screen_of(&members, &pane);
                Ok(says_it_compacts(&lines).then_some(lines))
            },
        )?;
        match seen {
            Some(lines) => {
                for line in &lines {
                    log.say(format!("{name} screen while compacting: {line:?}"));
                }
                (
                    true,
                    format!("its screen while compacting: {}", lines.join(" ⏎ ")),
                )
            }
            None => (
                false,
                format!(
                    "its screen never said it compacts in {} s: {}",
                    COMPACTING.as_secs(),
                    screen_of(&members, &pane).join(" ⏎ ")
                ),
            ),
        }
    };
    let (ok, detail) = match members.close() {
        Ok(()) => (ok, detail),
        Err(failed) => (false, format!("{detail}; the rig did not end: {failed}")),
    };
    Ok(Check::new(
        &name,
        "its screen while it compacts",
        ok,
        detail,
    ))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a real chief; cargo xtask live compaction-screens"]
fn a_claude_and_a_pi_window_say_on_their_screens_that_they_compact() -> Result {
    let log = Log::begin("live compaction-screens")?;
    let mut checks = Vec::new();
    for harness in harnesses()? {
        checks.push(check(harness, &log)?);
    }
    report::conclude(&log, "what the check came to", &checks)
}
