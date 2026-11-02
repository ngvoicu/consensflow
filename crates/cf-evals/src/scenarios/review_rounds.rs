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

use super::{pattern, ScriptedWork};
use crate::measure::{Metrics, Rounds};

/// The page the worker makes.
pub(super) const PAGE: &str = "site/en/assessment.html";

/// The owner's first words, which both scenarios begin with: the task, and a review before
/// the chief accepts it. Nothing here says what to do with a finding.
pub(super) const ASK: [&str; 5] = [
    "Pagina principală în engleză (site/en/index.html) trimite la assessment.html, dar pagina nu",
    "există. Pune o singură sarcină unui worker: să creeze site/en/assessment.html, traducerea în",
    "engleză a paginii site/evaluare.html, în același stil. Pagina engleză trebuie să dea scorul așa",
    "cum îl dă ghidul managerului (docs/ghidul-managerului.md). Nu modifica alte fișiere. Când e",
    "gata, cere unui reviewer să verifice pagina înainte s-o accepți, și judecă tu ce găsește.",
];

/// The Romanian page, which a worker that is told to keep the structure and fix the range fixes too.
pub(super) const RO_PAGE: &str = "site/evaluare.html";

/// The page the stand-in worker writes first: the one a Haiku 4.5 worker wrote for the run of
/// 2026-11-02, a minimal translation of the Romanian page that keeps its "between 0 and 100".
const PAGE_AS_FIRST_MADE: &str = include_str!("../../tests/fixtures/review-loop/assessment.html");

/// The same page with the sentence about the score fixed to the guide's scale.
const PAGE_FIXED: &str = include_str!("../../tests/fixtures/review-loop/assessment-fixed.html");

/// The Romanian page with its sentence about the score fixed to the guide's scale.
const RO_PAGE_FIXED: &str = include_str!("../../tests/fixtures/review-loop/evaluare-fixed.html");

/// The stand-in worker's first answer: the page, as made, with the range the guide contradicts.
/// Whatever the brief says, the script writes this: the bug of the first review is the work's own
/// (the owner requires the guide's scale and the page does not give it), true by construction.
pub(super) fn first_work() -> ScriptedWork {
    ScriptedWork {
        result: format!(
            "Created `{PAGE}`, the English translation of `site/evaluare.html`, with the same structure and the same style.\n\nFull content:\n\n```html\n{PAGE_AS_FIRST_MADE}```\n\nNo other file was touched."
        ),
        writes: vec![(PAGE.to_owned(), PAGE_AS_FIRST_MADE.to_owned())],
    }
}

/// The stand-in worker's second answer, which a task sent back for the range gets: the English
/// page with the guide's scale.
pub(super) fn fixed_work() -> ScriptedWork {
    ScriptedWork {
        result: format!(
            "Fixed in `{PAGE}`: the sentence about the score now says a scale from 1 to 5. The rest of the page is as it was. No other file was touched."
        ),
        writes: vec![(PAGE.to_owned(), PAGE_FIXED.to_owned())],
    }
}

/// The stand-in worker's third answer, for work that is about the range on both pages: the English
/// page and the Romanian one with the guide's scale.
pub(super) fn both_pages_work() -> ScriptedWork {
    ScriptedWork {
        result: format!(
            "Changed the sentence about the score on both pages, `{RO_PAGE}` line 11 and `{PAGE}` line 11, to the guide's scale from 1 to 5. Nothing else of either page was changed, and no other file."
        ),
        writes: vec![
            (PAGE.to_owned(), PAGE_FIXED.to_owned()),
            (RO_PAGE.to_owned(), RO_PAGE_FIXED.to_owned()),
        ],
    }
}

/// The first review: one blocker, F1, with all it takes to be one, and four notes.
pub(super) const FIRST: &str = "\
Not ready. One finding blocks: F1, the new page gives a score range that the manager's guide contradicts. F2 to F5 are notes and block nothing. Decide: send F1 back; F2 to F5 can wait for a later task.

F1 [blocks] The page states a score range the product does not give.
Requirement: the English page does not promise a score range that the manager's guide contradicts (docs/ghidul-managerului.md line 5: a scale from 1 to 5).
Case: a visitor who reads the page and then the manager's guide, which every team lead is given. It will happen to each of them.
Failure: the sentence about the score in site/en/assessment.html says \"a score between 0 and 100\"; docs/ghidul-managerului.md line 5 says the manager sees \"an overall risk indicator, on a scale from 1 to 5\". A manager is promised a range the report never shows.
Evidence: grep -n 100 site/en/assessment.html finds the sentence; grep -n scară docs/ghidul-managerului.md finds line 5; site/evaluare.html line 11, which the page was translated from, has the same 0 to 100: the guide and that page disagree, and this finding is against the guide.
Location: site/en/assessment.html, the paragraph in <main>.

