//! Live: a chief at rest, its box empty and the pane host's count up, is handed a
//! worker's result when Claude Code shows what it puts in an idle window: its prompt
//! suggestion in the empty box, and the recap it writes when the human is away. It is
//! opt-in (`cargo xtask live held-typed`): it opens a real Claude Code window and
//! spends three cheap turns and a recap, so it is ignored unless asked for and is no
//! part of `check` or of CI.
//!
//! The field case (T-839, 2026-10-27, ConsensFlow 3.0.6-alpha.2, Claude Code 2.1.310):
//! a chief's turn ended at 17:59:54, Claude wrote its recap (`away_summary`) at
//! 18:02:56, and three workers' results waited from 18:24 to 20:10, "you have typed in
//! this window and not sent it", though nobody had typed. Alpha.2 held every paste while
//! the pane host's count of the human's keys was up; alpha.3 reads the box on the screen
//! over the count (`input_box::read_unsent`). The owner's `tui` is fullscreen, but the
//! app's own settings for a window say `default` (`claude::install::settings`): the
//! window here is as the app opens it.
//!
//! A turn brings the window to rest, and the run waits for Claude Code's prompt
//! suggestion in its box (it makes none while the terminal is blurred, and backs off
//! after suggestions left unused). Claude Code writes its recap after three minutes
//! of a terminal that lost the focus, as it asks the terminal to say (`?1004h`), a
//! message of the conversation above the box: the run asks for one at once with
//! `/recap`, whose output is drawn there too (a focus-out of the run's own drew none in
//! nine minutes, 2.1.312). Keys are then typed as
//! the page types them (`pane.typed`) and cleared with two Escapes, which Claude Code
//! clears its box at and the count does not: the box empty, the count up, as in the
//! field. At each step the run reads the pane host's snapshot and asks the adapter's
//! own decision of it (`harness-ask unsent`), which the log shows with the screen's
//! last lines, the cursor and the bytes last printed: the screen the daemon decides
//! on. Last, the delivery: a paste told the box read empty must go in and be answered;
//! a decision of `held` fails the run.
//!
//! ```text
//! cargo xtask live held-typed
//! ```
//!
//! Isolation is `live escape-unsent`'s (`live_isolation/claude.rs`), with the
//! prompt suggestion and the recap on (as they are unless turned off).

#[path = "live_isolation/claude.rs"]
mod isolation;

use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, ask, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::Result;
use isolation::{claude_login, owners_claude, owners_unchanged, Throwaway};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder of the run's windows, below the live folders.
const FOLDER: &str = "held-typed";

/// What is typed and cleared: words no screen of Claude Code shows by itself.
const WORDS: &str = "zqv probe";

/// The words the turns ask for: none a screen shows by itself.
const RESTED: &str = "RESTEDOK";
const HANDED: &str = "HANDEDOK";

const KEY_GAP: Duration = Duration::from_millis(40);
const DOUBLE_PRESS: Duration = Duration::from_millis(150);
const KEYED: Duration = Duration::from_millis(600);
const ESCAPE: &[u8] = b"\x1b";
const ENTER: &[u8] = b"\r";
/// What a terminal says when its window loses the focus, to a program that asked
/// (`?1004h`): the page's emulator replies it, as no key. The field's chief was so.
const FOCUS_OUT: &[u8] = b"\x1b[O";

/// How long a window has to draw its prompt, its screen to hold still after a step,
/// Claude Code to draw its suggestion, and to write its recap.
const DRAWS: Duration = Duration::from_secs(120);
const STILL: Duration = Duration::from_millis(1500);
const SETTLE: Duration = Duration::from_secs(20);
const LOOK: Duration = Duration::from_millis(200);
const SUGGESTS: Duration = Duration::from_secs(45);
const RECAP: Duration = Duration::from_secs(120);

/// The lines of the screen as drawn the run reads: the whole of a 40-row window.
const BOTTOM: usize = 40;

/// How many of the last bytes a window printed the log shows at each step.
const RAW_TAIL: usize = 1_200;

/// What a step left: the pane host's word, the screen, and the adapter's decision.
struct Read {
    step: &'static str,
    /// Whether the pane host's count holds a paste.
    counted: bool,
    /// How many writes of the human's keys the host has taken.
    typed: u64,
    /// What the adapter reads of the box: `empty`, `holds`, or none.
    reader: Option<String>,
    /// The adapter's decision: `readEmpty N` or `held`.
    decision: Value,
    lines: Vec<String>,
    cursor: Value,
}

