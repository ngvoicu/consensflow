//! The evals' scripted reviewer (T-1010, SCX-17) end to end through the real pane host: a
//! reviewer whose window is started with `CF_TEST_SCRIPTED_RESULTS` answers each review the
//! chief puts on the board with the next result of the run's folder, whatever the brief says,
//! and the daemon collects it as the review's result like any other. The brief is the live
//! chief's, so nothing of the script can be in it: the window counts the briefs it is given in
//! the folder, since each review is a fresh window.

use std::fs;

use cf_e2e::rig::{write_staff, Project};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The result of the review task `number`, as the chief was given it.
fn result_of(project: &Project, number: i64) -> cf_e2e::Result<Value> {
    Ok(project.task(number)?["messages"]
        .as_array()
        .and_then(|messages| messages.iter().find(|message| message["kind"] == "result"))
        .map_or(Value::Null, |message| message["body"].clone()))
}

#[test]
fn a_reviewer_that_is_the_scripted_stand_in_answers_each_review_with_the_next_result_of_its_folder_and_the_last_again(
) -> Outcome {
    let folder = tempfile::tempdir()?;
    fs::write(folder.path().join(".eval-run"), "")?;
    fs::write(folder.path().join("1.md"), "REVIEW ONE: F1 blocks")?;
    fs::write(folder.path().join("2.md"), "REVIEW TWO: notes only")?;
    let rig = cf_e2e::rig::Rig::start(
        config().var("CF_TEST_SCRIPTED_RESULTS", folder.path().to_string_lossy()),
    )?;
    write_staff(&rig)?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("checker", "reviewer")])?;
    let tier = project.tiers()?.get("checker").cloned().unwrap_or_default();
    // One review at a time, as a chief asks for the second after the fix: the brief says nothing
    // of any script, and each is the first message of a fresh window.
    for (number, expected) in [
        (1, "REVIEW ONE: F1 blocks"),
        (2, "REVIEW TWO: notes only"),
        (3, "REVIEW TWO: notes only"),
    ] {
        rig.tell(
            project.id(),
            &format!(
                "DISPATCH --review --tier {tier} Review the page and say what is wrong with it."
            ),
        )?;
        rig.wait_for(&format!("review T-{number} to be done"), secs(90), || {
            Ok(project.task(number)?["state"] == "done")
        })?;
        assert_eq!(
            result_of(&project, number)?,
            json!(expected),
            "T-{number}'s result"
        );
        assert_eq!(project.task(number)?["pool"], "reviewer");
    }
    rig.close()?;
    Ok(())
}

#[test]
fn a_reviewer_whose_folder_is_no_eval_runs_is_refused_and_says_so_instead_of_answering() -> Outcome
{
    // A folder with the results and no marker: not an eval run's.
    let folder = tempfile::tempdir()?;
    fs::write(folder.path().join("1.md"), "REVIEW ONE: F1 blocks")?;
    let rig = cf_e2e::rig::Rig::start(
        config().var("CF_TEST_SCRIPTED_RESULTS", folder.path().to_string_lossy()),
    )?;
    write_staff(&rig)?;
    let project = Project::open_with_staff(&rig, "chief", None, &[("checker", "reviewer")])?;
    let tier = project.tiers()?.get("checker").cloned().unwrap_or_default();
    rig.tell(
        project.id(),
        &format!("DISPATCH --review --tier {tier} Review the page."),
    )?;
    rig.wait_for("the review to be given to its window", secs(60), || {
        Ok(project.task(1)?["state"] != "open")
    })?;
    // Whatever the window did, the review never came back with the script's words.
    std::thread::sleep(secs(5));
    assert_ne!(
        result_of(&project, 1)?,
        json!("REVIEW ONE: F1 blocks"),
        "a folder with no marker is no run's"
    );
    assert!(
        !folder.path().join(".taken-1").exists(),
        "and nothing was taken from it"
    );
    rig.close()?;
    Ok(())
}
