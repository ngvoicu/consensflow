//! What tells Claude Code's dialogs apart (T-1277): the time in its own warning line blanked,
//! every other time kept, and the LSP offer from its title down.

use super::*;
use crate::shared::dialog;
use crate::testing::screens::{CLAUDE_HOOK_ASK, CLAUDE_LSP};

/// The dialog `lines` show, as the engine's reader reads it.
fn read(lines: &[String]) -> Dialog {
    dialog::read(lines).expect("a dialog").dialog
}

fn owned(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

const GLYPH: &str = "\u{26a0}";

/// The warning line as Claude Code 2.1.319 draws it, `left` the time in it.
fn blocks(left: &str) -> String {
    format!(" {GLYPH} Claude Code will automatically block this request in {left}, to avoid holding up an unattended session")
}

/// Claude Code 2.1.319's permission prompt of a dangerous `rm`, as the pane host's tail showed it
/// on the owner's board on 2026-11-06 (T-1268), the paths and the time placeholders: `warning`
/// is the line (or lines) under the command that counts down.
fn rm_prompt(command: &str, warning: &[String]) -> Vec<String> {
    let mut lines = owned(&[
        " Bash command",
        &format!("   {command}"),
        "   Dangerous rm operation on possibly-empty variable path: \"$S/$l\" in `rm \"$S/$l\"`",
    ]);
    lines.extend(warning.iter().cloned());
    lines.extend(owned(&[
        " Do you want to proceed?",
        " \u{276f} 1. Yes",
        "   2. No",
    ]));
    lines
}

const COMMAND: &str = "$T/tree/app/tests/shots/make.mjs $T/scratch/ && ls $T/scratch";

fn id(command: &str, warning: &[String]) -> Dialog {
    identity(&read(&rm_prompt(command, warning)))
}

#[test]
fn the_time_in_the_warning_is_the_same_dialog_at_every_clock() {
    let first = id(COMMAND, &[blocks("1:57")]);
    for left in [
        "0:03",
        "9:59",
        "10:00",
        "1:02",
        "about 2 minutes",
        "about 57 seconds",
        "about 1 minute",
        "about 1 second",
    ] {
        assert_eq!(id(COMMAND, &[blocks(left)]), first, "{left}");
    }
    // What is left of the line is its words: the time is the one thing taken out.
    assert_eq!(
        first.detail.last().map(String::as_str),
        Some("\u{26a0} Claude Code will automatically block this request in #, to avoid holding up an unattended session")
    );
    // And the read it was made from is as it was read.
    let raw = read(&rm_prompt(COMMAND, &[blocks("1:57")]));
    assert!(raw
        .detail
        .last()
        .is_some_and(|line| line.contains("in 1:57,")));
}

#[test]
fn nothing_but_the_detail_of_the_dialog_is_touched() {
    let raw = read(&rm_prompt(COMMAND, &[blocks("1:57")]));
    let made = identity(&raw);
    assert_eq!(made.question, raw.question);
    assert_eq!(made.options, raw.options);
    assert_eq!(made.asks, raw.asks);
    assert_eq!(made.prompt_below, raw.prompt_below);
    assert_eq!(made.closes_after_s, raw.closes_after_s);
    assert_eq!(made.detail.len(), raw.detail.len());
}

#[test]
fn a_warning_wrapped_before_its_time_is_the_same_line_at_every_clock() {
    // A narrow screen breaks the line after "in": its second row begins with the time.
    let wrapped = |left: &str| {
        vec![
            format!(" {GLYPH} Claude Code will automatically block this request in"),
            format!(" {left}, to avoid holding up an unattended session"),
        ]
    };
    let first = id(COMMAND, &wrapped("1:57"));
    for left in ["0:03", "9:59", "about 3 seconds", "about 2 minutes"] {
        assert_eq!(id(COMMAND, &wrapped(left)), first, "{left}");
    }
    assert_eq!(
        first.detail[first.detail.len() - 1],
        "#, to avoid holding up an unattended session"
    );
    assert_ne!(
        first,
        read(&rm_prompt(COMMAND, &wrapped("1:57"))),
        "the clock is what is taken out"
    );
}

#[test]
fn a_warning_wrapped_after_its_time_is_the_same_line_at_every_clock() {
    // The line breaks after the time: the first row holds both, the second only words.
    let wrapped = |left: &str| {
        vec![
            format!(
                " {GLYPH} Claude Code will automatically block this request in {left}, to avoid"
            ),
            " holding up an unattended session".to_owned(),
        ]
    };
    assert_eq!(id(COMMAND, &wrapped("1:57")), id(COMMAND, &wrapped("0:03")));
}

#[test]
fn a_time_in_the_request_is_part_of_it_whatever_words_come_before_it() {
    // The command, the hook's reason and the question are the request: a time there is a word of
    // it, "in 5s" and "in 10s" are two requests (T-1273's probes).
    let five = id("printf 'retry in 5s'", &[blocks("1:57")]);
    let ten = id("printf 'retry in 10s'", &[blocks("1:57")]);
    assert_ne!(five, ten);
    assert_eq!(five.detail[1], "printf 'retry in 5s'");
    // Not even a time written as the countdown writes it.
    assert_ne!(
        id("printf 'retry in 1:30'", &[blocks("1:57")]),
        id("printf 'retry in 2:30'", &[blocks("1:57")])
    );
    // A command that says the warning's own words is no warning: it does not begin the line.
    let said = id(
        "echo \"Claude Code will automatically block this request in 1:57, to avoid it\"",
        &[blocks("1:57")],
    );
    assert!(said.detail[1].contains("in 1:57"), "{said:?}");
    let glyph = id(
        &format!("echo \"{GLYPH} Claude Code will automatically block this request in 1:57\""),
        &[blocks("1:57")],
    );
    assert!(glyph.detail[1].contains("in 1:57"), "{glyph:?}");
    // The words of the warning with no glyph before them are not Claude Code's line either.
    let bare = |left: &str| {
        id(
            COMMAND,
            &[format!(
                " Claude Code will automatically block this request in {left}, to avoid holding up an unattended session"
            )],
        )
    };
    assert_ne!(bare("1:57"), bare("0:03"));
    // A question that holds a time, with no warning beside it.
    let asks = |question: &str| {
        identity(&read(&owned(&[
            &format!(" {question}"),
            " \u{276f} 1. Yes",
            "   2. No",
        ])))
    };
    assert_ne!(asks("Retry in 5s?"), asks("Retry in 10s?"));
    assert_eq!(asks("Retry in 5s?").question, "Retry in 5s?");
}

#[test]
fn a_hooks_reason_that_begins_with_a_time_is_the_hooks_own_words() {
    // T-1273's finding 7: a row that begins with a time and goes on ", to avoid …" is blanked as
    // the second half of the warning only where the warning is the row above it.
    let hook = |reason: &str| {
        identity(&read(&owned(&[
            "\u{2502} sleep 30",
            "\u{2502} Hook tool.check requires confirmation for this",
            "\u{2502} command:",
            &format!("\u{2502} {reason}"),
            "\u{2502} [plugin:review-deployment]",
            "Do you want to proceed?",
            "\u{276f} 1. Yes",
            "  2. No",
        ])))
    };
    assert_ne!(
        hook("about 2 minutes, to avoid interrupting the deployment"),
        hook("about 3 minutes, to avoid interrupting the deployment")
    );
    // Without the box glyph of the hook's rows, the same.
    let bare = |reason: &str| {
        identity(&read(&owned(&[
            " Hook tool.check requires confirmation for this command:",
            &format!(" {reason}"),
            " [plugin:review-deployment]",
            " Do you want to proceed?",
            " \u{276f} 1. Yes",
            "   2. No",
        ])))
    };
    for (a, b) in [
        (
            "about 2 minutes, to avoid interrupting the deployment",
            "about 3 minutes, to avoid interrupting the deployment",
        ),
        ("1:57, to avoid a restart", "0:03, to avoid a restart"),
    ] {
        assert_ne!(bare(a), bare(b), "{a}");
    }
    // After a warning that carries its time the row under it is words of the dialog: the
    // continuation of a line is a line that was cut before its time.
    let under = |left: &str| {
        id(
            COMMAND,
            &[
                blocks("1:57"),
                format!(" {left}, to avoid interrupting the deployment"),
            ],
        )
    };
    assert_ne!(under("about 2 minutes"), under("about 3 minutes"));
    // And under a row that merely ends the way the warning does.
    let ends = |left: &str| {
        id(
            COMMAND,
            &[
                " Please wait, the build will automatically block this request in".to_owned(),
                format!(" {left}, to avoid a clash"),
            ],
        )
    };
    assert_ne!(ends("1:57"), ends("0:03"));
}

#[test]
fn a_time_that_is_not_all_of_one_the_warning_draws_is_left_as_it_was() {
    for left in [
        "1:5",
        "1:575",
        "x:57",
        "about 2",
        "about minutes",
        "soon",
        "",
    ] {
        let raw = read(&rm_prompt(COMMAND, &[blocks(left)]));
        assert_eq!(identity(&raw), raw, "{left:?}");
    }
}

#[test]
fn the_offer_to_install_an_lsp_plugin_is_its_lines_from_its_title_down() {
    // The release check's tails: the offer at work, and once its turn ended with the line that
    // closes a turn above it (T-1261).
    let at_work = read(&owned(CLAUDE_LSP));
    let mut ended = owned(CLAUDE_LSP);
    ended.insert(
        0,
        "\u{273b} Baked for 9s \u{b7} done 8:21 AM \u{b7} 1 shell still running".to_owned(),
    );
    let ended = read(&ended);
    assert_ne!(
        ended.detail, at_work.detail,
        "the reader sees the line above"
    );
    assert_eq!(identity(&ended), identity(&at_work));
    assert_eq!(
        identity(&ended).detail.first().map(String::as_str),
        Some(LSP_TITLE)
    );
    // Another plugin's offer is another dialog.
    let other = owned(CLAUDE_LSP)
        .into_iter()
        .map(|line| line.replace("clangd-lsp", "pyright-lsp"))
        .collect::<Vec<_>>();
    assert_ne!(identity(&read(&other)), identity(&at_work));
}

#[test]
fn a_title_the_conversation_said_above_the_offers_own_is_not_its_top() {
    let mut lines = owned(&["\u{23fa} The title is \"LSP plugin recommendation\""]);
    lines.push(" LSP plugin recommendation".to_owned());
    lines.extend(owned(CLAUDE_LSP).into_iter().skip(1));
    let said = read(&lines);
    assert_eq!(
        identity(&said),
        identity(&read(&owned(CLAUDE_LSP))),
        "{said:?}"
    );
    // The same line among the conversation's own, as a line by itself.
    let mut lines = owned(&[" LSP plugin recommendation", " (said by a worker)"]);
    lines.extend(owned(CLAUDE_LSP));
    assert_eq!(identity(&read(&lines)), identity(&read(&owned(CLAUDE_LSP))));
}

#[test]
fn a_tail_that_cut_the_offers_title_is_another_read() {
    // No title in what was read: nothing is cut. It is not the read that has the title, and the
    // engine tells it as another dialog, which its harness's list of safe declines then declines.
    let mut lines = owned(CLAUDE_LSP);
    lines.remove(0);
    let cut = read(&lines);
    assert!(!cut.detail.iter().any(|line| line == LSP_TITLE));
    assert_eq!(identity(&cut), cut);
    assert_ne!(identity(&cut), identity(&read(&owned(CLAUDE_LSP))));
}

#[test]
fn a_dialog_that_is_no_lsp_offer_is_not_cut_at_a_line_that_says_the_title() {
    let lines = owned(&[
        " conversation above",
        " LSP plugin recommendation",
        " Plugin: clangd-lsp",
        " Do you want to proceed?",
        " \u{276f} 1. Yes",
        "   2. No",
    ]);
    let dialog = read(&lines);
    assert_eq!(identity(&dialog).detail.len(), 3, "{dialog:?}");
}

#[test]
fn a_hooks_ask_without_a_warning_is_what_it_was() {
    let dialog = read(&owned(CLAUDE_HOOK_ASK));
    assert_eq!(identity(&dialog), dialog);
}

#[test]
fn the_live_permission_prompt_with_its_wrapped_warning_is_one_dialog_at_every_tick() {
    // The kit's live run of 2026-11-07: 120 looks, a second apart, 1:59 down to 0:00, each the same
    // rows but for the clock. The screen wrapped the warning after its time.
    use crate::testing::screens::CLAUDE_RM_PROMPT_LIVE;
    let at = |clock: &str| {
        let lines: Vec<String> = CLAUDE_RM_PROMPT_LIVE
            .iter()
            .map(|line| line.replace("in 1:59,", &format!("in {clock},")))
            .collect();
        identity(&read(&lines))
    };
    let first = at("1:59");
    for clock in ["1:58", "1:00", "0:59", "0:10", "0:01", "0:00"] {
        assert_eq!(at(clock), first, "{clock}");
    }
    let rows = &first.detail;
    assert!(
        rows.iter().any(|row| row
            == "\u{26a0} Claude Code will automatically block this request in #, to avoid holding"),
        "{rows:?}"
    );
    // The row that wraps under it holds words only, and is left as it was.
    assert_eq!(
        rows.last().map(String::as_str),
        Some("up an unattended session")
    );
    // The request is its own: another command is another dialog.
    let other: Vec<String> = CLAUDE_RM_PROMPT_LIVE
        .iter()
        .map(|line| line.replace("rm \"$CFA/$CFB\"", "rm \"$CFA/$CFC\""))
        .collect();
    assert_ne!(identity(&read(&other)), first);
}
