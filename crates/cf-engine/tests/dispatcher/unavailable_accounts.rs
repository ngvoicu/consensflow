//! A member whose account refused for what no wait cures (its billing, its login: a 402,
//! a 401, MUS-23) is unavailable, not out until a reset: its task is held with its window
//! and goes on at no time of its own; the requester and the human are told once; and only
//! the human's Try again now brings it back.

use cf_engine::testing::Context;
use std::sync::Arc;

use cf_harness::records::{Level, Quota};

use crate::fixtures::{after, assert_match, now, placed, Tiers};

const BECAUSE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

fn unavailable(context: &Context) -> Quota {
    Quota::Unavailable {
        at: Some(now(context)),
        because: BECAUSE.to_owned(),
    }
}

/// The chief's notes that T-1 waits, oldest first, each typed or shown, counted in runs.
fn hold_notes(context: &Context, tiers: &Tiers) -> Vec<(&'static str, usize)> {
    let mut notes = context.inbox(tiers.id("chief"));
    notes.reverse();
    let mut runs: Vec<(&'static str, usize)> = Vec::new();
    for note in notes
        .iter()
        .filter(|note| note.body.starts_with("T-1 waits"))
    {
        let how = if note.receipt.get(cf_ledger::SHOWN).is_some() {
            "shown"
        } else {
            "typed"
        };
        match runs.last_mut() {
            Some((last, count)) if *last == how => *count += 1,
            _ => runs.push((how, 1)),
        }
    }
    runs
}

/// The notes the human was sent, oldest first.
fn human_notes(tiers: &Tiers) -> Vec<String> {
    crate::fixtures::notes(tiers.context, tiers.context.id(tiers.project.id, "human"))
}

#[test]
fn holds_the_task_with_its_window_for_no_reset_and_tells_the_requester_and_the_human_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.assignee.as_deref()),
        ("paused", Some("zeus-amber-pine")),
        "held with its window: diana is free, but only the human or the chief gives it to another"
    );
    assert!(
        held.held_until
            .as_deref()
            .is_some_and(|until| until.starts_with("9999-")),
        "no reset: it is due at no time of its own, {:?}",
        held.held_until
    );
    assert!(tiers
        .out_until("zeus")
        .is_some_and(|until| until.starts_with("9999-")));
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"^T-1 waits with @zeus-amber-pine: its account is unavailable \(Muse refused: the account has no allowance; set up billing at accountscenter\.meta\.com/muse_code\), and nothing resets that; it goes on when the human says the account works again\.$",
    );
    // The first note of a hold is typed (RS-25), and T-1's line says the hold as well.
    assert_eq!(
        hold_notes(&context, &tiers),
        [("typed", 1)],
        "the first note of the hold is typed"
    );
    assert_eq!(
        tiers.task(1).task.aside,
        Some(format!(
            "waits with @zeus-amber-pine: its account is unavailable ({BECAUSE}) until the human says it works"
        ))
    );
    let told = human_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_match(
        &told[0],
        r"^@zeus-amber-pine's account is unavailable: Muse refused: the account has no allowance; .* Nothing is sent to it until you say it works again: press Try again now on @zeus once it does\.$",
    );
    // Hours later, passes and a window that still shows the refusal change nothing, and
    // nobody is told again.
    for _ in 0..3 {
        context.advance(2 * 3_600_000);
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(human_notes(&tiers).len(), 1);
    assert_eq!(
        tiers
            .notes("chief")
            .iter()
            .filter(|note| note.starts_with("T-1 waits"))
            .count(),
        1
    );
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id("zeus-amber-pine"), 100)
        .unwrap();
    assert!(
        inbox
            .iter()
            .all(|message| !message.body.starts_with("Resumed:")),
        "no retry but at the human's word"
    );
    // Another task of the tier goes to the member that is available.
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let second = tiers.task(2).task;
    assert_eq!(second.state, "working");
    assert!(
        second
            .assignee
            .as_deref()
            .is_some_and(|who| who.starts_with("diana")),
        "{:?}",
        second.assignee
    );
}

