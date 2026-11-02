//! The scenarios: each is what the JavaScript's was, word for word (their prose is
//! the eval, and a word changed in a prompt changes what is measured), and each
//! expectation holds on the numbers of a run that did what it asks and fails on
//! those of a run that did not (`tests/scenarios/verdicts.rs`), by the name a report
//! says it by; and on the numbers of the eleven ledgers recorded from real runs and
//! made ones, each judges as the JavaScript's did. A scenario written after the
//! JavaScript (`AFTER_NODE`) has no golden, and its verdicts alone hold it.

#![allow(clippy::unwrap_used)]

use cf_evals::scenarios::{self, FollowUp, Scenario};
use regex::Regex;
use serde_json::{json, Value};

mod support;

// The verdicts are in `tests/scenarios/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "scenarios/verdicts.rs"]
mod verdicts;

/// What the JavaScript scenarios said, as data: `tests/fixtures/scenarios.json`.
/// A pattern's source is JavaScript's, which writes a slash as `\/`; the pattern
/// of a scenario here has the slash as it is.
fn golden() -> Vec<Value> {
    let text = include_str!("fixtures/scenarios.json");
    let mut scenarios = serde_json::from_str::<Value>(text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    for scenario in &mut scenarios {
        for answer in scenario["answers"].as_array_mut().unwrap() {
            let source = answer["source"].as_str().unwrap().replace("\\/", "/");
            answer["source"] = json!(source);
        }
    }
    scenarios
}

/// A scenario as the golden says it: the pattern of an answer as its source and
/// its flags, a switch as `{ "switch": true }`.
fn described(scenario: &Scenario) -> Value {
    json!({
        "id": scenario.id,
        "title": scenario.title,
        "fixture": scenario.fixture,
        "prompt": scenario.prompt,
        "followUps": scenario.follow_ups.iter().map(|step| match step {
            FollowUp::Say(text) => json!(text),
            FollowUp::Switch => json!({ "switch": true }),
        }).collect::<Vec<_>>(),
        "answers": scenario.answers.iter().map(|answer| {
            let (flags, source) = match answer.matches.as_str().strip_prefix("(?i)") {
                Some(source) => ("i", source),
                None => ("", answer.matches.as_str()),
            };
            json!({ "source": source, "flags": flags, "text": answer.text })
        }).collect::<Vec<_>>(),
        "fallback": scenario.fallback,
        "nudge": scenario.nudge,
        "quietMs": u64::try_from(scenario.quiet.as_millis()).unwrap(),
        "expectations": scenario.expectations.iter().map(|e| e.name).collect::<Vec<_>>(),
    })
}

/// The scenario `id`, whose random codeword (`chief-switch`) is the one the golden
/// stands a placeholder for.
pub fn scenario(id: &str) -> Scenario {
    if id == "chief-switch" {
        scenarios::chief_switch_with("@CODEWORD@")
    } else {
        scenarios::by_id(id).unwrap()
    }
}

/// The scenarios written after the JavaScript was gone, which no golden holds: their
/// expectations are held by `tests/scenarios/verdicts.rs` alone.
const AFTER_NODE: [&str; 3] = ["review-blockers", "review-loop", "uncommitted-work"];

#[test]
fn every_scenario_says_what_the_javascript_one_said_word_for_word() {
    let golden = golden();
    assert_eq!(golden.len() + AFTER_NODE.len(), scenarios::IDS.len());
    for said in golden {
        let id = said["id"].as_str().unwrap();
        assert!(!AFTER_NODE.contains(&id), "{id}");
        assert_eq!(described(&scenario(id)), said, "{id}");
    }
}

/// What each scenario's expectations said of the numbers of each recorded ledger
/// (`tests/fixtures/ledgers/<name>.json` of the repository) while they were
/// JavaScript's, as a string with a `1` for each that held and a `0` for each that
/// did not, in the scenario's order: `tests/fixtures/verdicts.json`. The numbers are
/// the recording's own, which the measure is held to the byte
/// (`tests/measure/recordings.rs`), so a scenario that judges them otherwise has an
/// expectation that is not the one it was.
#[test]
fn each_scenario_judges_each_recorded_ledger_as_the_javascript_one_did() {
    let golden: Value = serde_json::from_str(include_str!("fixtures/verdicts.json")).unwrap();
    let recordings = golden.as_object().unwrap();
    assert_eq!(recordings.len(), 11);
    for (name, said) in recordings {
        let metrics = support::measured(name);
        for id in scenarios::IDS.iter().filter(|id| !AFTER_NODE.contains(id)) {
            let judged: String = scenario(id)
                .verdict(&metrics)
                .iter()
                .map(|check| if check.ok { '1' } else { '0' })
                .collect();
            assert_eq!(judged, said[id].as_str().unwrap(), "{id} on {name}");
        }
    }
}

#[test]
fn the_fifteen_scenarios_are_found_by_their_ids_and_no_other() {
    assert_eq!(scenarios::IDS.len(), 15);
    for id in scenarios::IDS {
        assert_eq!(scenarios::by_id(id).unwrap().id, id);
    }
    assert!(scenarios::by_id("no-such-scenario").is_none());
    assert!(scenarios::by_id("").is_none());
    let mut ids = scenarios::IDS.to_vec();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 15, "one each");
}

