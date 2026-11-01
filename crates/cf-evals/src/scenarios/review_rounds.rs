//! What the two review scenarios (`review-loop`, `review-blockers`) share: the task the
//! owner gives, the stand-in reviewer's reviews, and how a chief's rounds are read.
//!
//! The rule they hold the chief to is the one `skill/core/chief.md` gives in "Reviews"
//! (spec `smaller-contexts`, SCX-06, SCX-17): a finding blocks the work only when it is
//! evidenced; a second review checks the fixes, named by their numbers; the rest does not
//! hold the work; and when two reviews of the same work have both found blockers, the
//! chief stops sending it back as it is and changes the work or asks the human.
//!
//! **The task.** A worker translates `site/evaluare.html` into the English page the
//! English home already links, `site/en/assessment.html`. A faithful translation keeps the
//! sentence "un scor între 0 și 100", which the manager's guide
//! (`docs/ghidul-managerului.md`, line 5: a scale from 1 to 5) contradicts: the fixture's
//! planted discrepancy, so the bug the stand-in reports is true of the first result
//! whoever writes it. Fixing the English page alone leaves the Romanian one saying 0 to 100,
//! which is the second blocker of `review-blockers`.
//!
//! **The stand-in.** The reviews are text, in the form `skill/core/staff.md` asks of a
//! reviewer: ready or not first, each finding numbered (`F1`…) with whether it blocks, a
//! blocker with its requirement, case, failure and evidence, and on a later review each
//! earlier finding fixed, still open, fixed in part or newly broken. A finding's number is
//! distinctive (`F2`, never a bare `2.`) so that a brief that names it can be found.

use regex::Regex;

use super::pattern;
use crate::measure::{Metrics, Rounds};

/// The page the worker makes.
pub(super) const PAGE: &str = "site/en/assessment.html";

/// The owner's first words, which both scenarios begin with: the task, and a review before
/// the chief accepts it. Nothing here says what to do with a finding.
pub(super) const ASK: [&str; 4] = [
    "Pagina principală în engleză (site/en/index.html) trimite la assessment.html, dar pagina nu",
    "există. Pune o singură sarcină unui worker: să creeze site/en/assessment.html, traducerea în",
    "engleză a paginii site/evaluare.html, în același stil. Nu modifica alte fișiere. Când e gata,",
    "cere unui reviewer să verifice pagina înainte s-o accepți, și judecă tu ce găsește.",
];

/// The first review: one blocker, F1, with all it takes to be one, and four notes.
pub(super) const FIRST: &str = "\
Not ready. One finding blocks: F1, the new page gives a score range that the manager's guide contradicts. F2 to F5 are notes and block nothing. Decide: send F1 back; F2 to F5 can wait for a later task.

F1 [blocks] The page states a score range the product does not give.
Requirement: the English page says of the result what the Romanian page and the manager's guide say.
Case: a visitor who reads the page and then the manager's guide, which every team lead is given. It will happen to each of them.
Failure: the sentence about the score in site/en/assessment.html says \"a score between 0 and 100\"; docs/ghidul-managerului.md line 5 says the manager sees \"an overall risk indicator, on a scale from 1 to 5\". A manager is promised a range the report never shows.
Evidence: grep -n 100 site/en/assessment.html finds the sentence; grep -n scară docs/ghidul-managerului.md finds line 5; site/evaluare.html line 11 has the same 0 to 100, which is where it was copied from.
Location: site/en/assessment.html, the paragraph in <main>.

F2 [note, does not block] The <title> carries the page name and not the site name; \"Assessment — Burnout at Work\" would match the other pages. Location: site/en/assessment.html, <head>.
F3 [note, does not block] The <nav> in the footer has no aria-label. Location: site/en/assessment.html, <footer>.
F4 [note, does not block] The one long sentence reads better as two. Location: site/en/assessment.html, <main>.
F5 [note, does not block] The four colours are listed in the Romanian order; English readers expect red first. Location: site/en/assessment.html, <main>.
";

/// The second review of `review-loop`: the fix holds, and only notes are left.
pub(super) const NOTES_ONLY: &str = "\
Ready with notes. F1 is fixed and nothing blocks. F2 to F5 are still open and are notes; F6 is new, also a note. Decide: accept; F2 to F6 can go into one later task.

F1 fixed. The sentence no longer gives 0 to 100: grep -n 100 site/en/assessment.html finds nothing. The sentences around it are as they were.
F2 still open [note, does not block] The <title> does not carry the site name.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The long sentence reads better as two.
F5 still open [note, does not block] The colours are in the Romanian order.
F6 new [note, does not block] The page has no <meta name=\"description\">. Location: site/en/assessment.html, <head>.
";

