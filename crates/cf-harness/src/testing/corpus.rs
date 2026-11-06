//! A corpus of screens, labelled, and the differential test that holds the release bar of T-1243:
//! **no screen that `5b7acff7f` reads as no dialog may read as one now, on any harness.**
//!
//! `5b7acff7f` is 3.0.7-alpha.2's release head. T-1240 fixed the engine's reading of a harness's own
//! idle prompt as a dialog (Codex's composer, its footer and the last line of the conversation, read by
//! their shape as a list of choices with a worker's question above them as its words), and its first
//! two tries each made a false dialog that the release head did not read (Pi's and Muse's numbered
//! transcript text; Codex's footer, which the user configures: a branch called `enter`). A fix that reads
//! one more screen as a dialog holds a window that waits for nothing, so it has to be shown not to,
//! over screens that are not the one the bug was found on.
//!
//! **The screens** are what the engine reads of a window: the pane host's tail, the last twelve lines
//! that say something. Every one is cut from the rows as the harness draws them, empty rows and rules
//! in, with the pane host's own filter ([`cf_proto::panes::word_tail`], which its `Screen::tail` is):
//! - **recorded**: the real screens the harnesses drew in the probes of T-model-switch (Claude Code,
//!   Codex, Devin and Pi), each labelled by its harness and its state, `idle` (its prompt is drawn) or
//!   `dialog` (a picker, a question, a menu waits for a key); scrubbed of the owner's paths, ids and
//!   figures (`recorded.txt`);
//! - **rendered**: Pi 1.5.0's own renderings of the markdown of `bodies.txt` (`pi_rendered.txt`), made
//!   offline with its message components, no model and no session. Conversation;
//! - **generated**: each body (a question, a numbered list, a quoted list, a fenced menu, a user's
//!   numbered message…) drawn in each harness's style, alone (conversation) and over each of that
//!   harness's bottoms at rest or at work (idle: its prompt, its status, the words typed in its box, a
//!   folder or a branch called `enter`);
//! - **the fixtures** of [`super::screens`]: the idle screens of T-1240 and the dialogs the inventory
//!   lists (idle, and dialog), two of them (Devin's theme offer, Muse's quota card) never recorded and
//!   drawn in the shape of each harness's own lists, which is a guess.
//!
//! **The assertions** ([`audit`], held by a test on each harness's window): a window reads each screen as
//! the release head's window did ([`base_dialog::base_window_dialog`]: its reader, frozen, and the
//! window's `shows_prompt`, which is borrowed and pinned) or as less.
//! - no screen of any window reads as a dialog where the release head read none;
//! - a screen labelled `dialog` reads, on every window, exactly as the release head read it: the whole
//!   [`Dialog`], its timeout and its prompt below included. The one exception is the window of Codex,
//!   which reads no dialog by its shape at all in alpha.2 (`CodexWindow::dialog`): there it reads none,
//!   which the audit counts apart as not read by design;
//! - only screens labelled `idle` or `conversation` may read less, and the idle prompts that no longer
//!   read as a dialog the engine counts are counted from that label;
//! - an idle screen of the window's own harness is no dialog the engine counts, but for the few the
//!   corpus names as staying what they were (`Case::stays`).

use cf_proto::agents::Harness;
use cf_proto::panes::{word_tail, TAIL_LINES};

use super::base_dialog::base_window_dialog;
use super::screens as fixtures;
use crate::contract::{Dialog, Window};

/// What the corpus's bodies are: `## <id> <role>` and their words (`bodies.txt`).
const BODIES: &str = include_str!("corpus/bodies.txt");
/// Pi 1.5.0's renderings of those bodies, rows as drawn.
const PI_RENDERED: &str = include_str!("corpus/pi_rendered.txt");
/// The recorded screens, rows as drawn.
const RECORDED: &str = include_str!("corpus/recorded.txt");

/// What a screen of the corpus shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The window of a harness at rest or at work, its prompt drawn: no dialog is on it.
    Idle,
    /// Words of a conversation, no prompt: no dialog is on it.
    Conversation,
    /// A dialog its harness draws, which waits for a key.
    Dialog,
}

