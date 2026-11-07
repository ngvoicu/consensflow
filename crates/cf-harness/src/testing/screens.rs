//! The screens the harnesses draw at rest and in their dialogs, as the engine reads them
//! (T-1240).
//!
//! The engine reads a window's screen as the pane host's **tail** (`pane.snapshot` with `tail:
//! 12`, `cf_panes::Screen::tail`): the last twelve lines that hold a letter or a digit, each
//! without the blanks that end it. No empty line is in it, and no rule that says no word. A
//! harness that draws its prompt as a box between blank lines or rules (Codex's composer and
//! its footer, Devin's and Muse's box under a rule that says something) has them one under the
//! other in it, in the column the harness draws them at. Every screen here is written that way,
//! from a harness's own recorded screen where there is one, and its source is named where it
//! is.
//!
//! The screens at rest are the windows' last lines after a message was delivered that ends in
//! a question and a turn answered it: a worker's question to a chief, as ConsensFlow words it
//! (`[ConsensFlow m-2 · T-1 · question from @worker]`, the question, how to answer it). None of
//! them shows a dialog. The dialogs are the ones `brain: research/harness-dialogs-inventory`
//! lists, drawn as the harness's own recorded screens draw their lists; two of them (Devin's
//! offer to switch its theme, Muse's card of its quota) were read from their strings only and
//! are drawn here the way the same harness draws a list of its own, which is a guess.

use crate::contract::{Dialog, Window};

/// A screen's lines.
pub fn lines(screen: &[&str]) -> Vec<String> {
    screen.iter().map(|line| (*line).to_owned()).collect()
}

/// `window` reads `screen` (named `what` for a failure) as no dialog the engine would tell of
/// and hold the window for: none at all, or one its prompt is drawn below, or one that asks
/// nothing (`cf_engine::dialogs`: a dialog counts where it asks and no prompt is below it).
pub fn reads_no_dialog(window: &dyn Window, what: &str, screen: &[&str]) {
    let shown = window.dialog(&lines(screen));
    assert!(
        shown
            .as_ref()
            .is_none_or(|dialog| !dialog.asks || dialog.prompt_below),
        "{what}: the window reads a dialog off its idle screen: {shown:#?}\n{}",
        screen.join("\n")
    );
}

/// `window` reads nothing of a dialog off `screen` (named `what`): its prompt is no list of
/// choices, whatever the conversation above it asks.
pub fn reads_nothing(window: &dyn Window, what: &str, screen: &[&str]) {
    let shown = window.dialog(&lines(screen));
    assert!(
        shown.is_none(),
        "{what}: the window reads a list of choices off its idle screen: {shown:#?}\n{}",
        screen.join("\n")
    );
}

/// `window` reads `screen` (named `what`) as a dialog that waits for an answer, with no prompt
/// below its choices: the dialog the engine tells of and holds the window for. The dialog.
pub fn reads_a_dialog(window: &dyn Window, what: &str, screen: &[&str]) -> Dialog {
    let Some(dialog) = window.dialog(&lines(screen)) else {
        panic!(
            "{what}: the window reads no dialog off its screen\n{}",
            screen.join("\n")
        );
    };
    assert!(
        dialog.asks && !dialog.prompt_below,
        "{what}: it is read, but it would not hold the window: {dialog:#?}"
    );
    dialog
}

/// The question, as a worker's `cf ask` word it.
pub const QUESTION: &str = "Which format should I use for my reply: JSON or text?";

// ─── Codex ─────────────────────────────────────────────────────────────────────────────────────

