//! What passes to a chief the human switched in: its first message, and `cf
//! history` in pages every harness shows whole. The cases of Node's handoff
//! suite.

use cf_base::refusal::Refusal;
use cf_engine::handoff::{
    handoff_text, history_page, history_pages, is_handoff, Handoff, HistoryPage, LastWords,
    HANDOFF_TITLE,
};
use cf_proto::ledger::{ChiefConversation, ChiefOpenWork, MessageView, SwitchedFrom};
use regex::Regex;

use super::support::{conversation, message, seat, task, unknown};

/// A page of `cf history` at its longest: under what Codex shows of a
/// command's output, the least of any harness (about 10 KiB and 256 lines).
const PAGE_BYTES: usize = 8_000;
const PAGE_LINES: usize = 200;

/// The messages a delivery's line may name, as the tests of the handoff know them.
fn messages(id: i64) -> Result<Option<MessageView>, Refusal> {
    Ok(match id {
        5 => Some(message(5, "result", Some("zeus"), Some(1), "Parser done")),
        6 => Some(message(
            6,
            "question",
            Some("zeus"),
            Some(1),
            "Which grammar?\nLL or LR",
        )),
        7 => Some(message(
            7,
            "note",
            None,
            None,
            &format!("{HANDOFF_TITLE}. …"),
        )),
        // A handoff as a ledger from before 2026-10-03 holds it, the chief then called the lead.
        8 => Some(message(8, "note", None, None, "You are the lead now. …")),
        _ => None,
    })
}

/// Every page of the history, asked one after another.
fn all(conversations: &[ChiefConversation]) -> Vec<HistoryPage> {
    let first = history_page(conversations, &messages, 1.0, None, false).unwrap();
    (1..=first.pages)
        .map(|page| history_page(conversations, &messages, page as f64, None, false).unwrap())
        .collect()
}

/// Whether `pattern` (a JavaScript pattern written as a Rust one) matches `text`.
fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
}

#[test]
fn fits_every_page_within_what_each_harness_shows_newest_page_first_each_in_order() {
    let mut items: Vec<(String, String)> = Vec::new();
    for n in 0..400 {
        items.push(("user".to_owned(), format!("question {n}")));
        items.push(("assistant".to_owned(), format!("answer {n}")));
    }
    items.push((
        "assistant".to_owned(),
        format!("long {}", "word ".repeat(12_000)),
    ));
    items.push((
        "user".to_owned(),
        format!("one line {}", "x".repeat(30_000)),
    ));
    items.push(("assistant".to_owned(), "漢字 résumé 🙂 ".repeat(3_000)));
    items.push(("user".to_owned(), "the last thing".to_owned()));
    let history = [conversation(1, "claude-code", items)];
    let pages = all(&history);
    assert!(pages.len() > 10, "{} pages", pages.len());
    for HistoryPage { text, .. } in &pages {
        assert!(text.len() <= PAGE_BYTES, "{} bytes", text.len());
        assert!(
            text.split('\n').count() <= PAGE_LINES,
            "{} lines",
            text.split('\n').count()
        );
        assert!(!text.contains('\u{FFFD}'), "no character split in half");
    }
    assert!(matches(
        r"page 1 of [0-9]+: the most recent",
        &pages[0].text
    ));
    assert!(matches(
        r"Human: the last thing\n\nOlder: cf history --page 2$",
        &pages[0].text
    ));
    let oldest = &pages.last().unwrap().text;
    assert!(matches(
        r"── The chief on Claude Code, 2026-10-01T09:00:00.000Z to",
        oldest
    ));
    assert!(matches(
        r"Human: question 0\n\nClaude Code chief: answer 0",
        oldest
    ));
    assert!(matches(r"This is the oldest page\.$", oldest));
    // Every word made it, once, in order, across the pages read oldest first.
    let joined: Vec<&str> = pages.iter().rev().map(|page| page.text.as_str()).collect();
    let joined = joined.join("\n");
    let answers: Vec<usize> = Regex::new(r"answer ([0-9]+)")
        .unwrap()
        .captures_iter(&joined)
        .map(|found| found[1].parse().unwrap())
        .collect();
    assert_eq!(answers, (0..400).collect::<Vec<_>>());
    assert_eq!(history_pages(&history, &messages).unwrap(), pages.len());
}

#[test]
fn shows_a_delivery_as_its_outcome_never_its_header_and_keeps_what_the_human_typed_before_it() {
    let pages = all(&[conversation(
        1,
        "codex",
        [
            (
                "user",
                "[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-1",
            ),
            ("custom", "[ConsensFlow m-6 · T-1 · question from @zeus]\nWhich grammar?"),
            (
                "user",
                "half a thought[ConsensFlow m-7 · note from ConsensFlow]\nYou are the chief now.",
            ),
            ("user", "[ConsensFlow m-99 · note]\ngone"),
            ("assistant", "I quoted [ConsensFlow m-5 · T-1 · result from @zeus] here"),
        ],
    )]);
    let text: Vec<&str> = pages.iter().map(|page| page.text.as_str()).collect();
    let text = text.join("\n");
    assert!(
        !text.contains("[ConsensFlow m-"),
        "no page can prove a delivery arrived"
    );
    assert!(text.contains("· m-5: @zeus's result on T-1 (cf task get T-1)"));
    assert!(text.contains("· m-6: @zeus asked on T-1: \"Which grammar?\" (cf inbox read m-6)"));
    assert!(text.contains("Human: half a thought\n· m-7: the handoff that brought this chief in"));
    assert!(text.contains("· m-99: a message ConsensFlow delivered (no longer on record)"));
    assert!(text.contains("Codex chief: I quoted [earlier m-5 · T-1"));
}