#[test]
fn only_the_uncommitted_work_has_a_change_of_the_owner_s_reads_its_page_and_counts_the_files_gone()
{
    for id in scenarios::IDS {
        let scenario = scenarios::by_id(id).unwrap();
        if id == "uncommitted-work" {
            let change = scenario.uncommitted.unwrap();
            assert_eq!(change.file, "site/evaluare.html");
            assert_eq!(scenario.reads(), ["site/evaluare.html"]);
            assert!(scenario.counts_gone_files(), "{id}");
            // The prompt says nothing of the owner's change: that is what a worker must
            // find for itself.
            assert!(!scenario.prompt.contains(change.replace), "{id}");
            assert!(!scenario.prompt.contains("100"), "{id}");
            // The undo is the owner's second message, never the first's: the run reads
            // the page T-1 left just before it is typed (Sol, 2026-10-15).
            assert!(!scenario.prompt.contains("--after"), "{id}");
            let [FollowUp::Say(undo)] = scenario.follow_ups.as_slice() else {
                panic!("{id}: one message after the first, the undo");
            };
            assert!(undo.contains("cf task add --after T-1"), "{undo}");
            assert!(
                !undo.contains(change.replace) && !undo.contains("100"),
                "{undo}"
            );
        } else {
            // The others' reports keep their shape: no file read, no file gone counted.
            assert!(scenario.uncommitted.is_none(), "{id}");
            assert!(scenario.reads().is_empty(), "{id}");
            assert!(!scenario.counts_gone_files(), "{id}");
        }
    }
}

#[test]
fn only_the_chief_switch_scenario_switches_the_chief_and_its_codeword_is_a_bird_and_four_digits() {
    for id in scenarios::IDS {
        assert_eq!(
            scenarios::by_id(id).unwrap().switches_the_chief(),
            id == "chief-switch",
            "{id}"
        );
    }
    let codeword = Regex::new(r"^(TERN|LARK|WREN|KITE|ROOK|SWIFT)-\d{4}$").unwrap();
    let prompt = scenarios::by_id("chief-switch").unwrap().prompt;
    let said = Regex::new(r"The codeword for this release is (\S+)\. ")
        .unwrap()
        .captures(&prompt)
        .map(|found| found[1].to_owned())
        .unwrap();
    assert!(codeword.is_match(&said), "{said}");
    // Another each time it is asked for, in a sample of a few.
    let all: std::collections::HashSet<String> = (0..20)
        .map(|_| scenarios::by_id("chief-switch").unwrap().prompt)
        .collect();
    assert!(all.len() > 1);
}

/// The first words of a result a reviewer begins with, by `skill/core/staff.md`: whether the
/// work is ready.
const READINESS: [&str; 3] = ["Not ready.", "Ready with notes.", "Ready."];

#[test]
fn only_the_review_scenarios_script_a_reviewer_and_each_review_has_the_form_the_role_text_asks_of_one(
) {
    for id in scenarios::IDS {
        let scenario = scenarios::by_id(id).unwrap();
        if id != "review-loop" && id != "review-blockers" {
            assert!(scenario.stand_in_reviews.is_empty(), "{id}");
            assert!(scenario.stand_in_work.is_empty(), "{id}");
            continue;
        }
        assert_eq!(
            scenario.stand_in_reviews.len(),
            3,
            "{id}: three reviews, the last again for any more"
        );
        for (at, review) in scenario.stand_in_reviews.iter().enumerate() {
            // Whether the work is ready, first; and what the chief has to decide.
            assert!(
                READINESS.iter().any(|ready| review.starts_with(ready)),
                "{id} review {at}: {review}"
            );
            let first_line = review.lines().next().unwrap();
            assert!(
                first_line.contains("Decide"),
                "{id} review {at}: {first_line}"
            );
            // Each finding numbered so that a later review can name it, with whether it blocks.
            let findings: Vec<&str> = review
                .lines()
                .filter(|line| {
                    line.split_once(' ').is_some_and(|(word, _)| {
                        word.strip_prefix('F')
                            .is_some_and(|n| n.parse::<u8>().is_ok())
                    })
                })
                .collect();
            assert!(!findings.is_empty(), "{id} review {at}");
            for finding in &findings {
                assert!(
                    finding.contains("[blocks]")
                        || finding.contains("does not block")
                        || finding.contains("fixed")
                        || finding.contains("[blocks"),
                    "{id} review {at}: {finding}"
                );
            }
            // A blocker names its requirement, its case, its failure and its evidence.
            if review.contains("[blocks") {
                for part in [
                    "Requirement:",
                    "Case:",
                    "Failure:",
                    "Evidence:",
                    "Location:",
                ] {
                    assert!(review.contains(part), "{id} review {at} lacks {part}");
                }
            }
        }
    }
}