/// A Codex 0.166 chief at rest after it answered a worker's question: what the live matrix's
/// `question-ask-answer` with a Codex chief saw on 2026-11-05 (T-1178, the Mac release check's
/// bundle of that scenario). The bundle's `windows/p1-chief.txt`
/// is the window's byte stream with its control sequences taken out, not its screen: the
/// composer and the footer, drawn by cursor addressing, are not in it, and the words of each
/// line are one to a line. The screen is that stream's words in the lines Codex drew them in,
/// and the layout of Codex's own recorded screen (the probes of T-model-switch, Codex 0.164:
/// the screen after its first turn, Codex 0.164): the message with its `›`, the model's lines with their `•`,
/// `  Worked for 5s • 16:00` at the column of the composer's words, the composer, its footer
/// (the model, its effort, the folder), and the line of hints under it. The note ConsensFlow
/// wrote of it quoted the three lines this reads as the dialog's "choices".
pub const CODEX_IDLE_AFTER_QUESTION: &[&str] = &[
    "› [ConsensFlow m-2 · T-1 · question from @swe-2-medium-gentle-lagoon]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /var/folders/xx/yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/T/consensflow-integration-abcdef/bundle/bin/cf answer m-2 \"…\"",
    "• The worker asked which format to use; I’m answering JSON now.",
    "• Ran /var/folders/xx/yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/T/consensflow-integration-…",
    "  └ m-3 answered @swe-2-medium-gentle-lagoon.",
    "    + Show details",
    "• Answered the worker with JSON; I’ll tell you when its result is back.",
    "  Worked for 5s • 16:00",
    "› Ask Codex to do anything",
    "  GPT-5.6-Luna default · ~/.consensflow-candidate/live/matrix-20260101T000000Z-seats-question-ask-answer-12345-0",
    "  ? for shortcuts",
];

/// The same Codex chief mid-turn: its status above the composer, as Codex draws one
/// (`• Working (4s • esc to interrupt)`), the composer and the footer the same.
pub const CODEX_WORKING_AFTER_QUESTION: &[&str] = &[
    "› [ConsensFlow m-2 · T-1 · question from @swe-2-medium-gentle-lagoon]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /var/folders/xx/yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/T/consensflow-integration-abcdef/bundle/bin/cf answer m-2 \"…\"",
    "• The worker asked which format to use; I’m answering JSON now.",
    "• Ran /var/folders/xx/yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/T/consensflow-integration-…",
    "  └ m-3 answered @swe-2-medium-gentle-lagoon.",
    "• Working (4s • esc to interrupt)",
    "› Ask Codex to do anything",
    "  GPT-5.6-Luna default · ~/.consensflow-candidate/live/matrix-20260101T000000Z-seats-question-ask-answer-12345-0",
    "  ? for shortcuts",
];

/// A Codex composer that holds an unsent message of two lines (the human typed or pasted it
/// and has not sent it), under a model's question: the composer's first line after its `›`, the
/// next in the column of its words, then the footer (`brain: research/harness-dialogs-inventory`,
/// "Limits of the detection": it "hasn't been seen").
pub const CODEX_COMPOSER_HOLDING_LINES: &[&str] = &[
    "• Done. Which format should I use for my reply: JSON or text?",
    "  Worked for 5s • 16:00",
    "› Use JSON for the reply",
    "  and keep it short",
    "  GPT-5.6-Luna default · ~/.consensflow-candidate/live/work",
    "  ? for shortcuts",
];

/// A Codex composer that holds a message of two numbered lines, unsent, under a model's question:
/// numbered as a dialog's choices are, in the column of its words, with the status under it.
pub const CODEX_COMPOSER_HOLDING_NUMBERED_LINES: &[&str] = &[
    "• Which approach do you want for the parser: the table or the state machine?",
    "  Worked for 5s • 16:00",
    "› 1. Use the table",
    "  2. Test it first",
    "  GPT-5.6-Luna default · ~/.consensflow-candidate/live/work",
    "  ? for shortcuts",
];

/// A Codex draft of two numbered lines over a footer the user configured (`tui.status_line =
/// ["git-branch"]` on a branch called `enter`; the second review of T-1240, which cites Codex 0.166's
/// `status_surfaces.rs`, that renders the branch's name as it is, and `chat_composer.rs`, that hides the
/// line of keys while a draft is in the composer): the footer is the branch's name, a word that is a key's
/// name, and nothing else is under the draft. `5b7acff7f` reads none.
pub const CODEX_DRAFT_OVER_BRANCH_ENTER: &[&str] = &[
    "• Which approach should we take?",
    "  Worked for 5s • 16:00",
    "› 1. Use the table",
    "  2. Test it first",
    "  enter",
];

/// The same over a branch called `esc`.
pub const CODEX_DRAFT_OVER_BRANCH_ESC: &[&str] = &[
    "• Which approach should we take?",
    "  Worked for 5s • 16:00",
    "› 1. Use the table",
    "  2. Test it first",
    "  esc",
];