/// Where a screen comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Recorded,
    Rendered,
    Generated,
    Fixture,
}

/// One screen.
#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    /// The harness whose window draws it (the one that has a screen of it).
    pub harness: Harness,
    pub state: State,
    pub source: Source,
    /// The last twelve lines that say something: what the engine reads.
    pub lines: Vec<String>,
    /// An idle prompt the release head read as a dialog that nothing in the window's reach tells from
    /// one: a false dialog that stays as it was (a Devin box with no rule over it in the tail, Pi's box
    /// holding a quoted list), a follow-up of T-1243 and no regression.
    pub stays: bool,
}

/// Whether the engine holds a window for `dialog`, once it has read it twice: it asks, and no
/// prompt is drawn below its choices (`cf_engine::dialogs`).
pub fn counts(dialog: &Option<Dialog>) -> bool {
    dialog
        .as_ref()
        .is_some_and(|dialog| dialog.asks && !dialog.prompt_below)
}

// ─── The rows, and the pane host's filter ──────────────────────────────────────────────────────

/// The last twelve lines that say something of `rows`, as the pane host sends them.
fn cut(rows: Vec<String>) -> Vec<String> {
    word_tail(
        rows.into_iter().map(|row| row.trim_end().to_owned()),
        TAIL_LINES,
    )
}

/// `rows` of the screen of a window as strings.
fn rows(drawn: &[&str]) -> Vec<String> {
    drawn.iter().map(|row| (*row).to_owned()).collect()
}

/// The sections of a data file: `=== <title>` and the rows under it.
fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut all: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("=== ") {
            all.push((title.to_owned(), Vec::new()));
        } else if !line.starts_with('#') {
            if let Some(last) = all.last_mut() {
                last.1.push(line.to_owned());
            }
        }
    }
    all
}

// ─── The bodies, and their style in each harness ───────────────────────────────────────────────

/// A body: what a message says, as markdown.
#[derive(Debug, Clone)]
pub struct Body {
    pub id: String,
    pub user: bool,
    pub lines: Vec<String>,
}

/// The bodies of `bodies.txt`.
pub fn bodies() -> Vec<Body> {
    let mut all: Vec<Body> = Vec::new();
    for line in BODIES.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let (id, role) = head.split_once(' ').unwrap_or((head, "assistant"));
            all.push(Body {
                id: id.to_owned(),
                user: role == "user",
                lines: Vec::new(),
            });
        } else if let Some(last) = all.last_mut() {
            last.lines.push(line.to_owned());
        }
    }
    for body in &mut all {
        while body.lines.last().is_some_and(String::is_empty) {
            body.lines.pop();
        }
    }
    all
}

/// How a harness draws what a conversation says: the mark of the first line of a message of the
/// model's and of the user's, and the column the rest are in.
struct Style {
    assistant: &'static str,
    user: &'static str,
    rest: &'static str,
}

fn style(harness: Harness) -> Style {
    match harness {
        Harness::Codex => Style {
            assistant: "• ",
            user: "› ",
            rest: "  ",
        },
        Harness::Claude => Style {
            assistant: "⏺ ",
            user: "❯ ",
            rest: "  ",
        },
        Harness::Devin => Style {
            assistant: " ⏺ ",
            user: "  ",
            rest: "  ",
        },
        Harness::Muse => Style {
            assistant: "◆ ",
            user: "❯ ",
            rest: "  ",
        },
        Harness::Pi => Style {
            assistant: " ",
            user: " ",
            rest: " ",
        },
    }
}

/// `body` as `harness` draws it, a blank row between its paragraphs. Pi's is its own renderer's.
fn drawn(harness: Harness, body: &Body) -> (Vec<String>, Source) {
    if harness == Harness::Pi {
        let rendered = sections(PI_RENDERED)
            .into_iter()
            .find(|(id, _)| *id == body.id)
            .map(|(_, rows)| rows)
            .unwrap_or_default();
        return (rendered, Source::Rendered);
    }
    let style = style(harness);
    let mut first = true;
    let mut out = Vec::new();
    for line in &body.lines {
        if line.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mark = match (first, body.user) {
            (true, true) => style.user,
            (true, false) => style.assistant,
            (false, _) => style.rest,
        };
        out.push(format!("{mark}{line}"));
        first = false;
    }
    (out, Source::Generated)
}

