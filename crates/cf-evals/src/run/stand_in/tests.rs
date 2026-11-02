//! The stand-in members' folders: made as a play begins, fresh each time, and gone with the run;
//! the journal of the briefs the worker took, and what the brief asked for that the script holds not.

use cf_e2e::live::{Harness, Installed};

use super::*;
use crate::scenarios::by_id;

fn machine(home: &Path) -> Machine {
    Machine {
        home: home.to_path_buf(),
        installs: vec![Installed::at(Harness::Claude, Path::new("/bin/echo"), home)],
        node: None,
        login: None,
    }
}

fn stand_ins(machine: &Machine) -> StandIns {
    let scenario = by_id("review-loop").unwrap();
    StandIns::of(machine, &scenario, Path::new("/bin/fake-agent")).unwrap()
}

#[test]
fn only_a_scenario_with_stand_in_members_has_them_and_a_member_for_each_it_scripts() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    for id in crate::scenarios::IDS {
        let scenario = by_id(id).unwrap();
        let found = StandIns::of(&machine, &scenario, Path::new("/bin/fake-agent"));
        let has = id == "review-loop" || id == "review-blockers";
        assert_eq!(found.is_some(), has, "{id}");
        if let Some(found) = found {
            assert_eq!(
                found.reviewer_folder().is_some(),
                !scenario.stand_in_reviews.is_empty()
            );
            assert_eq!(
                found.worker_folder().is_some(),
                !scenario.stand_in_work.is_empty()
            );
        }
    }
}

#[test]
fn the_folders_hold_the_marker_and_the_scenarios_answers_in_order_with_the_workers_files_and_are_made_again_for_each_play(
) {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    let scenario = by_id("review-blockers").unwrap();
    let stand_ins = StandIns::of(&machine, &scenario, Path::new("/bin/fake-agent")).unwrap();
    assert_eq!(stand_ins.agent(), Path::new("/bin/fake-agent"));
    assert_eq!(
        stand_ins.reviewer_folder(),
        Some(machine.scripted_reviews().as_path())
    );
    assert_eq!(
        stand_ins.worker_folder(),
        Some(machine.scripted_work().as_path())
    );
    assert!(!machine.scripted_reviews().exists() && !machine.scripted_work().exists());
    stand_ins.restart().unwrap();
    // The reviewer: the reviews, no files.
    let reviews = machine.scripted_reviews();
    assert!(reviews.join(MARKER).is_file());
    for (at, text) in scenario.stand_in_reviews.iter().enumerate() {
        let file = reviews.join(format!("{}.md", at + 1));
        assert_eq!(&fs::read_to_string(file).unwrap(), text);
        assert!(!reviews.join(format!("{}.files", at + 1)).exists());
    }
    // The worker: the results, and the files of each beside it, by their paths in the project.
    let work = machine.scripted_work();
    assert!(work.join(MARKER).is_file());
    for (at, answer) in scenario.stand_in_work.iter().enumerate() {
        let number = at + 1;
        assert_eq!(
            fs::read_to_string(work.join(format!("{number}.md"))).unwrap(),
            answer.result
        );
        for (path, text) in &answer.writes {
            let file = work.join(format!("{number}.files")).join(path);
            assert_eq!(&fs::read_to_string(file).unwrap(), text, "{number}: {path}");
        }
    }
    // A brief taken in a play, a journal, and a file that is nobody's: the next play begins afresh.
    for folder in [&reviews, &work] {
        fs::write(folder.join(".taken-1"), "").unwrap();
        fs::write(folder.join(JOURNAL), "{}\n").unwrap();
        fs::write(folder.join("stray"), "").unwrap();
    }
    stand_ins.restart().unwrap();
    for folder in [&reviews, &work] {
        for gone in [".taken-1", JOURNAL, "stray"] {
            assert!(!folder.join(gone).exists(), "{gone}");
        }
        assert!(folder.join(MARKER).is_file());
    }
}

#[test]
fn the_folders_go_with_the_run() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    let folders = {
        let stand_ins = stand_ins(&machine);
        stand_ins.restart().unwrap();
        let folders = [
            stand_ins.reviewer_folder().unwrap().to_path_buf(),
            stand_ins.worker_folder().unwrap().to_path_buf(),
        ];
        assert!(folders.iter().all(|folder| folder.is_dir()));
        folders
    };
    for folder in folders {
        assert!(!folder.exists(), "the run is over: {}", folder.display());
    }
}