impl Read {
    fn said(&self) -> String {
        format!(
            "{}: {} typed={} box={} decision={}",
            self.step,
            if self.counted { "counted" } else { "clear" },
            self.typed,
            self.reader.as_deref().unwrap_or("unread"),
            self.decision
        )
    }

    /// The count of the human's writes at which the box read empty, where it did.
    fn read_empty(&self) -> Option<u64> {
        self.decision["readEmpty"].as_u64()
    }
}

/// Until `window`'s screen holds still for [`STILL`], or [`SETTLE`] passed.
fn settle(rig: &Rig, window: &Window) {
    let started = Instant::now();
    let (mut last, mut since) = (rig.screen(window.id()), Instant::now());
    while started.elapsed() < SETTLE {
        sleep(LOOK);
        let now = rig.screen(window.id());
        if now == last {
            if since.elapsed() >= STILL {
                return;
            }
        } else {
            (last, since) = (now, Instant::now());
        }
    }
}

/// Reads what `step` left in `window`, and logs the screen the decision was made on.
fn read(rig: &Rig, window: &Window, step: &'static str, log: &Log) -> Result<Read> {
    settle(rig, window);
    // The human's last key is a second past, as the adapter waits for it to be.
    sleep(Duration::from_millis(1_100));
    let snapshot = window.snapshot(BOTTOM)?;
    let lines: Vec<String> = snapshot["bottom"]
        .as_array()
        .map(|lines| lines.iter().map(live::text).collect())
        .unwrap_or_default();
    let reader = ask::question(&json!({
        "op": "input_box", "kind": "claude-code", "screen": lines, "cursor": snapshot["cursor"],
    }))?
    .as_str()
    .map(str::to_owned);
    let decision = ask::question(&json!({
        "op": "unsent", "kind": "claude-code", "snapshot": snapshot,
    }))?;
    let read = Read {
        step,
        counted: snapshot["unsent"] == true,
        typed: snapshot["typed"].as_u64().unwrap_or_default(),
        reader,
        decision,
        lines,
        cursor: snapshot["cursor"].clone(),
    };
    let printed = rig.printed(window.id());
    let last = String::from_utf8_lossy(&printed[printed.len().saturating_sub(RAW_TAIL)..]);
    log.block(
        &read.said(),
        &format!(
            "{}\ncursor: {}\ntypedQuietMs: {} outputQuietMs: {}\nbytes: {last:?}",
            read.lines.join("\n"),
            read.cursor,
            snapshot["typedQuietMs"],
            snapshot["outputQuietMs"]
        ),
    );
    Ok(read)
}

