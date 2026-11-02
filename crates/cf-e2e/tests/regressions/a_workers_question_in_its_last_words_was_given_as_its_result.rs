//! A worker's question in the last words of its turn was given as its result.
//!
//! Seen on another board of the owner's, 2026-11-02, version unknown (T-1119). Two workers
//! ended their turns with a question in their own words and did not ask with `cf ask`:
//! "Should I continue with the live A/B runs and the packet/socket capture, or stop here so
//! you can review the findings first?" (m-383 of that board, T-3) and "Question for you: has
//! the Build Tools 2022 install … finished? … I'm waiting for your answer." (m-387, T-6).
//! Since 3.0.7-alpha.1 a turn's end is the task's result, so both reached the chief as
//! results, "Decide with: cf task accept | cf task reopen", which `cf answer` cannot reach;
//! T-6 showed done, though it had done none of its work, and an accept of it would have
//! closed that work and released the tasks that wait for it.
//!
//! What the owner should have seen: each as the worker's question, which `cf answer` answers,
//! with the task waiting, not done; the answer reaching the worker, whose reply is then the
//! result. A report that ends with an open question about its product is still a result.
//!
//! The worker is the stand-in, whose `SAY` line answers the words it is given as its last
//! words, and the chief is the stand-in too, which runs `cf answer` as the case tells it.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// The first report's words: the work half done, and a choice offered.
const SHOULD_I_CONTINUE: &str = "Should I continue with the live A/B runs and the packet/socket capture, or stop here so you can review the findings first?";

/// The second report's words: nothing done, and a wait.
const WAITING_FOR_THE_INSTALL: &str =
    "Question for you: has the Build Tools 2022 install finished? I'm waiting for your answer.";

/// A board whose worker ended its turn on T-1 with `words` for its last words: the project,
/// once T-1 is no longer at work.
fn ended_on<'a>(rig: &'a Rig, words: &str) -> cf_e2e::Result<Project<'a>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added: Value = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} SAY {words}", tier(&added)),
    )?;
    rig.wait_for("the worker's turn to end", secs(60), || {
        Ok(matches!(
            project.task(1)?["state"].as_str(),
            Some("waiting" | "done")
        ))
    })?;
    Ok(project)
}

#[test]
fn a_turn_that_ends_asking_whether_to_go_on_is_a_question_the_chief_answers_and_the_reply_is_the_result(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = ended_on(&rig, SHOULD_I_CONTINUE)?;

    assert_eq!(project.task(1)?["state"], "waiting", "not done");
    let thread = told(&project)?;
    assert_eq!(
        thread,
        [("question".to_owned(), SHOULD_I_CONTINUE.to_owned())],
        "the worker's words as its question, and no result"
    );

    // `cf answer` reaches it: the worker takes the answer in and its reply is the result.
    let question = project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "question")
        .ok_or("the chief has no question")?;
    rig.tell(
        project.id(),
        &format!("CF answer m-{} :: Stop here.", question["id"]),
    )?;
    rig.wait_for("T-1 to be done on the answer", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let kinds: Vec<String> = told(&project)?.into_iter().map(|(kind, _)| kind).collect();
    assert_eq!(kinds, ["question", "result"]);
    rig.close()?;
    Ok(())
}

#[test]
fn a_turn_that_ends_saying_it_waits_for_the_chiefs_answer_is_a_question_and_its_task_waits(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = ended_on(&rig, WAITING_FOR_THE_INSTALL)?;

    assert_eq!(project.task(1)?["state"], "waiting", "not done");
    assert_eq!(
        told(&project)?,
        [("question".to_owned(), WAITING_FOR_THE_INSTALL.to_owned())]
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_report_that_ends_with_an_open_question_about_its_product_is_still_a_result() -> Outcome {
    let rig = Rig::start(config())?;
    let words = "The parser is done. Fine as it is?";
    let project = ended_on(&rig, words)?;

    assert_eq!(project.task(1)?["state"], "done");
    assert_eq!(told(&project)?, [("result".to_owned(), words.to_owned())]);
    rig.close()?;
    Ok(())
}