#[test]
fn the_names_the_wrapper_and_the_stand_in_agent_agree_on_are_the_ones_the_fake_agent_reads() {
    // The stand-in is a binary of another crate, so the names are written twice: a test holds the
    // spellings to each other.
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("cf-e2e")
            .join("src")
            .join("bin")
            .join("fake_agent")
            .join("scripted.rs"),
    )
    .unwrap();
    assert!(source.contains(&format!("pub const FOLDER: &str = \"{FOLDER}\";")));
    assert!(source.contains(&format!("pub const MARKER: &str = \"{MARKER}\";")));
    assert!(source.contains(&format!("pub const JOURNAL: &str = \"{JOURNAL}\";")));
}

#[test]
fn the_briefs_the_worker_took_are_read_from_its_journal_with_what_each_asked_that_the_script_holds_not(
) {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    let stand_ins = stand_ins(&machine);
    // No folder yet, and no worker: nothing taken.
    assert_eq!(stand_ins.work_taken().unwrap(), Some(Vec::new()));
    stand_ins.restart().unwrap();
    assert_eq!(stand_ins.work_taken().unwrap(), Some(Vec::new()));
    let line = |taken: i64, given: i64, brief: &str, wrote: &[&str]| serde_json::json!({ "taken": taken, "given": given, "brief": brief, "wrote": wrote });
    let journal = [
        line(1, 1, "Create ONE new file: site/en/assessment.html. Change no other file.", &["site/en/assessment.html"]),
        line(
            2,
            2,
            "Change the score range on two pages:\n1. site/evaluare.html, line 11: replace the range\n2. site/en/assessment.html, line 11: replace the range",
            &["site/en/assessment.html"],
        ),
    ]
    .map(|value| value.to_string())
    .join("\n");
    fs::write(machine.scripted_work().join(JOURNAL), journal + "\n").unwrap();
    let taken = stand_ins.work_taken().unwrap().unwrap();
    assert_eq!(taken.len(), 2);
    assert_eq!((taken[0].taken, taken[0].given), (1, 1));
    assert_eq!(taken[0].wrote, ["site/en/assessment.html"]);
    assert_eq!(taken[0].unheld, Vec::<String>::new());
    // The second brief asked for the Romanian page too, and its answer wrote the English one only.
    assert_eq!(taken[1].unheld, ["site/evaluare.html"]);
    assert_eq!(
        taken[1].to_value()["unheld"],
        serde_json::json!(["site/evaluare.html"])
    );
    assert!(taken[1].to_value()["brief"]
        .as_str()
        .unwrap()
        .contains("two pages"));
    // A scenario with a reviewer and no worker has no worker to ask.
    let mut only = by_id("review-loop").unwrap();
    only.stand_in_work.clear();
    let only = StandIns::of(&machine, &only, Path::new("/bin/fake-agent")).unwrap();
    assert_eq!(only.work_taken().unwrap(), None);
}

#[test]
fn a_file_is_asked_for_that_the_script_holds_not_where_a_line_of_the_brief_makes_or_changes_it() {
    let wrote = ["site/en/assessment.html".to_owned()];
    // What the script wrote is held, however the brief spells the path.
    assert!(unheld("Create site/en/assessment.html.", &wrote).is_empty());
    assert!(unheld(
        "Create /Users/me/.consensflow-candidate/evals/workspaces/1-2/site/en/assessment.html.",
        &wrote
    )
    .is_empty());
    assert!(unheld("Creează ./site/en/assessment.html.", &wrote).is_empty());
    // What it did not write is not, in English or Romanian.
    assert_eq!(
        unheld("Fix docs/ghidul-managerului.md line 5.", &wrote),
        ["docs/ghidul-managerului.md"]
    );
    assert_eq!(
        unheld("Modifică site/evaluare.html, linia 11.", &wrote),
        ["site/evaluare.html"]
    );
    assert_eq!(
        unheld(
            "Add a footer to site/en/index.html and to site/en/assessment.html",
            &wrote
        ),
        ["site/en/index.html"]
    );
    // A line that leaves a file alone, or only reads it, asks nothing of the script.
    assert!(unheld(
        "Change no other file than site/en/assessment.html. Leave site/notes.md and docs/ alone.",
        &wrote
    )
    .is_empty());
    assert!(unheld(
        "I read docs/ghidul-managerului.md and site/evaluare.html just now.",
        &wrote
    )
    .is_empty());
    assert!(unheld("Do not change site/evaluare.html.", &wrote).is_empty());
    assert!(unheld("Nu modifica site/evaluare.html.", &wrote).is_empty());
    // Each file once.
    assert_eq!(
        unheld("Edit site/a.html.\nThen edit site/a.html again.", &wrote),
        ["site/a.html"]
    );
}