// ─── The bottoms of each harness, rows as it draws them ────────────────────────────────────────

/// The last rows of a window at rest or at work, as the harness draws them: the empty rows between
/// Codex's lines, the rules round Devin's, Muse's, Pi's and Claude Code's boxes.
fn bottoms(harness: Harness) -> Vec<(&'static str, Vec<String>)> {
    match harness {
        Harness::Codex => {
            let footer = "  GPT-5.6-Luna low · ~/work/app";
            let keys = "  ? for shortcuts";
            let at = |folder: &str| format!("  GPT-5.6-Luna low · {folder}");
            let rest = |composer: &[&str], footer: &str, keys: Option<&str>| -> Vec<String> {
                let mut rows = rows(&["", "  Worked for 5s • 16:00", "", ""]);
                rows.extend(composer.iter().map(|row| (*row).to_owned()));
                rows.push(String::new());
                rows.push(footer.to_owned());
                rows.extend(keys.map(str::to_owned));
                rows
            };
            let draft = ["› 1. Use the table", "  2. Test it first"];
            vec![
                (
                    "rest",
                    rest(&["› Ask Codex to do anything"], footer, Some(keys)),
                ),
                (
                    "rest, no keys",
                    rest(&["› Ask Codex to do anything"], footer, None),
                ),
                (
                    "working",
                    rows(&[
                        "• Working (4s • esc to interrupt)",
                        "",
                        "› Ask Codex to do anything",
                        "",
                        footer,
                        keys,
                    ]),
                ),
                (
                    "typed two lines",
                    rest(
                        &["› Use JSON for the reply", "  and keep it short"],
                        footer,
                        Some(keys),
                    ),
                ),
                ("typed numbered", rest(&draft, footer, Some(keys))),
                (
                    "typed numbered, folder enter",
                    rest(&draft, &at("~/work/enter"), Some(keys)),
                ),
                (
                    "typed numbered, folder esc",
                    rest(&draft, &at("~/work/esc"), Some(keys)),
                ),
                (
                    "typed numbered, folder esc-demo",
                    rest(&draft, &at("~/work/esc-demo"), Some(keys)),
                ),
                (
                    "typed numbered, folder enter-esc",
                    rest(&draft, &at("~/work/enter-esc"), Some(keys)),
                ),
                (
                    "typed numbered, a title that says enter",
                    rest(
                        &draft,
                        "  GPT-5.6-Luna low · ~/work/app · Press enter to confirm",
                        None,
                    ),
                ),
                (
                    "typed numbered, keys of the composer naming esc",
                    rest(
                        &draft,
                        footer,
                        Some("  ⌃c to quit · esc esc to edit previous message"),
                    ),
                ),
                (
                    "rest, folder enter, no keys",
                    rest(&["› Ask Codex to do anything"], &at("~/work/enter"), None),
                ),
                // The footer the user configures (`tui.status_line`), and no line of keys under a
                // draft: Codex hides it (T-1243, the second review of `fd1837385`).
                (
                    "draft over a branch called enter",
                    rest(&draft, "  enter", None),
                ),
                (
                    "draft over a branch called esc",
                    rest(&draft, "  esc", None),
                ),
                (
                    "draft over the model and a branch called enter",
                    rest(&draft, "  GPT-5.6-Luna low · enter", None),
                ),
                (
                    "draft over a branch called esc-demo",
                    rest(&draft, "  esc-demo", None),
                ),
                (
                    "plain draft over a branch called enter",
                    rest(&["› Use the table", "  and test it first"], "  enter", None),
                ),
                (
                    "plain draft over a branch called esc",
                    rest(&["› Use the table", "  and test it first"], "  esc", None),
                ),
            ]
        }
        Harness::Devin => {
            let titled =
                "───────────────────────────── Answer the worker’s question about the format… ─";
            let moded = "──────── (bypass permissions on) ─";
            let plain =
                "────────────────────────────────────────────────────────────────────────────────";
            let footer =
                "SWE-2 Medium · bypass                            Context: 55k / 555k tokens (5%)";
            let hint = "❭ Ask Devin to build features, fix bugs, or work on your code";
            vec![
                (
                    "rest",
                    rows(&[
                        "",
                        titled,
                        hint,
                        plain,
                        footer,
                        "Update v3000.11.234 available!",
                    ]),
                ),
                (
                    "typed two lines",
                    rows(&[
                        "",
                        titled,
                        "❭ Use JSON for the reply",
                        "  and keep it short",
                        plain,
                        footer,
                    ]),
                ),
                (
                    "typed numbered",
                    rows(&[
                        "",
                        titled,
                        "❭ 1. Use the table",
                        "  2. Test it first",
                        plain,
                        footer,
                    ]),
                ),
                (
                    "rule with the mode",
                    rows(&["", moded, hint, plain, footer]),
                ),
                ("no rule in the tail", rows(&["", hint, plain, footer])),
                (
                    "typed two lines, no rule in the tail",
                    rows(&[
                        "",
                        "❭ Use JSON for the reply",
                        "  and keep it short",
                        plain,
                        footer,
                    ]),
                ),
            ]
        }
        Harness::Muse => {
            let rule =
                "── Voice input (⌥ + v to start) ────────────────────────────────────────────────";
            let plain =
                "────────────────────────────────────────────────────────────────────────────────";
            let status = "  muse-spark-1.3 · minimal · /p/w/app · YOLO";
            let tip = "❯ Ask to monitor a running build or log and get pinged on changes";
            vec![
                ("empty box", rows(&["", rule, "❯", plain, status])),
                ("a tip", rows(&["", rule, tip, plain, status])),
                (
                    "typed two lines",
                    rows(&[
                        "",
                        rule,
                        "❯ Use JSON for the reply",
                        "  and keep it short",
                        plain,
                        status,
                    ]),
                ),
                (
                    "typed numbered",
                    rows(&[
                        "",
                        rule,
                        "❯ 1. Use the table",
                        "  2. Test it first",
                        plain,
                        status,
                    ]),
                ),
                (
                    "status with a folder enter",
                    rows(&[
                        "",
                        rule,
                        tip,
                        plain,
                        "  muse-spark-1.3 · minimal · /p/w/enter · YOLO",
                    ]),
                ),
                (
                    "status with a folder esc-demo",
                    rows(&[
                        "",
                        rule,
                        tip,
                        plain,
                        "  muse-spark-1.3 · minimal · /p/w/esc-demo · YOLO",
                    ]),
                ),
            ]
        }
        Harness::Pi => {
            let rule = "────────────────────────────────────────────────────────────────────────────────────────";
            let stats = "↑5.5k ↓555 R55k CH55.5% $5.555 5.5%/5.5M (auto)                       meta/muse-spark-1.3 • low";
            let editor = |typed: &[&str], folder: &str| -> Vec<String> {
                let mut out = rows(&["", rule]);
                if typed.is_empty() {
                    out.push(String::new());
                }
                out.extend(typed.iter().map(|row| (*row).to_owned()));
                out.extend(rows(&[rule, folder, stats]));
                out
            };
            vec![
                ("rest", editor(&[], "/work/app")),
                ("folder enter", editor(&[], "/work/enter")),
                (
                    "typed two lines",
                    editor(
                        &[" Use JSON for the reply", " and keep it short"],
                        "/work/app",
                    ),
                ),
                (
                    "typed numbered",
                    editor(&[" 1. Use the table", " 2. Test it first"], "/work/app"),
                ),
                (
                    "typed quote",
                    editor(&[" > 1. Use the table", "   2. Test it first"], "/work/app"),
                ),
            ]
        }
        Harness::Claude => {
            let rule = "────────────────────────────────────────────────────────────────────────────────────────";
            let prompt = "  ⏵⏵ bypass permissions on (shift+tab to cycle)";
            vec![
                ("rest", rows(&["", rule, "❯ ", rule, prompt])),
                (
                    "working",
                    rows(&[
                        "",
                        "✻ Thinking… (12s · esc to interrupt)",
                        "",
                        rule,
                        "❯ ",
                        rule,
                        prompt,
                    ]),
                ),
                (
                    "typed two lines",
                    rows(&[
                        "",
                        rule,
                        "❯ Use JSON for the reply",
                        "  and keep it short",
                        rule,
                        prompt,
                    ]),
                ),
                (
                    "typed numbered",
                    rows(&[
                        "",
                        rule,
                        "❯ 1. Use the table",
                        "  2. Test it first",
                        rule,
                        prompt,
                    ]),
                ),
            ]
        }
    }
}