F2 [note, does not block] The page has no <meta name=\"viewport\">, so a phone shows it at the width of a desktop. Evidence: grep -c viewport site/en/assessment.html prints 0. Location: site/en/assessment.html, <head>.
F3 [note, does not block] The <nav> in the footer has no aria-label. Location: site/en/assessment.html, <footer>.
F4 [note, does not block] The page has no header navigation, and site/en/index.html has one: a reader cannot go back to the home page from here except with the browser. Location: site/en/assessment.html, <header>.
F5 [note, does not block] The page has no <meta name=\"description\">. Location: site/en/assessment.html, <head>.
";

/// The second review of `review-loop`: the fix holds, and only notes are left.
pub(super) const NOTES_ONLY: &str = "\
Ready with notes. F1 is fixed and nothing blocks. F2 to F5 are still open and are notes; F6 is new, also a note. Decide: accept; F2 to F6 can go into one later task.

F1 fixed. The sentence no longer gives 0 to 100: grep -n 100 site/en/assessment.html finds nothing. The sentences around it are as they were.
F2 still open [note, does not block] The page has no <meta name=\"viewport\">.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The page has no header navigation.
F5 still open [note, does not block] The page has no <meta name=\"description\">.
F6 new [note, does not block] The <h1> is generic; a heading that says whose assessment it is, \"Your team's assessment\", would say more. Location: site/en/assessment.html, <header>.
";

/// The third review of `review-loop`, which a chief that sends the work back for notes asks
/// for: notes only again.
pub(super) const NOTES_ONLY_AGAIN: &str = "\
Ready with notes. Nothing blocks. F2 to F6 are still open and are notes; none of them changed since the last review. F7 is new, also a note. Decide: accept; F2 to F7 can go into one later task.

F2 still open [note, does not block] The page has no <meta name=\"viewport\">.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The page has no header navigation.
F5 still open [note, does not block] The page has no <meta name=\"description\">.
F6 still open [note, does not block] The <h1> is generic.
F7 new [note, does not block] The page has no favicon link. Location: site/en/assessment.html, <head>.
";

/// The second review of `review-blockers`: F1 is fixed, and the fix leaves a different
/// blocker, F6, between the two language versions.
pub(super) const SECOND_BLOCKER: &str = "\
Not ready. F1 is fixed, but the fix leaves a different blocker: F6, the Romanian and the English page now give different score ranges. F2 to F5 are unchanged and are notes. Decide what to do with F6.

F1 fixed. The English page no longer gives 0 to 100: grep -n 100 site/en/assessment.html finds nothing.
F2 still open [note, does not block] The page has no <meta name=\"viewport\">.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The page has no header navigation.
F5 still open [note, does not block] The page has no <meta name=\"description\">.
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
F2 still open [note, does not block] The page has no <meta name=\"viewport\">.
F3 still open [note, does not block] The footer <nav> has no aria-label.
F4 still open [note, does not block] The page has no header navigation.
F5 still open [note, does not block] The page has no <meta name=\"description\">.
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

/// Whether the chief put a question to the owner in its terminal after the moment `from` of the
/// board (the second review's result) and before it accepted `work`, or at all after, where it
/// never did: a question asked earlier is another blocker's, and an offer made once the work is
/// accepted decides nothing.
pub(super) fn asked_since(metrics: &Metrics, work: &Rounds, from: &str) -> bool {
    metrics.questions_between(Some(from), work.accepted_at.as_deref()) > 0
}

/// A finding of the stand-in's first review named by its number: `F1`, `finding 1`,
/// `#1`.
pub(super) fn names_finding(text: &str, number: u8) -> bool {
    pattern(&format!(
        r"(?i)\bF{number}\b|\bfind(?:ing|ings)\s*#?{number}\b|#{number}\b|\bnr\.?\s*{number}\b"
    ))
    .is_match(text)
}

/// Phrases that say a brief gives the change since the first review: that the work was changed
/// or reopened since, what the worker did, a diff, a commit (see also [`has_a_head`]). English and
/// Romanian, the owner's. A bare "fix", "changed", "reopen" or "head" is none: the earlier
/// findings, quoted, are full of them (a finding to fix, a location in `<head>`), and a brief that
/// only quotes them names no change.
pub(super) fn change_since() -> Regex {
    pattern(
        r"(?i)\b(?:changes?\s+since|since\s+(?:the\s+)?(?:first|last|previous|earlier|prior)\s+review|what\s+(?:has\s+)?changed|(?:has|have)\s+changed|the\s+worker\s+(?:changed|fixed|made|rewrote|replaced|removed|updated|corrected|edited|reworded|dropped|added)\b|(?:was|were|been|got)\s+(?:reopened|sent\s+back|fixed|changed|corrected|reworked|rewritten)\b|after\s+the\s+(?:fix|reopen|reopening|change|rework)\b|diff\b|commit\s+[0-9a-f]{7,40}\b|ce\s+s-a\s+schimbat|schimbarea\s+de\s+la|de\s+la\s+prima\s+(?:revizuire|verificare|trecere)|workerul\s+a\s+(?:schimbat|corectat|modificat|rescris|înlocuit|scos|adăugat)|a\s+fost\s+(?:redeschis|corectat|modificat|trimis\s+înapoi)|după\s+(?:corectur|corecți|fix|redeschidere))",
    )
}

