use cf_e2e::live::Harness;
use regex::Regex;

use super::*;

fn planned(chief: Harness) -> Planned {
    Planned {
        number: 3,
        chief,
        steps: Vec::new(),
    }
}

#[test]
fn the_folder_tells_the_chief_how_to_use_the_scripted_workers_and_where_to_ask_its_questions() {
    // The stand-in reads the first word after `Reply with exactly:`: the line the folder
    // has the chief end its briefs with is the one it answers.
    let answer = Regex::new(r"Reply with exactly: (\S+)").unwrap();
    let said = |text: &str| answer.captures(text).map(|found| found[1].to_owned());
    assert_eq!(
        said("Add hello.txt.\nReply with exactly: DONE-HELLO-01").as_deref(),
        Some("DONE-HELLO-01")
    );
    assert!(BRIEF
        .contains("end every brief you put on the board with the line `Reply with exactly: DONE`"));
    assert!(BRIEF.contains("`Reply with exactly: DONE-<code>`"));
    assert!(BRIEF.contains("accept a result that says so"));
    assert!(BRIEF.contains("ask in plain text, all your questions in one message"));
    assert!(BRIEF.contains("Nothing in this project needs research"));
    assert!(BRIEF.ends_with(".\n"));
    // It tells nothing of specs, which a project with them off is told nothing of: what
    // the chief knows of them it is told by its role.
    assert!(!BRIEF.to_lowercase().contains("spec"));
}

#[test]
fn the_roster_is_the_chief_s_harness_on_its_cheap_model_and_the_stand_in_worker_in_one_tier() {
    let rows = roster(&planned(Harness::Pi));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "live-pi");
    assert_eq!(rows[0]["kind"], "pi");
    assert_eq!(rows[0]["workTier"], TIER);
    assert_eq!(rows[1]["id"], "worker");
    assert_eq!(rows[1]["kind"], "claude-code");
    assert_eq!(rows[1]["model"], "fake");
    assert_eq!(rows[1]["workTier"], TIER);
    assert_ne!(TIER, "critical");
}

#[test]
fn the_page_is_asked_to_turn_specs_on_with_the_folder_and_the_variant_and_says_so_back() {
    let settings = Settings {
        folder: "docs/specs".to_owned(),
        variant: Chosen {
            name: "tdd",
            installed: false,
        },
        installed_at: None,
    };
    assert_eq!(
        settings.request(4),
        json!({ "project": 4, "on": true, "folder": "docs/specs", "variant": "tdd" })
    );
    assert_eq!(
        settings.view(),
        json!({ "on": true, "folder": "docs/specs", "variant": "tdd" })
    );
}
