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
    // The two rows are one row of the identity, the sentence whole with its time blanked.
    assert_eq!(
        first.detail[first.detail.len() - 1],
        "\u{26a0} Claude Code will automatically block this request in #, to avoid holding up an unattended session"
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
    let lines_at = |clock: &str| -> Vec<String> {
        CLAUDE_RM_PROMPT_LIVE
            .iter()
            .map(|line| line.replace("in 1:59,", &format!("in {clock},")))
            .collect()
    };
    let at = |clock: &str| identity(&read(&lines_at(clock)));
    let first = at("1:59");
    for clock in ["1:58", "1:00", "0:59", "0:10", "0:01", "0:00"] {
        assert_eq!(at(clock), first, "{clock}");
    }
    // The two rows the screen wrapped it in are one row of the identity, whole, its time blanked.
    assert_eq!(
        first.detail.last().map(String::as_str),
        Some("\u{26a0} Claude Code will automatically block this request in #, to avoid holding up an unattended session")
    );
    assert_eq!(first.detail.len(), read(&lines_at("1:59")).detail.len() - 1);
    // The request is its own: another command is another dialog.
    let other: Vec<String> = CLAUDE_RM_PROMPT_LIVE
        .iter()
        .map(|line| line.replace("rm \"$CFA/$CFB\"", "rm \"$CFA/$CFC\""))
        .collect();
    assert_ne!(identity(&read(&other)), first);
}

/// A command that prints Claude Code's warning, as a box of the prompt draws its rows (the reader
/// drops the frame), its quoted time `clock`, the quoted line wrapped before its time when
/// `wrapped`; a hook's header and reason follow the command.
fn quoting_command(clock: &str, wrapped: bool) -> Vec<String> {
    let mut lines = owned(&["\u{2502} cat <<'NOTICE'"]);
    if wrapped {
        lines.push(format!(
            "\u{2502} {GLYPH} Claude Code will automatically block this request in"
        ));
        lines.push(format!(
            "\u{2502} {clock}, to avoid holding up an unattended session"
        ));
    } else {
        lines.push(format!("\u{2502} {}", blocks(clock).trim()));
    }
    lines.extend(owned(&[
        "\u{2502} NOTICE",
        "\u{2502} Hook tool.check requires confirmation for this command:",
        "\u{2502} Review the generated warning text.",
        "\u{2502} [plugin:review-warning]",
        "Do you want to proceed?",
        "\u{276f} 1. Yes",
        "  2. No",
    ]));
    lines
}

#[test]
fn a_warning_a_command_prints_is_the_commands_whatever_time_it_says() {
    // T-1282's blocker: the words of the warning in a command's rows, above a hook's header, are
    // command data; two commands that differ in the time they print are two requests.
    for wrapped in [false, true] {
        let at = |clock: &str| identity(&read(&quoting_command(clock, wrapped)));
        assert_ne!(at("1:57"), at("0:03"), "wrapped: {wrapped}");
        assert_eq!(at("1:57"), at("1:57"));
    }
}

#[test]
fn the_warning_is_the_block_that_ends_the_rows_above_the_question_and_nothing_else() {
    // Its place: the nearest rows above the question, all of them its sentence. A row after it, a
    // sentence that is not whole, or the words inside a longer row are not Claude Code's.
    let raw = |extra_after: &[&str], row: &str| {
        let mut lines = rm_prompt(COMMAND, &[row.to_owned()]);
        let at = lines
            .iter()
            .position(|l| l.contains("Do you want"))
            .unwrap();
        for (n, line) in extra_after.iter().enumerate() {
            lines.insert(at + n, (*line).to_owned());
        }
        identity(&read(&lines))
    };
    let sentence = |left: &str| blocks(left).trim().to_owned();
    // The native shape: ticks collapse.
    assert_eq!(raw(&[], &sentence("1:57")), raw(&[], &sentence("0:03")));
    // A row between it and the question: raw.
    assert_ne!(
        raw(&[" one more row"], &sentence("1:57")),
        raw(&[" one more row"], &sentence("0:03"))
    );
    // The sentence is not whole: raw.
    let whole = "to avoid holding up an unattended session";
    for after in [
        format!("{whole}."),
        format!("{whole} (quoted)"),
        "to avoid holding up an unattended".to_owned(),
    ] {
        let row = |left: &str| {
            format!("{GLYPH} Claude Code will automatically block this request in {left}, {after}")
        };
        assert_ne!(raw(&[], &row("1:57")), raw(&[], &row("0:03")), "{after:?}");
    }
}