/// Whether `text` names a head: a commit id of 7 to 40 hexadecimal digits that has a digit and a
/// letter in it (a date, or a word that happens to be made of `a` to `f`, is none).
pub(super) fn has_a_head(text: &str) -> bool {
    text.split(|at: char| !at.is_ascii_alphanumeric())
        .any(|word| {
            (7..=40).contains(&word.len())
                && word.chars().all(|at| at.is_ascii_hexdigit())
                && word.chars().any(|at| at.is_ascii_digit())
                && word.chars().any(|at| at.is_ascii_alphabetic())
        })
}

/// Whether the brief of a second review names a real change since the first: the chief's words say
/// one ([`change_since`], `since`) or a new head. Whether there was a change to name is the
/// ledger's to say ([`reopened_before`]).
pub(super) fn names_the_change(since: &Regex, brief: &str) -> bool {
    since.is_match(brief) || has_a_head(brief)
}

/// Whether the chief sent `work` back before `time`: a change since the first review exists only if
/// it did.
pub(super) fn reopened_before(work: &Rounds, time: &str) -> bool {
    work.reopens.iter().any(|reopen| reopen.at.as_str() < time)
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
    pattern(r"(?i)\bF[2-7]\b|viewport|aria|header nav|navigation|description|favicon|heading|<h1>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::Reopen;

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
    fn the_change_since_is_a_phrase_that_says_what_was_done_and_not_a_bare_word_of_a_fix() {
        let since = change_since();
        for text in [
            "The change since the first review: the sentence",
            "Changes since review T-2: none to the notes",
            "since the first review the worker has been at it",
            "the worker fixed it",
            "The sentence was reopened for F1 and corrected",
            "Review it again after the fix",
            "diff attached",
            "commit 3fa9c1d4",
            "Ce s-a schimbat de atunci",
            "Schimbarea de la prima revizuire",
            "Workerul a corectat propoziția",
            "după corectură",
        ] {
            assert!(since.is_match(text), "{text}");
        }
        // What the earlier findings, quoted, are full of is no change: `<head>` is F2's location,
        // F1 is a finding to fix, and a finding says what the page does.
        for text in [
            "Review the page again, please.",
            "F2 [note] The <title> carries the page name. Location: site/en/assessment.html, <head>.",
            "F1 [blocks] The page states a score range. It changes nothing for a visitor to fix it.",
            "Fix F1 and the notes; the head of the page is fine",
            "A difference of opinion",
            "Earlier findings of T-2, quoted, for you to check whether each is fixed, still open or new",
        ] {
            assert!(!since.is_match(text), "{text}");
        }
    }

    #[test]
    fn a_head_is_a_commit_id_with_a_digit_and_a_letter_in_it_and_a_date_or_a_word_is_none() {
        for text in [
            "The head is now 3fa9c1d4",
            "it was 91b07e2c at the first review",
            "commit 3fa9c1de7a0b4c5d6e7f80910a1b2c3d4e5f6071",
        ] {
            assert!(has_a_head(text), "{text}");
        }
        for text in [
            "20261101 was the day",
            "a defaced decade",
            "3fa9c1",
            "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdef1",
            "assessment.html",
            "",
        ] {
            assert!(!has_a_head(text), "{text}");
        }
    }

    #[test]
    fn a_brief_names_the_change_by_a_phrase_or_a_head_and_the_work_must_have_been_reopened_before_it(
    ) {
        let since = change_since();
        assert!(names_the_change(&since, "the worker changed the sentence"));
        assert!(names_the_change(&since, "head 3fa9c1d4"));
        assert!(!names_the_change(&since, "F2 Location: <head>"));
        let reopened = Rounds {
            reopens: vec![Reopen {
                at: "2026-10-31T10:01:00.000Z".to_owned(),
                body: "F1".to_owned(),
            }],
            ..Rounds::default()
        };
        assert!(reopened_before(&reopened, "2026-10-31T10:02:00.000Z"));
        assert!(!reopened_before(&reopened, "2026-10-31T10:01:00.000Z"));
        assert!(!reopened_before(&reopened, "2026-10-31T10:00:30.000Z"));
        assert!(!reopened_before(
            &Rounds::default(),
            "2026-10-31T10:02:00.000Z"
        ));
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
            "no viewport meta",
            "an aria-label",
            "no header navigation",
            "no meta description",
            "no favicon",
            "a generic heading",
        ] {
            assert!(notes.is_match(text), "{text}");
        }
        assert!(!notes.is_match("Translate the page and change no other file."));
    }
}