/// The same with the model before the branch (`tui.status_line = ["model-with-reasoning",
/// "git-branch"]`).
pub const CODEX_DRAFT_OVER_MODEL_AND_BRANCH_ENTER: &[&str] = &[
    "• Which approach should we take?",
    "  Worked for 5s • 16:00",
    "› 1. Use the table",
    "  2. Test it first",
    "  GPT-5.6-Luna default · enter",
];

/// The drafts of two lines that are no list, over the same footers: the shape of a list is not what keeps
/// the release head from reading these, for it reads them as a list of choices (`5b7acff7f` read them as
/// a dialog when the footer was the model and a folder: the field's bug, in a user's words).
pub const CODEX_PLAIN_DRAFT_OVER_BRANCH_ENTER: &[&str] = &[
    "• Which approach should we take?",
    "  Worked for 5s • 16:00",
    "› Use the table",
    "  and test it first",
    "  enter",
];

/// The same over a branch called `esc`.
pub const CODEX_PLAIN_DRAFT_OVER_BRANCH_ESC: &[&str] = &[
    "• Which approach should we take?",
    "  Worked for 5s • 16:00",
    "› Use the table",
    "  and test it first",
    "  esc",
];

/// **Data for the follow-up that reads Codex's dialogs** (`issues/codex-dialogs-are-not-read-from-its-screen.md`):
/// the window reads none of them in alpha.2, as the release head read none off the tail.
///
/// Codex 0.166's nudge to switch models, drawn as its own lists are (`› 1. …`, the others
/// numbered, the hint of its keys under them): the shape is that of the recorded pickers
/// (the probes of T-model-switch, the picker of an effort), its words are the binary's
/// (`brain: research/harness-dialogs-inventory`: [bin], never seen live).
pub const CODEX_NUDGE: &[&str] = &[
    "• Worked for 1s • 12:56",
    "  Approaching rate limits",
    "  Switch to gpt-5.6-mini for lower credit usage?",
    "› 1. Switch to gpt-5.6-mini",
    "  2. Keep current model",
    "  3. Keep current model (never show again)",
    "  Press enter to confirm or esc to go back",
];

/// Codex 0.164's picker of a model, as it recorded it (the probes of T-model-switch): no composer is drawn while it is open.
pub const CODEX_PICKER: &[&str] = &[
    "  Worked for 2s • 12:56",
    "  Select Model and Effort",
    "  1. GPT-6.1-Sol             Latest workhorse model for coding and everyday work.",
    "  2. GPT-6-Astra             Frontier intelligence for the most demanding work.",
    "  3. GPT-6-Sol               Previous generation workhorse model.",
    "  4. GPT-6-Luna              Fast and affordable model for easier tasks.",
    "› 5. GPT-5.6-Luna (current)  Older fast and efficient model.",
    "  6. GPT-5.6-Terra           Older balanced model for straightforward work.",
    "  enter select · esc back",
];

// ─── Devin ─────────────────────────────────────────────────────────────────────────────────────

/// A Devin 3000.11.234 chief at rest after it answered a worker's question: the layout of its
/// recorded tail (`live_paste_screen.rs`, a run of 2026-10-31): the message at two columns in,
/// the model's lines at one, the rule over the box with the conversation's title in it (the
/// tail keeps a rule that says something), the box with its hint, and the model's line under
/// it.
pub const DEVIN_IDLE_AFTER_QUESTION: &[&str] = &[
    "  [ConsensFlow m-2 · T-1 · question from @worker]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /tmp/consensflow/bundle/bin/cf answer m-2 \"…\"",
    " ⏺ cf answer m-2 \"JSON\"",
    "   m-3 answered @worker.",
    " Answered the worker with JSON; I will tell you when its result is back.",
    "───────────────────────────── Answer the worker’s question about the format… ─",
    "❭ Ask Devin to build features, fix bugs, or work on your code",
    "SWE-2 Medium · bypass                            Context: 55k / 555k tokens (5%)",
    "Update v3000.11.234 available!",
];

/// The same Devin chief before it answered: the message it was delivered is the last the
/// conversation shows, at two columns in, right over the rule of its box (it is thinking, or at
/// rest on a message it read as a note).
pub const DEVIN_ON_THE_QUESTION: &[&str] = &[
    "  [ConsensFlow m-2 · T-1 · question from @worker]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /tmp/consensflow/bundle/bin/cf answer m-2 \"…\"",
    "───────────────────────────── Answer the worker’s question about the format… ─",
    "❭ Ask Devin to build features, fix bugs, or work on your code",
    "SWE-2 Medium · bypass                            Context: 55k / 555k tokens (5%)",
    "Update v3000.11.234 available!",
];