/// The third review of `review-loop`, which a chief that sends the work back for notes asks
/// for: notes only again.
pub(super) const NOTES_ONLY_AGAIN: &str = "\
Ready with notes. Nothing blocks. F2 to F6 are still open and are notes; none of them changed since the last review. F7 is new, also a note. Decide: accept; F2 to F7 can go into one later task.

F2 still open [note, does not block] The <title> does not carry the site name.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The long sentence reads better as two.
F5 still open [note, does not block] The colours are in the Romanian order.
F6 still open [note, does not block] The page has no <meta name=\"description\">.
F7 new [note, does not block] The page has no favicon link. Location: site/en/assessment.html, <head>.
";

/// The second review of `review-blockers`: F1 is fixed, and the fix leaves a different
/// blocker, F6, between the two language versions.
pub(super) const SECOND_BLOCKER: &str = "\
Not ready. F1 is fixed, but the fix leaves a different blocker: F6, the Romanian and the English page now give different score ranges. F2 to F5 are unchanged and are notes. Decide what to do with F6.

F1 fixed. The English page no longer gives 0 to 100: grep -n 100 site/en/assessment.html finds nothing.
F2 still open [note, does not block] The <title> does not carry the site name.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The long sentence reads better as two.
F5 still open [note, does not block] The colours are in the Romanian order.
F6 new [blocks, caused by the fix to F1] The two language versions of the assessment page disagree about the score.
Requirement: the Romanian and the English page of the assessment say the same of the result.
Case: a team with Romanian-speaking and English-speaking members, whose lead reads the page in each language. It will happen to every such team.
Failure: site/evaluare.html line 11 still says \"un scor între 0 și 100\" while the English page now gives the guide's scale; the same site tells one team two ranges.
Evidence: grep -n 100 site/evaluare.html finds line 11; the English page has no 100 any more.
Location: site/evaluare.html line 11, against the sentence about the score in site/en/assessment.html.
";

/// The third review of `review-blockers`, which a chief that changed the work and asked
/// again gets: nothing blocks any more.
pub(super) const READY: &str = "\
Ready with notes. Nothing blocks. F6 no longer holds: the two versions agree, or the sentence is gone from one of them. F2 to F5 are unchanged and are notes. Decide: accept.

F6 fixed. grep -n 100 site/evaluare.html site/en/assessment.html finds no range that differs between the two.
F2 still open [note, does not block] The <title> does not carry the site name.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The long sentence reads better as two.
F5 still open [note, does not block] The colours are in the Romanian order.
";

/// The rounds of the chief's tasks, or none where the measure did not read them.
pub(super) fn rounds(metrics: &Metrics) -> &[Rounds] {
    metrics.rounds.as_deref().unwrap_or_default()
}

/// The work the chief gave a worker: its first task that is no review, advice or test.
pub(super) fn work(metrics: &Metrics) -> Option<&Rounds> {
    rounds(metrics).iter().find(|task| task.is_work())
}

/// The reviews the chief asked for that came back, in the order it asked for them.
pub(super) fn reviews(metrics: &Metrics) -> Vec<&Rounds> {
    rounds(metrics)
        .iter()
        .filter(|task| task.is_review() && task.first_result().is_some())
        .collect()
}

/// When the result of the `nth` review (the first is 0) came back.
pub(super) fn result_of(metrics: &Metrics, nth: usize) -> Option<String> {
    reviews(metrics)
        .get(nth)
        .and_then(|review| review.first_result())
        .map(str::to_owned)
}

/// The tasks of work the chief asked for after `time`, besides the work itself.
pub(super) fn tasks_after<'a>(metrics: &'a Metrics, time: &str) -> Vec<&'a Rounds> {
    let work = work(metrics).map(|task| task.number);
    rounds(metrics)
        .iter()
        .filter(|task| {
            task.is_work() && Some(task.number) != work && task.created_at.as_str() > time
        })
        .collect()
}

/// A finding of the stand-in's first review named by its number: `F1`, `finding 1`,
/// `#1`.
pub(super) fn names_finding(text: &str, number: u8) -> bool {
    pattern(&format!(
        r"(?i)\bF{number}\b|\bfind(?:ing|ings)\s*#?{number}\b|#{number}\b|\bnr\.?\s*{number}\b"
    ))
    .is_match(text)
}