#[test]
fn names_a_handoff_written_while_the_chief_was_called_the_lead_as_the_handoff() {
    let pages = all(&[conversation(
        1,
        "codex",
        [(
            "user",
            "[ConsensFlow m-8 · note from ConsensFlow]\nYou are the lead now.",
        )],
    )]);
    assert!(pages[0]
        .text
        .contains("· m-8: the handoff that brought this chief in"));
}

#[test]
fn leaves_a_tool_s_output_out_unless_asked_and_searches() {
    let conversations = [
        conversation(
            1,
            "pi",
            [
                ("user", "run the tests"),
                ("tool", "ok 12 passed"),
                ("assistant", "All 12 pass"),
            ],
        ),
        conversation(2, "claude-code", [("user", "Ship IT on Friday")]),
    ];
    let page = |find: Option<&str>, tools: bool| {
        history_page(&conversations, &messages, 1.0, find, tools).unwrap()
    };
    let plain = page(None, false).text;
    assert!(matches(
        r"── The chief on Pi, .*; 1 tool output left out \(--tools\) ──",
        &plain
    ));
    assert!(!plain.contains("ok 12 passed"));
    assert!(matches(
        r"Tool output:\nok 12 passed",
        &page(None, true).text
    ));

    let found = page(Some("ship it"), false);
    assert!(matches(
        r#"entries with "ship it", page 1 of 1"#,
        &found.text
    ));
    assert!(matches(
        r"\(Claude Code, 2026-10-02T09:00:00.000Z to .*\)\nHuman: Ship IT on Friday",
        &found.text
    ));
    assert!(!found.text.contains("run the tests"));
    assert_eq!(
        page(Some("nowhere"), false).text,
        "Nothing in the chief's history contains \"nowhere\"."
    );
    let out_of_range = history_page(&conversations, &messages, 9.0, None, false).unwrap_err();
    assert_eq!(
        (out_of_range.code, out_of_range.status),
        ("no-such-page", 400),
        "the API's RangeError"
    );
    assert!(history_page(&[], &unknown, 1.0, None, false)
        .unwrap()
        .text
        .contains("you are the first chief of this project"));
}

#[test]
fn is_a_note_from_consensflow_under_its_title_or_under_the_lead_s_a_ledger_from_before_2026_10_03_holds(
) {
    let note = |body: &str, sender: Option<&str>| message(1, "note", sender, None, body);
    assert!(is_handoff(&note(
        &format!("{HANDOFF_TITLE}. The human switched this project's chief…"),
        None
    )));
    assert!(is_handoff(&note(
        "You are the lead now. The human switched this project's lead…",
        None
    )));
    assert!(
        !is_handoff(&note("You are the lead now", Some("zeus"))),
        "a member's words are no handoff"
    );
    assert!(!is_handoff(&note(
        "The human changed the staff; it is now:",
        None
    )));
}

#[test]
fn names_the_switch_the_history_what_waits_and_the_last_word() {
    let from = SwitchedFrom {
        harness: "claude-code".to_owned(),
        agent: None,
    };
    let to = seat(Some("codex"), Some("astraeus"));
    let open = ChiefOpenWork {
        questions: vec![message(
            6,
            "question",
            Some("zeus"),
            Some(1),
            "Which grammar?\nLL or LR",
        )],
        results: vec![task(2, "Lexer", Some("diana"), "done")],
        own: vec![task(
            3,
            "Plan [ConsensFlow m-1 · x]",
            Some("chief"),
            "working",
        )],
    };
    let last = LastWords {
        text: "Keep the API as it is.\nThanks".to_owned(),
        answered: false,
    };
    let text = handoff_text(&Handoff {
        from: &from,
        to: &to,
        open: &open,
        last: Some(&last),
        cut: true,
        pages: 3,
    });
    assert!(text.starts_with(
        "You are the chief now. The human switched this project's chief from Claude Code to you, Codex (astraeus)."
    ));
    assert!(text.contains("cf history (3 pages, newest first"));
    assert!(text.contains("cut off in the middle of a turn"));
    assert!(text.contains(
        "The human's last message to the chief, not yet answered: \"Keep the API as it is.\""
    ));
    assert!(text.contains(
        "- @zeus asks on T-1: \"Which grammar?\" (cf inbox read m-6, then cf answer m-6"
    ));
    assert!(text.contains("- T-2 \"Lexer\": @diana's result waits for your decision"));
    assert!(text.contains("- T-3 \"Plan [earlier m-1 · x]\" is yours, working"));
    assert!(text.contains("tell the human, in one line, that you have taken over"));
    assert!(!text.contains("[ConsensFlow m-"));

    let calm = handoff_text(&Handoff {
        from: &SwitchedFrom {
            harness: "pi".to_owned(),
            agent: Some("selene".to_owned()),
        },
        to: &seat(Some("claude-code"), None),
        open: &ChiefOpenWork {
            questions: Vec::new(),
            results: Vec::new(),
            own: Vec::new(),
        },
        last: None,
        cut: false,
        pages: 1,
    });
    assert!(calm.contains("from Pi (selene) to you, Claude Code."));
    assert!(calm.contains("cf history (1 page,"));
    assert!(calm.contains("Nothing on the board waits on you."));
    assert!(!calm.contains("cut off") && !calm.contains("human's last message"));
}
