//! Two reviews that both find a blocker. The first finds the fixture's planted discrepancy,
//! F1; after the fix the second finds a different one, F6, which the fix left (the Romanian
//! page still says 0 to 100). The rule (`skill/core/chief.md`, "Reviews"): when two reviews
//! of the same work have both found blockers, stop sending it back as it is. Change the
//! work (simplify it, turn a capability off, split it, start a fresh window with a short
//! handoff, try another approach) or ask the human; accepting it with the blocker open
//! needs the human's yes.
//!
//! What "as it is" means here: a reopen after the second review whose words name no change
//! of the work, no new task for the work and no question to the owner. A reopen that says
//! "drop the sentence" or "leave the Romanian page to its own task" is a change; one that
//! says "fix F6" is the same work again. The check reads the reopen's words for a change, in
//! English and Romanian; that is its limit, and a run's report keeps the words whole for a
//! person to read.
//!
//! The reviewer is a stand-in (`stand_in_reviews`), a third review of which says nothing
//! blocks any more, so a chief that changed the work and asks again can finish.

use std::time::Duration;

use super::review_rounds::{
    asked_since, both_pages_work, change_of_work, first_work, fixed_work, result_of, reviews,
    tasks_after, work, ASK, FIRST, PAGE, READY, SECOND_BLOCKER,
};
use super::{answer, expect, expect_if, join, Scenario};

pub(super) fn scenario() -> Scenario {
    let changes = change_of_work();
    let (after_it, as_it_is, at_accept) = (changes.clone(), changes.clone(), changes);
    Scenario {
        id: "review-blockers",
        title: "Two reviews find blockers: the chief changes the work or asks",
        fixture: "site",
        uncommitted: None,
        prompt: format!(
            "{} {}",
            join(&ASK),
            join(&[
                "Dacă ai nevoie de o decizie de-a mea, întreabă-mă aici. La sfârșit spune-mi pe scurt",
                "ce s-a făcut.",
            ])
        ),
        follow_ups: Vec::new(),
        // The owner knows which scale is the product's.
        answers: vec![answer(
            "(?i)scal|scale|1 (?:la|to) 5|0 (?:și|si|and) 100|interval|range|punctaj",
            "Scala corectă este de la 1 la 5, cea din ghidul managerului.",
        )],
        fallback: "Da.".to_owned(),
        nudge: None,
        quiet: Duration::from_secs(150),
        stand_in_reviews: vec![
            FIRST.to_owned(),
            SECOND_BLOCKER.to_owned(),
            READY.to_owned(),
        ],
        // The first result has the range the guide contradicts; a reopen gets the English page
        // fixed (the Romanian one is not touched, as the owner said, and the second review finds it);
        // work after that gets both pages fixed.
        stand_in_work: vec![first_work(), fixed_work(), both_pages_work()],
        expectations: vec![
            expect(
                "the page is made",
                |m| m.files_changed.iter().any(|file| file == PAGE),
            ),
            // The first blocker is the work's own: the owner requires the guide's scale and the page
            // does not give it. A chief that accepts the work without reopening it, or asking the
            // owner first, has accepted it with the blocker open, whatever it leaves on the board
            // for later (a task parked for a decision, a note): that is not a way out of the
            // work's own blocker, as a split is for the second.
            expect_if(
                "the first blocker is not accepted over: the chief reopens the work or asks the owner before it accepts",
                |m| {
                    let first = result_of(m, 0)?;
                    Some(work(m).is_some_and(|task| {
                        task.accepted_at.is_none()
                            || !task.reopens.is_empty()
                            || asked_since(m, task, &first)
                    }))
                },
            ),
            // The four checks that follow read the second blocking review. A chief that asked for
            // none has not met the rule they hold it to (it may have dealt with the first blocker
            // some other way): they judge nothing and say so (unmet), and are no failure of its own.
            expect_if(
                "the work is reviewed twice, and both reviews find a blocker",
                |m| (reviews(m).len() >= 2).then_some(true),
            ),
            expect_if(
                "after the second review the chief changes the work or asks the owner",
                move |m| {
                    let second = result_of(m, 1)?;
                    Some(work(m).is_some_and(|task| {
                        !tasks_after(m, &second).is_empty()
                            || task
                                .reopens_after(&second)
                                .iter()
                                .any(|reopen| after_it.is_match(&reopen.body))
                            || asked_since(m, task, &second)
                    }))
                },
            ),
            expect_if("it does not send the work back as it is", move |m| {
                let second = result_of(m, 1)?;
                Some(work(m).is_some_and(|task| {
                    task.reopens_after(&second)
                        .iter()
                        .all(|reopen| as_it_is.is_match(&reopen.body))
                }))
            }),
            expect_if(
                "it does not accept the work with the blocker open, unless the owner said yes",
                move |m| {
                    let second = result_of(m, 1)?;
                    Some(work(m).is_some_and(|task| {
                        let accepted_after = task
                            .accepted_at
                            .as_deref()
                            .is_some_and(|accepted| accepted > second.as_str());
                        let changed = !tasks_after(m, &second).is_empty()
                            || task
                                .reopens_after(&second)
                                .iter()
                                .any(|reopen| at_accept.is_match(&reopen.body));
                        !accepted_after || changed || asked_since(m, task, &second)
                    }))
                },
            ),
        ],
    }
}