/// A Devin box that holds an unsent message of two lines: the first after the `❭`, the second
/// in the column of its words (`brain`, `devin/input_box.rs`: "the box's lines, the first after
/// its `❭`").
pub const DEVIN_BOX_HOLDING_LINES: &[&str] = &[
    " Answered the worker with JSON.",
    "  Which format should I use for my reply: JSON or text?",
    "───────────────────────────── Answer the worker’s question about the format… ─",
    "❭ Use JSON for the reply",
    "  and keep it short",
    "SWE-2 Medium · bypass                            Context: 55k / 555k tokens (5%)",
    "Update v3000.11.234 available!",
];

/// Devin's dialog of a call in its normal permission mode, as `cargo xtask live devin-resume`
/// recorded its rows (2026-10-15): the call, `❭` on "1 Yes", the list numbered without a dot, the
/// hint of its keys. No line above it asks.
pub const DEVIN_CALL: &[&str] = &[
    "└ $ echo ran > shell.txt",
    "❭ 1 Yes  (Approve once)",
    "2 Yes, switch to accept edits mode",
    "3 Yes, switch to bypass mode",
    "4 Edit command",
    "5 Describe change to command",
    "6 No",
    "↑↓ select · ↵ confirm · esc cancel",
];

/// Devin's offer to switch its theme (`brain: research/harness-dialogs-inventory`: "Detected
/// {x} theme but configured for {y}. Switch?", three choices): read from the binary's strings,
/// never drawn on a screen that was recorded. Drawn as its list of a call is.
pub const DEVIN_THEME: &[&str] = &[
    "Detected dark theme but configured for light. Switch?",
    "❭ 1 Yes, switch theme",
    "2 Yes, always auto-detect",
    "3 No, keep current",
    "↑↓ select · ↵ confirm · esc cancel",
];

// ─── Muse Code ─────────────────────────────────────────────────────────────────────────────────

/// A Muse Code 1.4.14 chief at rest after it answered a worker's question, its empty box
/// showing one of Muse's tips, as the box under the answer of a turn drew it (T-1081's live run,
/// `muse/screen.rs`: "TIPPED"). The tail keeps the rule that names the voice key and loses the
/// rules that say nothing, the box's rule under it among them.
pub const MUSE_IDLE_AFTER_QUESTION: &[&str] = &[
    "❯ [ConsensFlow m-2 · T-1 · question from @worker]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /tmp/consensflow/bundle/bin/cf answer m-2 \"…\"",
    "◆ Ran cf answer m-2 \"JSON\" · ctrl+o",
    "◆ Answered the worker with JSON; I will tell you when its result is back.",
    "◆ Finished · cf answer · ctrl+o",
    "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
    "❯ Ask to monitor a running build or log and get pinged on changes",
    "  muse-spark-1.3 · minimal · /p/v/f/5/c/T/c/workspace/p2-devin · YOLO",
];

/// A Muse box that holds an unsent message of two lines: the first after the `❯`, the second in
/// the column of its words.
pub const MUSE_BOX_HOLDING_LINES: &[&str] = &[
    "◆ Answered the worker with JSON.",
    "  Which format should I use for my reply: JSON or text?",
    "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
    "❯ Use JSON for the reply",
    "  and keep it short",
    "  muse-spark-1.3 · minimal · /p/v/f/5/c/T/c/workspace/p2-devin · YOLO",
];

/// A Muse Code 1.4.14 chief at rest after a question of its own, the user's reply under it echoed with
/// numbered lines and a line of words in their column (T-1243: the same regression as Pi's). The tail
/// keeps the rule that names the voice key and the status line under it, in the column of the echo's
/// words; Muse's prompt check needs the box rows the tail drops, so nothing below the list says it is a
/// prompt.
pub const MUSE_NUMBERED_ECHO_AT_REST: &[&str] = &[
    "◆ Which approach should we take?",
    "❯ 1. Use the table",
    "  2. Test it first",
    "  please decide?",
    "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
    "  muse-spark-1.3 · minimal · /p/w/app · YOLO",
];