/// The bottoms whose false dialog the release head had and nothing in the window's reach can rule out:
/// Devin's box when the rule that says its mode is not in the tail, and Pi's box holding a quoted list
/// (Pi's editor draws no mark of its own, and a menu of its extensions keeps its footer too).
fn stays(bottom: &str) -> bool {
    bottom.contains("no rule in the tail") || bottom.contains("typed quote")
}

/// Every harness the corpus draws.
pub const HARNESSES: [Harness; 5] = [
    Harness::Codex,
    Harness::Devin,
    Harness::Muse,
    Harness::Pi,
    Harness::Claude,
];

fn name_of(harness: Harness) -> &'static str {
    match harness {
        Harness::Codex => "codex",
        Harness::Devin => "devin",
        Harness::Muse => "muse",
        Harness::Pi => "pi",
        Harness::Claude => "claude",
    }
}

fn harness_named(name: &str) -> Option<Harness> {
    HARNESSES
        .into_iter()
        .find(|harness| name_of(*harness) == name)
}

// ─── The corpus ────────────────────────────────────────────────────────────────────────────────

/// Every screen of the corpus.
pub fn corpus() -> Vec<Case> {
    let mut all: Vec<Case> = Vec::new();
    let bodies = bodies();
    // Each body, in each harness's style, alone and over each of that harness's bottoms.
    for harness in HARNESSES {
        let name = name_of(harness);
        for body in &bodies {
            let (said, source) = drawn(harness, body);
            all.push(Case {
                name: format!("{name}: {} (conversation)", body.id),
                harness,
                state: State::Conversation,
                source,
                lines: cut(said.clone()),
                stays: false,
            });
            for (bottom, bottom_rows) in bottoms(harness) {
                let mut rows = said.clone();
                rows.extend(bottom_rows);
                all.push(Case {
                    name: format!("{name}: {} over its prompt, {bottom}", body.id),
                    harness,
                    state: State::Idle,
                    source,
                    lines: cut(rows),
                    stays: stays(bottom),
                });
            }
        }
        // A bottom alone, as a window that has said nothing yet shows it.
        for (bottom, bottom_rows) in bottoms(harness) {
            all.push(Case {
                name: format!("{name}: its prompt alone, {bottom}"),
                harness,
                state: State::Idle,
                source: Source::Generated,
                lines: cut(bottom_rows),
                stays: stays(bottom),
            });
        }
    }
    // The idle screens of T-1240 and T-1243, and the dialogs, as the fixtures of `screens` have them.
    for (name, harness, screen) in [
        (
            "codex at rest after a question",
            Harness::Codex,
            fixtures::CODEX_IDLE_AFTER_QUESTION,
        ),
        (
            "codex working after a question",
            Harness::Codex,
            fixtures::CODEX_WORKING_AFTER_QUESTION,
        ),
        (
            "codex composer holding lines",
            Harness::Codex,
            fixtures::CODEX_COMPOSER_HOLDING_LINES,
        ),
        (
            "codex composer holding numbered lines",
            Harness::Codex,
            fixtures::CODEX_COMPOSER_HOLDING_NUMBERED_LINES,
        ),
        (
            "codex draft over a branch called enter",
            Harness::Codex,
            fixtures::CODEX_DRAFT_OVER_BRANCH_ENTER,
        ),
        (
            "codex draft over a branch called esc",
            Harness::Codex,
            fixtures::CODEX_DRAFT_OVER_BRANCH_ESC,
        ),
        (
            "codex draft over the model and a branch called enter",
            Harness::Codex,
            fixtures::CODEX_DRAFT_OVER_MODEL_AND_BRANCH_ENTER,
        ),
        (
            "devin at rest after a question",
            Harness::Devin,
            fixtures::DEVIN_IDLE_AFTER_QUESTION,
        ),
        (
            "devin on the question",
            Harness::Devin,
            fixtures::DEVIN_ON_THE_QUESTION,
        ),
        (
            "devin box holding lines",
            Harness::Devin,
            fixtures::DEVIN_BOX_HOLDING_LINES,
        ),
        (
            "muse at rest after a question",
            Harness::Muse,
            fixtures::MUSE_IDLE_AFTER_QUESTION,
        ),
        (
            "muse box holding lines",
            Harness::Muse,
            fixtures::MUSE_BOX_HOLDING_LINES,
        ),
        (
            "muse at rest after a numbered echo",
            Harness::Muse,
            fixtures::MUSE_NUMBERED_ECHO_AT_REST,
        ),
        (
            "pi at rest after a question",
            Harness::Pi,
            fixtures::PI_IDLE_AFTER_QUESTION,
        ),
        (
            "pi at rest after a numbered quoted example",
            Harness::Pi,
            fixtures::PI_QUOTED_EXAMPLE_AT_REST,
        ),
        (
            "claude at rest after a question",
            Harness::Claude,
            fixtures::CLAUDE_IDLE_AFTER_QUESTION,
        ),
        (
            "claude box holding lines",
            Harness::Claude,
            fixtures::CLAUDE_BOX_HOLDING_LINES,
        ),
    ] {
        all.push(Case {
            name: format!("fixture: {name}"),
            harness,
            state: State::Idle,
            source: Source::Fixture,
            lines: cut(fixtures::lines(screen)),
            stays: false,
        });
    }
    for (name, harness, screen) in [
        (
            "claude's offer to install an LSP plugin",
            Harness::Claude,
            fixtures::CLAUDE_LSP,
        ),
        (
            "claude's ask of a hook",
            Harness::Claude,
            fixtures::CLAUDE_HOOK_ASK,
        ),
        (
            "devin's dialog of a call",
            Harness::Devin,
            fixtures::DEVIN_CALL,
        ),
        (
            "devin's offer to switch its theme",
            Harness::Devin,
            fixtures::DEVIN_THEME,
        ),
        (
            "muse's card of its quota, framed",
            Harness::Muse,
            fixtures::MUSE_QUOTA,
        ),
        (
            "muse's card of its quota, unframed",
            Harness::Muse,
            fixtures::MUSE_QUOTA_UNFRAMED,
        ),
        (
            "codex's nudge to switch models",
            Harness::Codex,
            fixtures::CODEX_NUDGE,
        ),
        (
            "codex's picker of a model",
            Harness::Codex,
            fixtures::CODEX_PICKER,
        ),
    ] {
        all.push(Case {
            name: format!("fixture: {name}"),
            harness,
            state: State::Dialog,
            source: Source::Fixture,
            lines: cut(fixtures::lines(screen)),
            stays: false,
        });
    }
    // The real screens, labelled in their headers: `<harness> <file> <state>`.
    for (title, screen) in sections(RECORDED) {
        let mut words = title.split_whitespace();
        let (Some(harness), Some(_), Some(state)) = (words.next(), words.next(), words.next())
        else {
            panic!("a recorded screen's header is `<harness> <file> <state>`: {title}");
        };
        all.push(Case {
            name: format!("recorded: {title}"),
            harness: harness_named(harness).unwrap_or_else(|| panic!("{title}: no such harness")),
            state: match state {
                "idle" => State::Idle,
                "dialog" => State::Dialog,
                other => panic!("{title}: no such state {other}"),
            },
            source: Source::Recorded,
            lines: cut(screen),
            stays: false,
        });
    }
    all
}