#[test]
fn a_quoted_warning_above_the_native_one_keeps_its_time_and_the_native_one_is_blanked() {
    let screen = |quoted: &str, native: &str| -> Vec<String> {
        owned(&[
            " Bash command",
            "\u{2502} cat <<'NOTICE'",
            &format!("\u{2502} {GLYPH} Claude Code will automatically block this request in {quoted}, to avoid holding up an unattended session"),
            "\u{2502} NOTICE",
            &format!(" {}", blocks(native).trim()),
            " Do you want to proceed?",
            " \u{276f} 1. Yes",
            "   2. No",
        ])
    };
    let at = |quoted: &str, native: &str| identity(&read(&screen(quoted, native)));
    assert_eq!(at("1:57", "1:57"), at("1:57", "0:03"));
    assert_ne!(at("1:57", "1:57"), at("0:03", "1:57"));
}

#[test]
fn the_native_warning_is_one_line_whichever_way_the_screen_wrapped_it() {
    // Wrapped before its time, after it, inside "about 2 minutes", or not at all: the identity is
    // the sentence with its time blanked.
    let one = id(COMMAND, &[blocks("1:57")]);
    let wrapped = |rows: &[&str]| {
        id(
            COMMAND,
            &rows.iter().map(|r| format!(" {r}")).collect::<Vec<_>>(),
        )
    };
    let head = format!("{GLYPH} Claude Code will automatically block this request in");
    assert_eq!(
        wrapped(&[&head, "0:03, to avoid holding up an unattended session"]),
        one
    );
    assert_eq!(
        wrapped(&[
            &format!("{head} 9:59, to avoid holding"),
            "up an unattended session"
        ]),
        one
    );
    assert_eq!(
        wrapped(&[
            &format!("{head} about"),
            "2 minutes, to avoid holding up an unattended session"
        ]),
        one
    );
    assert_eq!(
        wrapped(&[
            &format!("{head} about 2"),
            "minutes, to avoid holding up an",
            "unattended session"
        ]),
        one
    );
    // A clock that is wider wraps it elsewhere: the same line.
    assert_eq!(
        wrapped(&[
            &format!("{head} 10:00, to avoid holding up"),
            "an unattended session"
        ]),
        one
    );
}

// ----- No row is cut on words alone (T-1300, after T-1316's review) -----
//
// A first attempt cut every row up to a title Claude Code draws, found in the first four rows. The
// reader drops the frame of a row, a blank row and a rule, so a title is a row of words, and a
// command that prints one, or a hook's reason that is one, is a title as far as the words go: the
// cut erased the command above it and two different commands were one identity.

/// A hook's ask of a command, the rows as the tail shows them: the command's rows, the hook's
/// header and `reason` rows, its source and the configuration hint Claude Code appends, the
/// question and its choices.
fn hook_ask(command: &[&str], reason: &[&str]) -> Vec<String> {
    let mut lines: Vec<String> = command
        .iter()
        .map(|row| format!("\u{2502} {row}"))
        .collect();
    lines.push("\u{2502} Hook tool.check requires confirmation for this command:".to_owned());
    lines.extend(reason.iter().map(|row| format!("\u{2502} {row}")));
    lines.extend(owned(&[
        "\u{2502} [plugin:review-shell]",
        "plugin hooks.json to update hooks",
        "Do you want to proceed?",
        "\u{276f} 1. Yes",
        "  2. No",
        "Esc to cancel \u{b7} Tab to amend",
    ]));
    lines
}

const TITLES_CLAUDE_DRAWS: [&str; 12] = [
    "Bash command",
    "Run command",
    "PowerShell command",
    "Edit file",
    "Create file",
    "Overwrite file",
    "Read file",
    "Fetch",
    "Use this skill?",
    "Tool use",
    "Bash command (unsandboxed)",
    "Computer Use needs macOS permissions",
];