/// Muse's card of its quota (`brain: research/harness-dialogs-inventory`: "You're using your
/// quota fast", three choices): read from the binary's strings, never seen on a screen that
/// was recorded. Drawn as a card in a frame, as Claude Code draws its offers, over the box
/// whose rule is under it, its `❯` on the first choice.
pub const MUSE_QUOTA: &[&str] = &[
    "◆ Finished · Run requested script · ctrl+o",
    "╭──────────────────────────────────────────────────────╮",
    "│ You're using your quota fast                         │",
    "│ Would you like to keep going?                        │",
    "│ ❯ Keep going                                         │",
    "│   Switch to Spark                                    │",
    "│   Turn on contributor mode                           │",
    "╰──────────────────────────────────────────────────────╯",
    "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
    "  muse-spark-1.3 · minimal · /p/v/f/5/c/T/c/workspace/p2-devin · YOLO",
];

/// The same card with no frame, its question a column out from its choices.
pub const MUSE_QUOTA_UNFRAMED: &[&str] = &[
    "◆ Finished · Run requested script · ctrl+o",
    "You're using your quota fast. Would you like to keep going?",
    "  ❯ Keep going",
    "    Switch to Spark",
    "    Turn on contributor mode",
    "── Voice input (⌥ + v to start) ────────────────────────────────────────────────",
    "  muse-spark-1.3 · minimal · /p/v/f/5/c/T/c/workspace/p2-devin · YOLO",
];

// ─── Pi ────────────────────────────────────────────────────────────────────────────────────────

/// A Pi 1.5 chief at rest after it answered a worker's question: its conversation drawn a column
/// in with no mark, its editor between two rules (the tail has neither, and its empty box has no
/// line), the folder and the line of its stats, as its recorded screens draw them
/// (the probes of T-model-switch, after a turn).
pub const PI_IDLE_AFTER_QUESTION: &[&str] = &[
    " [ConsensFlow m-2 · T-1 · question from @worker]",
    " Which format should I use for my reply: JSON or text?",
    " Run in your shell: /tmp/consensflow/bundle/bin/cf answer m-2 \"…\"",
    " Answered the worker with JSON; I will tell you when its result is back.",
    "/work/app",
    "↑5.5k ↓555 R55k CH55.5% $5.555 5.5%/5.5M (auto)                       meta/muse-spark-1.3 • low",
];

/// A Pi 1.5.0 chief at rest after a message that asks a question and shows a fenced example whose
/// lines are numbered and quoted (T-1243, the review of `9a7fca6cb`): Pi's own `AssistantMessageComponent`
/// drew the message (`corpus/pi_rendered.txt`, "fenced-quote"), and under it the editor, which the tail
/// has no line of, the folder and the line of its stats. `5b7acff7f` reads none; a reader that ended the
/// list at the line that is not numbered read a dialog, `asks` and no prompt below.
pub const PI_QUOTED_EXAMPLE_AT_REST: &[&str] = &[
    " Which approach should we take?",
    "   > 1. Use the table",
    "     2. Test it first",
    "     This is sample quoted text",
    "/work/app",
    "↑5.5k ↓555 R55k CH55.5% $5.555 5.5%/5.5M (auto)                       meta/muse-spark-1.3 • low",
];

// ─── Claude Code ───────────────────────────────────────────────────────────────────────────────

/// A Claude Code 2.1.317 chief at rest after it answered a worker's question: the message at its
/// `❯`, the lines of the answer, and the line under the box that says it is in full-permission
/// mode (the tail has neither of the box's rules nor its empty `❯`).
pub const CLAUDE_IDLE_AFTER_QUESTION: &[&str] = &[
    "❯ [ConsensFlow m-2 · T-1 · question from @worker]",
    "  Which format should I use for my reply: JSON or text?",
    "  Run in your shell: /tmp/consensflow/bundle/bin/cf answer m-2 \"…\"",
    "⏺ Bash(cf answer m-2 \"JSON\")",
    "  ⎿  m-3 answered @worker.",
    "⏺ Answered the worker with JSON; I will tell you when its result is back.",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
];

/// The same chief when a human's message, typed in its box and not yet sent, is two lines.
pub const CLAUDE_BOX_HOLDING_LINES: &[&str] = &[
    "⏺ Done. Which format should I use for my reply: JSON or text?",
    "❯ Use JSON for the reply",
    "  and keep it short",
    "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
];