/// Words that say a brief gives the change since the first review: a fix, a diff, a
/// commit, what the worker did. English and Romanian, the owner's.
pub(super) fn change_since() -> Regex {
    pattern(
        r"(?i)\b(?:since|changed?|changes|diff|fix(?:ed|es)?|commit|head|reopen(?:ed)?|rework(?:ed)?|updated|schimb\w*|modific\w*|corect\w*|repar\w*|după|dupa)\b",
    )
}

/// Words that say the chief changes the work, and does not send it back as it is: it
/// simplifies it, drops or turns off a capability, splits it, leaves a part out, starts
/// again in a fresh window from a handoff or takes another approach. English and Romanian.
/// A reopen that says none of them asks for the same work again, and "fix", "remove the
/// 100" and "instead of" are no change of the work: they are the fix of the finding. This
/// is a check of words and its limit: a run's report keeps the reopen's words whole.
pub(super) fn change_of_work() -> Regex {
    pattern(
        r"(?i)\b(?:simplif\w*|drop(?:s|ped|ping)?\s+(?:the|this|that|it|them|all)|(?:turn|switch)(?:s|ed|ing)?\s+(?:\w+\s+){0,3}?off|disabl\w*|split(?:s|ting)?|(?:separate|own)\s+(?:task|window)|out\s+of\s+scope|leav\w+\s+(?:\w+\s+){0,2}?out|left\s+out|take\s+out|(?:different|another|new|simpler)\s+approach|fresh\s+(?:window|start|session)|hand-?off|descop\w*|elimin\w*|scoate\w*|renun\w*|dezactiv\w*|împăr\w*|sarcin[ăa]\s+separat[ăa]|(?:într-o|intr-o)\s+sarcin[ăa])\b",
    )
}

/// Words of the stand-in's notes, which a task or a note that gathers them names.
pub(super) fn notes() -> Regex {
    pattern(
        r"(?i)\bF[2-7]\b|<title>|\btitle\b|aria|site name|long sentence|two sentences|colou?rs?|description",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finding_is_named_by_its_number_in_the_forms_a_chief_writes_it_and_not_by_a_longer_number()
    {
        for text in [
            "F1 blocks",
            "findings F1–F5",
            "finding 1",
            "Finding #1",
            "see #1, the score",
            "nr. 1",
        ] {
            assert!(names_finding(text, 1), "{text}");
        }
        for text in [
            "F10",
            "finding 12",
            "#11",
            "a scale from 1 to 5",
            "F2 to F5",
            "Review it again",
        ] {
            assert!(!names_finding(text, 1), "{text}");
        }
        assert!(names_finding("F2 to F5 are notes", 2) && names_finding("F2 to F5 are notes", 5));
    }

    #[test]
    fn the_change_since_is_a_word_of_a_fix_a_diff_or_a_commit_in_english_or_romanian() {
        let since = change_since();
        for text in [
            "The change since the first review: the sentence",
            "the worker fixed it",
            "diff attached",
            "commit 3fa9c1",
            "Modificarea de la prima revizuire",
            "Ce s-a schimbat de atunci",
            "după corectură",
        ] {
            assert!(since.is_match(text), "{text}");
        }
        assert!(!since.is_match("Review the page again, please."));
    }

    #[test]
    fn a_change_of_the_work_is_named_by_a_word_of_it_and_a_fix_of_the_same_work_is_none() {
        let change = change_of_work();
        for text in [
            "Drop the sentence about the score from the English page.",
            "Simplify the page: no score range.",
            "Turn off the range for now.",
            "Turn the range off for now.",
            "Split this: the Romanian page is its own task.",
            "The Romanian page gets its own task.",
            "Leave out the range.",
            "Start a fresh window with this handoff.",
            "Try another approach.",
            "Eliminați propoziția despre scor.",
            "Scoate fraza despre scor.",
            "Pagina în română se face într-o sarcină separată.",
        ] {
            assert!(change.is_match(text), "{text}");
        }
        for text in [
            "Fix F6.",
            "Fix F6 as well: make site/evaluare.html agree with the English page.",
            "Make the Romanian page say 1 to 5.",
            "Make the English page say 1 to 5 instead of 0 to 100.",
            "Remove the 100 from site/evaluare.html, as the guide says.",
            "Corectează și pagina în română, fără să modifici alt fișier.",
        ] {
            assert!(!change.is_match(text), "{text}");
        }
    }

    #[test]
    fn the_notes_are_found_by_their_numbers_or_by_what_they_are_about() {
        let notes = notes();
        for text in [
            "F2 to F6",
            "the <title> without the site name",
            "an aria-label",
            "the colour order",
        ] {
            assert!(notes.is_match(text), "{text}");
        }
        assert!(!notes.is_match("Translate the page and change no other file."));
    }
}
