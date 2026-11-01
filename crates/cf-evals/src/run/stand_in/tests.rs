//! The stand-in reviewer's folder: made as a play begins, fresh each time, and gone with the run.

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

#[test]
fn only_a_scenario_with_a_stand_in_reviewer_has_one() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    for id in crate::scenarios::IDS {
        let scenario = by_id(id).unwrap();
        let reviews = Reviews::of(&machine, &scenario, Path::new("/bin/fake-agent"));
        assert_eq!(
            reviews.is_some(),
            id == "review-loop" || id == "review-blockers",
            "{id}"
        );
    }
}

#[test]
fn the_folder_holds_the_marker_and_the_scenarios_reviews_in_order_and_is_made_again_for_each_play()
{
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    let scenario = by_id("review-loop").unwrap();
    let reviews = Reviews::of(&machine, &scenario, Path::new("/bin/fake-agent")).unwrap();
    assert_eq!(reviews.agent(), Path::new("/bin/fake-agent"));
    assert_eq!(reviews.folder(), machine.scripted_reviews());
    assert!(!reviews.folder().exists(), "not made before a play");
    reviews.restart().unwrap();
    assert!(reviews.folder().join(MARKER).is_file());
    for (at, text) in scenario.stand_in_reviews.iter().enumerate() {
        let file = reviews.folder().join(format!("{}.md", at + 1));
        assert_eq!(&fs::read_to_string(file).unwrap(), text);
    }
    // A brief taken in a play, and a file that is nobody's: the next play begins afresh.
    fs::write(reviews.folder().join(".taken-1"), "").unwrap();
    fs::write(reviews.folder().join("stray"), "").unwrap();
    reviews.restart().unwrap();
    assert!(!reviews.folder().join(".taken-1").exists());
    assert!(!reviews.folder().join("stray").exists());
    assert!(reviews.folder().join(MARKER).is_file());
}

#[test]
fn the_folder_goes_with_the_run() {
    let home = tempfile::tempdir().unwrap();
    let machine = machine(home.path());
    let scenario = by_id("review-blockers").unwrap();
    let folder = {
        let reviews = Reviews::of(&machine, &scenario, Path::new("/bin/fake-agent")).unwrap();
        reviews.restart().unwrap();
        assert!(reviews.folder().is_dir());
        reviews.folder().to_path_buf()
    };
    assert!(!folder.exists(), "the run is over: {}", folder.display());
}

#[test]
fn the_names_the_wrapper_and_the_stand_in_agree_on_are_the_ones_the_fake_agent_reads() {
    // The stand-in is a binary of another crate, so the names are written twice: a test holds
    // the two spellings to each other.
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
}