#[test]
fn a_title_a_command_prints_is_a_row_of_the_command_and_cuts_nothing() {
    for title in TITLES_CLAUDE_DRAWS {
        let screen = |path: &str| {
            hook_ask(
                &[
                    &format!("rm -rf -- {path}"),
                    "printf '%s\\n' '",
                    title,
                    "cleanup finished'",
                ],
                &["Review this shell command."],
            )
        };
        let (one, other) = (
            read(&screen("$T/review-one")),
            read(&screen("$T/review-two")),
        );
        assert_eq!(identity(&one), one, "{title}: nothing is cut");
        assert_ne!(identity(&one), identity(&other), "{title}");
    }
}

#[test]
fn a_title_a_hooks_reason_is_cuts_nothing_and_claude_codes_own_formatter_draws_it_so() {
    // 2.1.319's formatter for a hook's reason "Bash command\n" gives these rows
    // (the offline render of the formatter in the review of T-1316): the header, the reason's line, the source.
    for title in TITLES_CLAUDE_DRAWS {
        let screen = |path: &str| hook_ask(&[&format!("rm -rf -- {path}")], &[title]);
        let (one, other) = (
            read(&screen("$T/review-one")),
            read(&screen("$T/review-two")),
        );
        assert_eq!(identity(&one), one, "{title}: nothing is cut");
        assert_ne!(identity(&one), identity(&other), "{title}");
    }
}

#[test]
fn a_command_row_that_slid_out_is_another_read_whatever_title_is_left_in_the_request() {
    for title in TITLES_CLAUDE_DRAWS {
        let whole = hook_ask(&["rm -rf -- $T/review-one"], &[title]);
        let slid = whole[1..].to_vec();
        assert_ne!(
            identity(&read(&whole)),
            identity(&read(&slid)),
            "{title}: the read without the command is another dialog"
        );
    }
}

#[test]
fn the_field_pair_is_two_identities_the_cost_of_never_merging_on_words() {
    // T-1306's window (m-7776 at 1:57, m-7780 at 0:05): the tail slid two rows. Nothing in what the
    // engine reads says the rows that went were the conversation's, so the second read is another
    // dialog and an extra notice is told.
    let request = [
        " Delete the sparse bundle by its literal path",
        " rm -rf /Volumes/Data/consensflow-builds.sparsebundle; echo \"rm finished rc=$?\"",
        " \u{2502} Dangerous rm operation on mounted drive path: /Volumes/Data/consensflow-builds.sparsebundle",
    ];
    let below = |clock: &str| {
        vec![
            blocks(clock),
            " Do you want to proceed?".to_owned(),
            " \u{276f} 1. Yes".to_owned(),
            "   2. No".to_owned(),
        ]
    };
    let mut first = owned(&["  \u{23bf} \u{a0}Waiting\u{2026}", " Bash command"]);
    first.extend(owned(&request));
    first.extend(below("1:57"));
    let mut second = owned(&request);
    second.extend(below("0:05"));
    let (first, second) = (read(&first), read(&second));
    assert_ne!(identity(&first), identity(&second));
    // The countdown alone is still blanked: the same read a countdown later is the same dialog.
    let mut later = owned(&["  \u{23bf} \u{a0}Waiting\u{2026}", " Bash command"]);
    later.extend(owned(&request));
    later.extend(below("0:05"));
    assert_eq!(identity(&first), identity(&read(&later)));
    // And what is kept is every row of the read but the time.
    assert_eq!(identity(&first).detail.len(), first.detail.len());
}

#[test]
fn a_request_that_prints_the_computer_use_title_is_cut_nowhere() {
    // The Computer Use prompt's cut needs its own choices, which a request's words cannot supply
    // (they are read from the cursor rows): a Yes/No dialog with that title among its rows keeps all.
    let screen = |path: &str| {
        hook_ask(
            &[
                &format!("rm -rf -- {path}"),
                "Computer Use needs macOS permissions",
            ],
            &["Review this shell command."],
        )
    };
    let (one, other) = (
        read(&screen("$T/review-one")),
        read(&screen("$T/review-two")),
    );
    assert_eq!(identity(&one), one);
    assert_ne!(identity(&one), identity(&other));
}