#[test]
fn a_second_window_of_the_member_refused_a_minute_later_holds_its_task_and_the_human_is_not_told_again(
) {
    // Two windows of one member run into the same refused account: each task is held and its
    // requester told, and the human, who has one account to fix, hears it once.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    context.adapter.refuse(&one, unavailable(&context));
    context.pass().unwrap();
    context.advance(60_000);
    context.adapter.refuse(&two, unavailable(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    let waits: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains(" waits with @"))
        .collect();
    assert_eq!(waits.len(), 2, "{waits:?}");
    assert_eq!(human_notes(&tiers).len(), 1, "{:?}", human_notes(&tiers));
}

#[test]
fn the_human_saying_the_account_works_again_brings_the_member_back_and_its_task_goes_on() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    context.advance(30 * 60_000);
    let back = context.back_from_quota(tiers.project.id, "zeus").unwrap();
    assert_eq!(back.out_until, None);
    // Its window reads at rest again: the account works.
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    // A refusal that came after its member was marked back is news again, told again.
    context.adapter.refuse(
        "zeus-amber-pine",
        Quota::Unavailable {
            at: Some(after(&context, 60_000)),
            because: BECAUSE.to_owned(),
        },
    );
    context.advance(2 * 60_000);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(human_notes(&tiers).len(), 2);
    // No turn got through between: the requester's note is the same hold said again,
    // shown on T-1's line, not typed (RS-25).
    assert_eq!(hold_notes(&context, &tiers), [("typed", 1), ("shown", 1)]);
    assert!(
        tiers
            .task(1)
            .task
            .aside
            .is_some_and(|aside| aside.ends_with("until the human says it works")),
        "{:?}",
        tiers.task(1).task.aside
    );
}

#[test]
fn an_allowance_the_harness_shows_again_brings_an_unavailable_member_back_a_still_full_one_does_not(
) {
    brought_back_by_a_count_of(Level::Ok, 3);
}

#[test]
fn an_allowance_nearly_spent_brings_an_unavailable_member_back_too() {
    // A count past 95 percent (`Level::Low`) holds no member (T-1356): it is an allowance.
    brought_back_by_a_count_of(Level::Low, 96);
}

/// A member whose account refused for what no wait cures is back once the harness shows a count
/// that is not used up (`level`, `used` percent), and not before.
fn brought_back_by_a_count_of(level: Level, used: u64) {
    // Muse's `usage` says the account has an allowance (MUS-23): that is the harness's own word
    // that the refusal is over, and no human's is needed. A count that says used up is not.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let usage = |level, used: u64| Quota::Usage {
        level,
        used_percent: Some(used.into()),
        resets_at: None,
    };
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.failed = false;
        agent.quota = Some(Arc::new(usage(Level::Exhausted, 100)));
    });
    context.pass().unwrap();
    assert!(
        tiers.out_until("zeus").is_some(),
        "a count used up is no allowance"
    );
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.quota = Some(Arc::new(usage(level, used)));
    });
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "its allowance is back");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine")),
        "its task went on"
    );
}

#[test]
fn keeps_the_harnesss_words_with_the_hold_for_the_board_to_say() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    let board =
        serde_json::to_value(context.ledger.borrow().board(tiers.project.id).unwrap()).unwrap();
    let zeus = board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["handle"] == "zeus")
        .expect("zeus's lane");
    assert_eq!(
        zeus["participant"]["outBecause"], BECAUSE,
        "the board says the harness's words from the hold, with no window to say them"
    );
}

/// What a refusal's words carry that must never reach the ledger, the trace or the board.
const KEY: &str = "sk-ant-api03-Zq8vN2mLk4Rt7XyP1wUc5Hd9";
const ADDRESS: &str = "owner.account@example.com";

fn everything_kept_of_the_hold(context: &Context, tiers: &Tiers) -> String {
    let board =
        serde_json::to_string(&context.ledger.borrow().board(tiers.project.id).unwrap()).unwrap();
    let events = serde_json::to_string(
        &context
            .ledger
            .borrow()
            .events(tiers.project.id, 0, 5000)
            .unwrap(),
    )
    .unwrap();
    format!("{board}\n{events}\n{}", tiers.notes("chief").join("\n"))
}