#[test]
fn the_review_loop_holds_one_blocker_and_notes_then_notes_only_then_notes_only_again() {
    let scenario = scenarios::by_id("review-loop").unwrap();
    let [first, second, third] = scenario.stand_in_reviews.as_slice() else {
        panic!("three reviews");
    };
    assert!(first.starts_with("Not ready."));
    assert_eq!(first.matches("[blocks]").count(), 1, "one bug");
    assert!(first.matches("[note").count() >= 3, "several notes");
    for later in [second, third] {
        assert!(later.starts_with("Ready with notes."), "{later}");
        assert!(!later.contains("[blocks"), "notes only: {later}");
    }
    // The second says of the bug that it is fixed; the third has nothing of it left to say.
    assert!(second.contains("F1 fixed."));
    assert!(!third.contains("F1"));
}

#[test]
fn review_blockers_holds_a_blocker_and_then_a_different_one_that_the_fix_left() {
    let scenario = scenarios::by_id("review-blockers").unwrap();
    let [first, second, third] = scenario.stand_in_reviews.as_slice() else {
        panic!("three reviews");
    };
    assert!(first.starts_with("Not ready.") && first.contains("F1 [blocks]"));
    assert!(
        second.starts_with("Not ready.")
            && second.contains("F6 new [blocks, caused by the fix to F1]")
    );
    assert!(second.contains("F1 fixed."));
    assert!(
        !second.contains("F1 [blocks]"),
        "a different blocker, not the first again"
    );
    assert!(third.starts_with("Ready with notes.") && !third.contains("[blocks"));
    // The owner knows which scale is the product's, and says so to a question about it.
    let asked = scenario.answers.iter().any(|answer| {
        answer
            .matches
            .is_match("Care e scala corectă, de la 1 la 5 sau de la 0 la 100?")
            && answer.text.contains("1 la 5")
    });
    assert!(asked);
}

#[test]
fn the_review_prompts_say_what_the_work_is_and_that_it_is_reviewed_and_nothing_of_what_to_do_with_a_finding(
) {
    for id in ["review-loop", "review-blockers"] {
        let prompt = scenarios::by_id(id).unwrap().prompt;
        assert!(
            prompt.contains("site/en/assessment.html") && prompt.contains("reviewer"),
            "{id}"
        );
        // What the chief does with a finding is what the eval measures: the owner says none of it.
        for word in [
            "F1",
            "reopen",
            "redeschid",
            "înapoi",
            "blocant",
            "1 la 5",
            "0 și 100",
        ] {
            assert!(
                !prompt.to_lowercase().contains(&word.to_lowercase()),
                "{id}: {word}"
            );
        }
        // The owner requires the page to give the score as the manager's guide does, and does
        // not say what that is: the chief reads the guide for itself. The page the scripted
        // worker writes first does not give it, so the bug is the work's own, by construction.
        assert!(
            prompt.contains("ghidul managerului (docs/ghidul-managerului.md)"),
            "{id}: {prompt}"
        );
        assert!(
            prompt.contains("Pagina engleză trebuie să dea scorul așa"),
            "{id}"
        );
        // And it forbids touching another file, which is why the Romanian page is the second blocker's.
        assert!(prompt.contains("Nu modifica alte fișiere"), "{id}");
    }
    // One asks the chief to leave the owner out of it, the other lets it ask.
    assert!(scenarios::by_id("review-loop")
        .unwrap()
        .prompt
        .contains("Nu mă întreba"));
    assert!(scenarios::by_id("review-blockers")
        .unwrap()
        .prompt
        .contains("întreabă-mă"));
}

