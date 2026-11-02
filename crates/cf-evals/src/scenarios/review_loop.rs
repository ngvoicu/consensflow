//! The review that must not loop. The first review of a small piece of work finds one
//! real bug and four notes; after the fix, the second finds notes only; a third, which only
//! a chief that sends the work back for notes would ask for, finds notes only again. The
//! rule (`skill/core/chief.md`, "Reviews"): reopen for the blocker, give the second review
//! the earlier findings by number and the change since, and accept once only notes are left,
//! with one later task or a note for them. On poker-lab's board 211 of 224 reviews said FIX
//! FIRST, and one runbook was reviewed from round 31 to round 45 (T-1008).
//!
//! The reviewer is a stand-in (`stand_in_reviews`): the findings are the same whoever
//! the chief is, and what is measured is what it does with them.

use std::time::Duration;

use super::review_rounds::{
    change_since, first_work, fixed_work, names_finding, names_the_change, notes, reopened_before,
    result_of, reviews, tasks_after, work, ASK, FIRST, NOTES_ONLY, NOTES_ONLY_AGAIN, PAGE,
};
use super::{expect, expect_if, join, pattern, Scenario};

pub(super) fn scenario() -> Scenario {
    let bug = pattern(r"(?i)\bF1\b|score|scale|scor|scară|scara|100|1 (?:to|la|-|–) 5");
    let nits = notes();
    let since = change_since();
    Scenario {
        id: "review-loop",
        title: "A review finds a bug and notes: reopen once, accept on the notes",
        fixture: "site",
        uncommitted: None,
        prompt: format!(
            "{} {}",
            join(&ASK),
            join(&[
                "Nu mă întreba pe mine nimic. Dacă rămân lucruri care merită făcute mai târziu,",
                "lasă-le pe board. La sfârșit spune-mi pe scurt ce s-a făcut.",
            ])
        ),
        follow_ups: Vec::new(),
        answers: Vec::new(),
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(150),
        stand_in_reviews: vec![
            FIRST.to_owned(),
            NOTES_ONLY.to_owned(),
            NOTES_ONLY_AGAIN.to_owned(),
        ],
        // The first result has the range the guide contradicts, the owner's requirement unmet;
        // every later answer (a reopen, a task for the notes) is the page fixed.
        stand_in_work: vec![first_work(), fixed_work()],
        expectations: vec![
            expect("the page is made, and no other file changes", |m| {
                m.files_changed == [PAGE]
            }),
            // The checks that read the second review are judged only if the chief asked for one:
            // a chief that did not has not met the rule they hold it to, and they say so (unmet),
            // which is no failure of its own.
            expect_if(
                "the work is reviewed again after the fix: two reviews at least",
                |m| (reviews(m).len() >= 2).then_some(true),
            ),
            expect("the work is reopened once, for the bug", move |m| {
                work(m).is_some_and(|task| {
                    task.reopens.len() == 1 && bug.is_match(&task.reopens[0].body)
                })
            }),
            expect_if(
                "the work is never sent back after the review that found notes only",
                |m| {
                    let notes_only = result_of(m, 1)?;
                    Some(work(m).is_some_and(|task| task.reopens_after(&notes_only).is_empty()))
                },
            ),
            expect_if(
                "the work is accepted once only notes are left, not before",
                |m| {
                    let notes_only = result_of(m, 1)?;
                    Some(work(m).is_some_and(|task| {
                        task.accepted_at
                            .as_deref()
                            .is_some_and(|accepted| accepted > notes_only.as_str())
                    }))
                },
            ),
            expect_if(
                "the notes go into one later task or a note, not into many tasks",
                move |m| {
                    let notes_only = result_of(m, 1)?;
                    let later = tasks_after(m, &notes_only);
                    Some(
                        later.len() <= 1
                            && (later.iter().any(|task| nits.is_match(&task.brief))
                                || nits.is_match(&m.notes_text)),
                    )
                },
            ),
            expect_if(
                "the second review's brief names the earlier findings by number and the change since",
                move |m| {
                    let review = reviews(m).get(1).copied()?;
                    Some(work(m).is_some_and(|task| {
                        reopened_before(task, &review.created_at)
                            && names_finding(&review.brief, 1)
                            && names_the_change(&since, &review.brief)
                    }))
                },
            ),
            expect(
                "the owner is not asked anything before the work is accepted",
                |m| m.questions_between(None, work(m).and_then(|task| task.accepted_at.as_deref())) == 0,
            ),
        ],
    }
}