#[test]
fn a_refusal_that_names_a_key_or_a_login_is_kept_with_none_of_its_words_and_says_which_it_was() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.refuse(
        "zeus-amber-pine",
        Quota::Unavailable {
            at: Some(now(&context)),
            because: format!(
                "its login was refused: 401 invalid x-api-key {KEY} for {ADDRESS}, password: hunter2"
            ),
        },
    );
    context.pass().unwrap();
    let kept = everything_kept_of_the_hold(&context, &tiers);
    for secret in [KEY, ADDRESS, "hunter2"] {
        assert!(
            !kept.contains(secret),
            "{secret} reached what is kept:\n{kept}"
        );
    }
    let board: serde_json::Value =
        serde_json::to_value(context.ledger.borrow().board(tiers.project.id).unwrap()).unwrap();
    let zeus = board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["handle"] == "zeus")
        .unwrap();
    let said = zeus["participant"]["outBecause"].as_str().unwrap();
    assert!(said.starts_with("its login was refused"), "{said}");
    assert!(said.contains("left out"), "{said}");
}

/// The most characters of a hold's words that are kept after the screen: one bound, at the place
/// the words are written (T-2353), for every harness and every kind of hold.
const BOUND: usize = 300;

/// A hold's report made from the harness's words and the time of the refusal.
type Made = fn(String, String) -> Quota;

/// How each kind of hold's report is made from the harness's words, each with the lead the screen strips from the words (a refusal's), and the words' own start.
fn holds() -> [(&'static str, &'static str, Made); 3] {
    [
        ("its login was refused: ", "401 ", |because, at| {
            Quota::Unavailable {
                at: Some(at),
                because,
            }
        }),
        (
            "",
            "its model moved from a to b on its own (",
            |because, at| Quota::Moved {
                at: Some(at),
                because,
            },
        ),
        ("", "503 ", |because, at| Quota::Busy {
            at: Some(at),
            because,
        }),
    ]
}

/// Zeus held by `quota`, and everything kept of it: the ledger's events, the board, the notes,
/// and the window's activity; with what the board says of the hold.
fn zeus_held_by(quota: impl FnOnce(&Context) -> Quota) -> (String, String, String) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.refuse("zeus-amber-pine", quota(&context));
    context.pass().unwrap();
    let kept = everything_kept_of_the_hold(&context, &tiers);
    let board =
        serde_json::to_value(context.ledger.borrow().board(tiers.project.id).unwrap()).unwrap();
    let said = board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["handle"] == "zeus")
        .unwrap()["participant"]["outBecause"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let activity = context
        .dispatcher
        .activity(tiers.id("zeus-amber-pine"))
        .reason
        .unwrap_or_default();
    (kept, said, activity)
}

#[test]
fn a_hold_s_words_are_screened_whole_before_they_are_cut_whatever_their_length_and_kind() {
    // The password fits in the first 300 characters and its terminating `@` is character 301:
    // the screen sees the whole words, and nothing of them is kept (T-2353, T-2350's check).
    for (prefix, lead, hold) in holds() {
        let url_head = "https://test-user:test-password";
        let fill = BOUND - lead.len() - url_head.len();
        let filler: String = " .".repeat(fill).chars().take(fill).collect();
        // The `@` is character 301 of the harness's words, which a refusal's lead precedes.
        let words =
            format!("{prefix}{lead}{filler}{url_head}@gateway.invalid and the rest of its words");
        let (kept, said, activity) = zeus_held_by(|context| hold(words.clone(), now(context)));
        for place in [&kept, &said, &activity] {
            assert!(!place.contains("test-password"), "{lead}: {place}");
            assert!(!place.contains("gateway.invalid"), "{lead}: {place}");
        }
    }
}

#[test]
fn a_hold_s_words_are_kept_to_one_bound_whatever_the_harness_cut_before_whatever_the_kind() {
    // Codex's `unauthorized` message and a model's move reach the dispatcher as long as they
    // are: the bound is set once, after the screen, where the words are written.
    for (prefix, lead, hold) in holds() {
        let words = format!("{prefix}{lead}{}", "plain words ".repeat(100));
        let (kept, said, activity) = zeus_held_by(|context| hold(words.clone(), now(context)));
        for place in [&said, &activity] {
            assert!(
                place.chars().count() <= prefix.len() + lead.len() + BOUND + 120,
                "{lead}: {} characters: {place}",
                place.chars().count()
            );
        }
        assert!(
            !kept.contains(&"plain words ".repeat(40)),
            "{lead}: the tail of the words was kept: {kept}"
        );
    }
}