#[test]
fn the_first_blocker_states_its_requirement_against_the_guide_alone_and_opens_both_scenarios() {
    // The Romanian page and the guide disagree and the owner forbids touching other files: a
    // requirement against both could not be met inside the work, and a chief that sent the
    // source's discrepancy to a later task would fail for it.
    let first = scenarios::by_id("review-loop").unwrap().stand_in_reviews[0].clone();
    let requirement = first
        .lines()
        .find(|line| line.starts_with("Requirement:"))
        .unwrap();
    assert!(requirement.contains("guide"), "{requirement}");
    assert!(!requirement.contains("Romanian"), "{requirement}");
    assert_eq!(
        scenarios::by_id("review-blockers")
            .unwrap()
            .stand_in_reviews[0],
        first
    );
}

/// The page a worker wrote for the stand-in to review: a minimal English translation of the
/// Romanian page, as a run of 2026-11-02 made it (`tests/fixtures/review-loop/README.md`).
const REFERENCE_PAGE: &str = include_str!("fixtures/review-loop/assessment.html");

/// A finding of the stand-in's reviews as a test of a page: the id of the finding, what it says,
/// and whether it is true of a page.
type Claim = (&'static str, &'static str, fn(&str) -> bool);

/// What each finding of the stand-in's reviews says of a page, as a test of the page.
fn claims() -> Vec<Claim> {
    // The part of a page between a tag and its closing tag.
    fn between<'a>(page: &'a str, open: &str, close: &str) -> &'a str {
        let (from, to) = (page.find(open), page.find(close));
        match (from, to) {
            (Some(from), Some(to)) if from < to => &page[from..to],
            _ => "",
        }
    }
    vec![
        // F1: the page gives the range the guide contradicts.
        ("F1", "", |page| {
            page.contains("100") && !page.contains("1 to 5")
        }),
        // F2: no <meta name="viewport">.
        ("F2", "viewport", |page| !page.contains("viewport")),
        // F3: the footer's <nav> has no aria-label.
        ("F3", "aria-label", |page| {
            let footer = between(page, "<footer>", "</footer>");
            footer.contains("<nav") && !footer.contains("aria-label")
        }),
        // F4: no header navigation.
        ("F4", "header navigation", |page| {
            !between(page, "<header>", "</header>").contains("<nav")
        }),
        // F5: no <meta name="description">.
        ("F5", "description", |page| {
            !page.contains("name=\"description\"")
        }),
        // F6: the <h1> is generic: it does not say whose assessment it is.
        ("F6", "<h1>", |page| {
            !between(page, "<h1>", "</h1>")
                .to_lowercase()
                .contains("team")
        }),
        // F7: no favicon link.
        ("F7", "favicon", |page| !page.contains("icon")),
    ]
}

#[test]
fn every_finding_of_the_stand_in_is_true_of_the_page_a_worker_wrote_and_none_is_vacuous() {
    let scenario = scenarios::by_id("review-loop").unwrap();
    let text = scenario.stand_in_reviews.join("\n");
    for (id, keyword, true_of) in claims() {
        assert!(text.contains(id), "{id} is in no review");
        assert!(
            true_of(REFERENCE_PAGE),
            "{id} is false of the page a worker wrote"
        );
        // And the review says of the id what the claim tests: each line of the reviews that
        // begins with it, in every review, is about it.
        for line in text
            .lines()
            .filter(|line| line.starts_with(&format!("{id} ")))
        {
            assert!(
                line.contains(keyword),
                "{id}'s line is not about {keyword}: {line}"
            );
        }
    }
    // The page the second review's F1 is fixed on: the range is gone from it, and F1 no longer holds.
    let fixed = REFERENCE_PAGE.replace("between 0 and 100", "on a scale from 1 to 5");
    assert!(!claims()[0].2(&fixed));
    // A claim that holds of every page says nothing: each fails on a page that has what it denies.
    let with = |from: &str, to: &str| REFERENCE_PAGE.replace(from, to);
    let pages = [
        with(
            "<meta charset=\"utf-8\">",
            "<meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">",
        ),
        with("<footer><nav>", "<footer><nav aria-label=\"More\">"),
        with("<header><h1>", "<header><nav>Home</nav><h1>"),
        with(
            "<meta charset=\"utf-8\">",
            "<meta charset=\"utf-8\"><meta name=\"description\" content=\"d\">",
        ),
        with("The Assessment", "Your team's assessment"),
        with(
            "<meta charset=\"utf-8\">",
            "<meta charset=\"utf-8\"><link rel=\"icon\" href=\"x.ico\">",
        ),
    ];
    for (page, (id, _, true_of)) in pages.iter().zip(claims().into_iter().skip(1)) {
        assert!(
            !true_of(page),
            "{id} holds of a page that has what it denies"
        );
    }
}