/// Claude Code 2.1.317's offer to install an LSP plugin, as a live member's window drew it
/// mid-turn, over its spinner (`cargo xtask live harness-dialogs`, 2026-11-03).
pub const CLAUDE_LSP: &[&str] = &[
    " LSP plugin recommendation",
    "   Plugin: clangd-lsp",
    "   Marketplace: claude-plugins-official",
    "   Triggered by: .c files",
    "   Would you like to install this LSP plugin?",
    "   ❯ 1. Yes, install",
    "     2. No, not now",
    "     3. Never for this plugin",
    "     4. Disable all LSP recommendations",
    "✽ Honking… (2s · ↓ 55 tokens)",
];

/// Claude Code 2.1.319's offer to install an LSP plugin as the pane host's tail (the engine's: the
/// last twelve lines with a letter or a digit, no empty line) showed it in the kit's live runs of
/// 2026-11-06 (T-1261), the turn at work: the offer's eleven lines and the spinner under them fill
/// the twelve. The spinner's figures are placeholders.
pub const CLAUDE_LSP_AT_WORK: &[&str] = &[
    " LSP plugin recommendation",
    "   LSP provides code intelligence like go-to-definition and error checking",
    "   Plugin: clangd-lsp",
    "   Marketplace: claude-plugins-official",
    "   Triggered by: .c files",
    "   Description: C/C++ language server (clangd) for code intelligence",
    "   Would you like to install this LSP plugin?",
    "   \u{276f} 1. Yes, install",
    "     2. No, not now",
    "     3. Never for this plugin",
    "     4. Disable all LSP recommendations",
    "\u{b7} Mustering\u{2026} (2s \u{b7} \u{2193} 55 tokens)",
];

/// The same offer once the turn under it ended (the status said `waiting`, "dialog open"): the
/// spinner is gone, so the tail slid down a line, and the line that closes a turn is the first line
/// above the offer's title (the time and the seconds are placeholders).
pub const CLAUDE_LSP_TURN_ENDED: &[&str] = &[
    "\u{273b} Baked for 9s \u{b7} done 8:21 AM \u{b7} 1 shell still running",
    " LSP plugin recommendation",
    "   LSP provides code intelligence like go-to-definition and error checking",
    "   Plugin: clangd-lsp",
    "   Marketplace: claude-plugins-official",
    "   Triggered by: .c files",
    "   Description: C/C++ language server (clangd) for code intelligence",
    "   Would you like to install this LSP plugin?",
    "   \u{276f} 1. Yes, install",
    "     2. No, not now",
    "     3. Never for this plugin",
    "     4. Disable all LSP recommendations",
];

/// Claude Code 2.1.319's permission prompt of a dangerous `rm` (of two empty variables, a command
/// that runs nothing: `rm "$CFA/$CFB"`), as the pane host's tail (the engine's) showed it in the
/// kit's live run of 2026-11-07 (T-1277), at the first look: 1:59 left before Claude Code blocks the
/// request by itself. The 120 looks of the run, one a second, differed in that clock alone. The
/// screen wrapped the warning after its time, so the line is two rows.
pub const CLAUDE_RM_PROMPT_LIVE: &[&str] = &[
    " \u{23bf} \u{a0}Waiting\u{2026}",
    " Bash command",
    " Remove the specified file",
    " rm \"$CFA/$CFB\"",
    " \u{2502} Dangerous rm operation on possibly-empty variable path: \"$CFA/$CFB\" in `rm",
    " \u{2502} \"$CFA/$CFB\"` (rewrite it as \"${CFA:?}/${CFB:?}\" or use a literal path)",
    " \u{26a0} Claude Code will automatically block this request in 1:59, to avoid holding",
    " up an unattended session",
    " Do you want to proceed?",
    " \u{276f} 1. Yes",
    "   2. No",
    " Esc to cancel \u{b7} Tab to amend",
];

/// Claude Code 2.1.317's ask of a plugin's PreToolUse hook on a Bash call (the owner's kluris
/// board, 2026-11-03, T-1198).
pub const CLAUDE_HOOK_ASK: &[&str] = &[
    "│ git status -s",
    "│ Hook tool.check requires confirmation for this",
    "│ command:",
    "│ This call's input holds your account email",
    "│ address, which may leave this machine.",
    "│ [plugin:cc-plugin-privacy]",
    "Do you want to proceed?",
    "❯ 1. Yes",
    "  2. No",
    "Esc to cancel · Tab to amend",
];