/// How many screens of each state and source the corpus holds: what a test pins so that what the
/// docs and the reports say of it is what it is.
pub fn labelled_counts() -> Vec<(State, Source, usize)> {
    let corpus = corpus();
    let mut counts = Vec::new();
    for state in [State::Idle, State::Conversation, State::Dialog] {
        for source in [
            Source::Recorded,
            Source::Rendered,
            Source::Generated,
            Source::Fixture,
        ] {
            let n = corpus
                .iter()
                .filter(|case| case.state == state && case.source == source)
                .count();
            if n > 0 {
                counts.push((state, source, n));
            }
        }
    }
    counts
}

// ─── The audit ─────────────────────────────────────────────────────────────────────────────────

/// What a window made of the corpus, beside the release head's window.
#[derive(Debug, Default)]
pub struct Audit {
    /// How many screens.
    pub screens: usize,
    /// How many of each label: idle, conversation, dialog.
    pub idle: usize,
    pub conversation: usize,
    pub dialog: usize,
    /// Read exactly as the release head's window read them.
    pub same: usize,
    /// Idle prompts the release head read as a dialog the engine counts, and the window does not.
    pub idle_no_longer_a_dialog: usize,
    /// Idle prompts the release head read as something that is no dialog the engine counts, and the
    /// window reads as none.
    pub idle_read_less: usize,
    /// Conversations read less.
    pub conversation_read_less: usize,
    /// Dialogs the window reads none of by design (Codex's window, in alpha.2), which the release head
    /// read through it.
    pub dialogs_not_read_by_design: usize,
    /// Idle screens of the window's own harness that read as a dialog the engine counts, as they did at
    /// the release head, and the corpus says nothing in the window's reach can rule out.
    pub stays: usize,
    /// Idle screens of the window's own harness that it still reads as a dialog the engine counts.
    pub unfixed: Vec<String>,
    /// Screens read otherwise than the assertions allow.
    pub regressions: Vec<String>,
    /// The recorded and fixture screens read differently from the release head, each with how.
    pub changed: Vec<(String, &'static str)>,
}

/// Reads every screen of the corpus with `window`, the window of `harness`, and with the release
/// head's window, and says how they differ. `reads_none_by_design` is true of the window of a harness
/// that reads no dialog by its shape (Codex's).
pub fn audit(window: &dyn Window, harness: Harness, reads_none_by_design: bool) -> Audit {
    let mut audit = Audit::default();
    for case in corpus() {
        audit.screens += 1;
        match case.state {
            State::Idle => audit.idle += 1,
            State::Conversation => audit.conversation += 1,
            State::Dialog => audit.dialog += 1,
        }
        let base = base_window_dialog(harness, window, &case.lines);
        let now = window.dialog(&case.lines);
        let shown = || case.lines.join("\n");
        let own_idle = case.state == State::Idle && case.harness == harness;
        if base == now {
            audit.same += 1;
            if own_idle && counts(&now) {
                if case.stays {
                    audit.stays += 1;
                } else {
                    audit
                        .unfixed
                        .push(format!("{}: {now:?}\n{}", case.name, shown()));
                }
            }
            continue;
        }
        let regression = |why: &str| {
            format!(
                "{}: {why} (window of {}):\n  release head: {base:?}\n  now: {now:?}\n{}",
                case.name,
                name_of(harness),
                shown()
            )
        };
        // Never a dialog where the release head read none.
        if base.is_none() {
            audit
                .regressions
                .push(regression("reads a dialog the release head did not"));
            continue;
        }
        let how = match case.state {
            // A dialog reads as the release head read it, but through the window of a harness that
            // reads none by design.
            State::Dialog if reads_none_by_design && now.is_none() => {
                audit.dialogs_not_read_by_design += 1;
                continue;
            }
            State::Dialog => {
                audit.regressions.push(regression(
                    "a dialog reads otherwise than the release head read it",
                ));
                continue;
            }
            // Idle and conversation may read less: none, or no longer a dialog the engine counts.
            State::Idle | State::Conversation => {
                if now.is_none() || (counts(&base) && !counts(&now)) {
                    if case.state == State::Conversation {
                        audit.conversation_read_less += 1;
                        "conversation read less"
                    } else if counts(&base) {
                        audit.idle_no_longer_a_dialog += 1;
                        "idle prompt no longer a dialog"
                    } else {
                        audit.idle_read_less += 1;
                        "idle read less"
                    }
                } else {
                    audit.regressions.push(regression(
                        "reads otherwise than the release head, and not less",
                    ));
                    continue;
                }
            }
        };
        if matches!(case.source, Source::Recorded | Source::Fixture) {
            audit.changed.push((case.name.clone(), how));
        }
        if own_idle && counts(&now) {
            if case.stays {
                audit.stays += 1;
            } else {
                audit
                    .unfixed
                    .push(format!("{}: {now:?}\n{}", case.name, shown()));
            }
        }
    }
    audit
}

/// [`audit`], asserted: nothing read as a dialog that the release head read as none, every dialog read as
/// it was, nothing but idle prompts and conversations read less, and no idle screen of the window's own
/// harness read as a dialog the engine counts. Returns the audit for a test to say its counts.
pub fn assert_no_regression(
    window: &dyn Window,
    harness: Harness,
    reads_none_by_design: bool,
) -> Audit {
    let audit = audit(window, harness, reads_none_by_design);
    assert!(
        audit.regressions.is_empty(),
        "{} screen(s) read otherwise than the release head's window allows:\n{}",
        audit.regressions.len(),
        audit.regressions.join("\n---\n")
    );
    assert!(
        audit.unfixed.is_empty(),
        "{} idle screen(s) of {} still read as a dialog the engine counts:\n{}",
        audit.unfixed.len(),
        name_of(harness),
        audit.unfixed.join("\n---\n")
    );
    audit
}

/// Says what an audit found, for a test to print.
pub fn say(audit: &Audit, harness: Harness) -> String {
    format!(
        "{} window over the corpus: {} screens ({} idle, {} conversation, {} dialog): {} read as the release head's window read them; {} idle prompts no longer read as a dialog the engine counts, {} idle read less, {} conversations read less; {} dialogs not read by design; {} idle false dialogs stay as they were",
        name_of(harness),
        audit.screens,
        audit.idle,
        audit.conversation,
        audit.dialog,
        audit.same,
        audit.idle_no_longer_a_dialog,
        audit.idle_read_less,
        audit.conversation_read_less,
        audit.dialogs_not_read_by_design,
        audit.stays,
    )
}

#[cfg(test)]
mod guard;

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus is what its documents say it is: its recorded screens are all of the probes' that
    /// have a cursor on a line of words, each labelled, and a screen is a tail.
    #[test]
    fn the_corpus_holds_what_it_says_and_each_screen_is_a_tail() {
        let corpus = corpus();
        let recorded: Vec<&Case> = corpus
            .iter()
            .filter(|case| case.source == Source::Recorded)
            .collect();
        for (state, source, n) in labelled_counts() {
            println!("corpus: {n} {state:?} from {source:?}");
        }
        assert_eq!(recorded.len(), 62);
        // A tail is the last twelve lines that say something, wherever they are among the empty rows
        // and the rules: a recorded screen's rows start at the twelfth from the last, so each has twelve
        // unless the screen said less.
        for ((title, section), case) in sections(RECORDED).iter().zip(&recorded) {
            let said = section
                .iter()
                .filter(|row| row.chars().any(char::is_alphanumeric))
                .count();
            assert_eq!(
                case.lines.len(),
                said.min(TAIL_LINES),
                "{title}: not the pane host's tail of its rows"
            );
        }
        assert_eq!(
            recorded
                .iter()
                .filter(|case| case.state == State::Dialog)
                .count(),
            14
        );
        for harness in [Harness::Codex, Harness::Claude, Harness::Devin, Harness::Pi] {
            assert!(recorded.iter().any(|case| case.harness == harness));
        }
        assert!(corpus.len() > 800, "{}", corpus.len());
        for case in &corpus {
            assert!(
                case.lines.len() <= TAIL_LINES,
                "{}: {} lines",
                case.name,
                case.lines.len()
            );
            assert!(
                case.lines
                    .iter()
                    .all(|line| line.chars().any(char::is_alphanumeric) && line == line.trim_end()),
                "{}: a line the pane host's tail has none like:\n{}",
                case.name,
                case.lines.join("\n")
            );
        }
    }
}