#[test]
fn the_owner_s_prompt_for_the_review_scenarios_says_nothing_of_the_stand_ins_notes() {
    // What the notes are about is what a reviewer finds: the owner asks for none of it.
    for id in ["review-loop", "review-blockers"] {
        let prompt = scenarios::by_id(id).unwrap().prompt.to_lowercase();
        for word in [
            "viewport",
            "aria",
            "description",
            "favicon",
            "navigație",
            "nav ",
        ] {
            assert!(!prompt.contains(word), "{id}: {word}");
        }
    }
}

/// The page of the fixture, `evals/fixtures/site/site/evaluare.html`.
fn romanian_page() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("evals/fixtures/site/site/evaluare.html"),
    )
    .unwrap()
}

#[test]
fn the_scripted_worker_writes_the_page_with_the_range_the_guide_contradicts_then_the_page_fixed_then_both_pages(
) {
    let loop_work = scenarios::by_id("review-loop").unwrap().stand_in_work;
    let blockers_work = scenarios::by_id("review-blockers").unwrap().stand_in_work;
    // The loop: the first answer has the bug, every later one has the page fixed (a reopen, a task
    // for the notes), and the Romanian page is never touched. The blockers: a third answer fixes
    // both pages, for the work that is about the range on both.
    assert_eq!(loop_work.len(), 2);
    assert_eq!(blockers_work.len(), 3);
    assert_eq!(loop_work[..], blockers_work[..2]);
    let page = |answer: &scenarios::ScriptedWork, path: &str| {
        answer
            .writes
            .iter()
            .find(|(written, _)| written == path)
            .map(|(_, text)| text.clone())
    };
    let (first, fixed, both) = (&loop_work[0], &loop_work[1], &blockers_work[2]);
    assert_eq!(first.writes.len(), 1);
    assert_eq!(fixed.writes.len(), 1);
    assert_eq!(both.writes.len(), 2);
    let english = |answer| page(answer, "site/en/assessment.html").unwrap();
    // F1 is true of the first and false of the fixed, and the tests of the claims say so of the page.
    assert_eq!(english(first), REFERENCE_PAGE);
    assert!(english(first).contains("between 0 and 100") && !english(first).contains("1 to 5"));
    assert!(!english(fixed).contains("100") && english(fixed).contains("scale from 1 to 5"));
    // The fixed pages are the first by one phrase, and the Romanian page the fixture's by one.
    assert_eq!(
        english(fixed),
        english(first).replace(
            "a score between 0 and 100",
            "a score on a scale from 1 to 5"
        )
    );
    assert_eq!(english(both), english(fixed));
    assert_eq!(
        page(both, "site/evaluare.html").unwrap(),
        romanian_page().replace("un scor între 0 și 100", "un scor pe o scară de la 1 la 5")
    );
    assert!(
        romanian_page().contains("între 0 și 100"),
        "the fixture still has the range the second blocker finds"
    );
    // Every answer says what it did and that it touched nothing else.
    for answer in [first, fixed, both] {
        assert!(
            answer.result.contains("site/en/assessment.html"),
            "{}",
            answer.result
        );
        assert!(
            answer.result.to_lowercase().contains("no other file"),
            "{}",
            answer.result
        );
    }
    // The first answer gives the page whole, as a worker's report does.
    assert!(first
        .result
        .contains("<title>Assessment — Burnout at Work</title>"));
}

#[test]
fn the_stand_in_reviews_are_said_of_the_pages_the_scripted_worker_writes() {
    // F1 of the first review is the first answer's page; F1 fixed of the second, the second's.
    let scenario = scenarios::by_id("review-blockers").unwrap();
    let first_page = &scenario.stand_in_work[0].writes[0].1;
    assert!(first_page.contains("100"));
    assert!(scenario.stand_in_reviews[0].contains("F1 [blocks]"));
    let fixed_page = &scenario.stand_in_work[1].writes[0].1;
    assert!(!fixed_page.contains("100"));
    assert!(scenario.stand_in_reviews[1].contains("F1 fixed."));
    // F6 of the second review: the Romanian page, which the second answer leaves alone.
    assert!(scenario.stand_in_work[1]
        .writes
        .iter()
        .all(|(path, _)| path != "site/evaluare.html"));
    assert!(scenario.stand_in_reviews[1].contains("site/evaluare.html line 11 still says"));
    // And the third answer fixes it, as the third review says.
    assert!(scenario.stand_in_work[2]
        .writes
        .iter()
        .any(|(path, _)| path == "site/evaluare.html"));
    assert!(scenario.stand_in_reviews[2].contains("F6 fixed."));
}