/// Whether a transcript of the config folder `config` has Claude Code's recap.
fn recapped(config: &Path) -> bool {
    let Ok(projects) = fs::read_dir(config.join("projects")) else {
        return false;
    };
    projects
        .filter_map(|entry| fs::read_dir(entry.ok()?.path()).ok())
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
        .any(|text| text.contains(r#""subtype":"away_summary""#))
}

/// Whether `lines` draw the recap `/recap` asked for: its command, and its output below
/// it, above the box (the recap Claude Code writes by itself, `away_summary`, is a message
/// of the conversation, drawn there too).
fn recap_drawn(lines: &[String]) -> bool {
    lines
        .iter()
        .skip_while(|line| line.trim() != "❯ /recap")
        .any(|line| line.trim_start().starts_with('⎿'))
}

/// Whether the last box `lines` draw shows anything after its mark.
fn box_shows(lines: &[String]) -> bool {
    lines
        .iter()
        .rev()
        .find_map(|line| line.trim_start().strip_prefix('❯'))
        .is_some_and(|after| !after.trim().is_empty())
}

/// The window's screen, by its lines.
fn screen_lines(window: &Window) -> Vec<String> {
    window.screen().lines().map(str::to_owned).collect()
}

/// Types `text` as the page does, a key at a time.
fn types(window: &Window, text: &str) -> Result {
    for key in text.chars() {
        window.types(key.to_string().as_bytes())?;
        sleep(KEY_GAP);
    }
    Ok(())
}

/// The window, from a turn to the delivery after the recap.
fn probe(rig: &Rig, install: &Installed, log: &Log) -> Result<Vec<Check>> {
    let who = "claude";
    let window = Open::new(rig, install, FOLDER, "held-typed").open_telling(log, who)?;
    if !window.still(DRAWS) {
        return Ok(vec![Check::failed(
            who,
            "opened",
            format!("did not draw: {}", window.tail()),
        )]);
    }
    let printed = String::from_utf8_lossy(&rig.printed(window.id())).into_owned();
    log.say(format!(
        "claude asked for focus reports: {}",
        printed.contains("\x1b[?1004h")
    ));
    let body = window.given(&format!(
        "Reply with exactly the word {RESTED} and nothing else."
    ))?;
    if !window.send(&body, RESTED)?.shown {
        return Ok(vec![Check::failed(
            who,
            "at rest",
            format!("the first turn was not answered: {}", window.tail()),
        )]);
    }
    let waited = Instant::now();
    while waited.elapsed() < SUGGESTS && !box_shows(&screen_lines(&window)) {
        sleep(LOOK);
    }
    let mut reads = vec![read(rig, &window, "suggested", log)?];
    let suggested = box_shows(&reads[0].lines);
    types(&window, "/recap")?;
    sleep(KEYED);
    window.types(ENTER)?;
    let config = window.claude_config();
    let waited = Instant::now();
    while !recapped(&config) && !recap_drawn(&screen_lines(&window)) && waited.elapsed() < RECAP {
        sleep(Duration::from_secs(2));
    }
    reads.push(read(rig, &window, "recapped", log)?);
    let recap = recapped(&config) || recap_drawn(&reads[1].lines);
    types(&window, WORDS)?;
    reads.push(read(rig, &window, "typed", log)?);
    window.types(ESCAPE)?;
    sleep(DOUBLE_PRESS);
    window.types(ESCAPE)?;
    window.press(FOCUS_OUT)?;
    let last = read(rig, &window, "away", log)?;
    let words_gone = !last.lines.iter().any(|line| line.contains(WORDS));
    let mut checks = vec![
        Check::passed(
            who,
            "what each step left",
            reads
                .iter()
                .chain([&last])
                .map(Read::said)
                .collect::<Vec<_>>()
                .join("; "),
        ),
        Check::new(
            who,
            "the shape formed: the count up, the words gone, the recap written",
            last.counted && words_gone && recap,
            format!(
                "count {}, words {}, recap {recap}, a suggestion drawn {suggested}",
                last.counted,
                if words_gone { "gone" } else { "shown" },
            ),
        ),
    ];
    const WHAT: &str = "a worker's result handed to the chief at rest";
    checks.push(match last.read_empty() {
        None => Check::failed(
            who,
            WHAT,
            format!(
                "HELD: the adapter decided {} on the screen logged under \"away\"",
                last.decision
            ),
        ),
        Some(typed) => {
            let body = window.given(&format!(
                "Reply with exactly the word {HANDED} and nothing else."
            ))?;
            let sent = window.send_read_empty(&body, HANDED, typed)?;
            if sent.written["ok"] != true {
                Check::failed(who, WHAT, format!("refused: {}", sent.written))
            } else if sent.shown {
                Check::passed(who, WHAT, format!("answered in {} s", sent.seconds))
            } else {
                Check::failed(
                    who,
                    WHAT,
                    format!("pasted and not answered: {}", window.tail()),
                )
            }
        }
    });
    window.kill();
    Ok(checks)
}

#[test]
#[ignore = "opt-in: opens a real Claude Code window and spends three cheap turns and a recap; cargo xtask live held-typed"]
fn a_chief_at_rest_that_claude_shows_its_suggestion_and_recap_is_handed_its_workers_result(
) -> Result {
    let log = Log::begin("live held-typed")?;
    ask::prepare()?;
    let install = log.found(Harness::Claude)?;
    let home = live::machine::home()?;
    let before = owners_claude(&home, FOLDER);
    let settings = json!({ "promptSuggestionEnabled": true, "awaySummaryEnabled": true });
    let config = Throwaway::holding(".credentials.json", claude_login()?)?
        .with(".claude.json", r#"{"hasCompletedOnboarding":true}"#)?
        .with("settings.json", &settings.to_string())?;
    let rig = Rig::start(live::rig_with_claude_config(
        std::slice::from_ref(&install),
        FAKE_AGENT,
        config.path(),
    ))?;
    let mut checks = probe(&rig, &install, &log)
        .unwrap_or_else(|failed| vec![Check::failed("claude", "probed", failed.to_string())]);
    rig.close()?;
    checks.push(config.check("claude"));
    checks.push(owners_unchanged(&home, FOLDER, &before));
    report::conclude(&log, "what the window came to", &checks)
}
