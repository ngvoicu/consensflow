//! `the catalog turns a name into a working agent`: the ready-made agents, which
//! are listed from their names alone and are not the roster's to change, and
//! the agents of a person's own, which are.

use cf_e2e::ScratchHome;

use crate::{agent, Outcome};

#[test]
fn lists_ready_made_agents_per_tool() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["catalog"])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    for word in ["claude", "zeus", "codex", "hyperion", "glm-5.3"] {
        assert!(ran.stdout.contains(word), "{word}: {ran}");
    }
    Ok(())
}

#[test]
fn narrows_to_one_tool_on_request_and_answers_json_for_scripts() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["catalog", "--harness", "opencode"])?;
    assert!(ran.stdout.contains("mani"), "{ran}");
    assert!(!ran.stdout.contains("hyperion"), "{ran}");

    let json = home.cf(["catalog", "--json"])?.json()?;
    assert!(
        json["catalog"]["pi"]
            .as_array()
            .is_some_and(|agents| !agents.is_empty()),
        "{json}"
    );
    Ok(())
}

#[test]
fn lists_a_catalog_agent_from_its_name_alone_and_refuses_to_add_it_again() -> Outcome {
    let home = ScratchHome::new()?;
    let listed = home.cf(["agent", "list", "--json"])?.json()?;
    let hyperion = agent(&listed, "hyperion");
    assert_eq!(hyperion["harness"], "codex", "{listed}");
    assert_eq!(hyperion["model"], "gpt-6.1-sol", "{listed}");
    assert_eq!(hyperion["effort"], "max", "{listed}");
    let ran = home.cf(["agent", "add", "hyperion"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert!(ran.stderr.contains("catalog agent"), "{ran}");
    Ok(())
}

#[test]
fn a_catalog_agent_is_not_edited_or_removed_from_here_either() -> Outcome {
    let home = ScratchHome::new()?;
    let edited = home.cf(["agent", "edit", "hyperion", "--effort", "low"])?;
    assert_eq!(edited.code, Some(1), "{edited}");
    assert!(
        edited.stderr.contains("catalog agent and stays"),
        "{edited}"
    );
    let removed = home.cf(["agent", "remove", "hyperion"])?;
    assert_eq!(removed.code, Some(1), "{removed}");
    assert!(removed.stderr.contains("not yours to remove"), "{removed}");
    let reset = home.cf(["agent", "reset", "hyperion"])?;
    assert_eq!(reset.code, Some(1), "{reset}");
    Ok(())
}

#[test]
fn still_requires_harness_and_model_for_a_name_it_does_not_know() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["agent", "add", "nemo"])?;
    assert_ne!(ran.code, Some(0), "{ran}");
    let said = ran.output();
    assert!(
        said.contains("cf catalog") || said.contains("--harness"),
        "{ran}"
    );
    Ok(())
}

#[test]
fn an_edit_changes_one_field_of_an_agent_of_your_own_and_keeps_the_rest() -> Outcome {
    let home = ScratchHome::new()?;
    home.cf([
        "agent",
        "add",
        "my-luna",
        "--harness",
        "codex",
        "--model",
        "gpt-5.6-luna",
        "--effort",
        "xhigh",
    ])?;
    home.cf(["agent", "edit", "my-luna", "--effort", "low"])?;
    let listed = home.cf(["agent", "list", "--json"])?.json()?;
    let luna = agent(&listed, "my-luna");
    assert_eq!(luna["model"], "gpt-5.6-luna", "{listed}");
    assert_eq!(luna["effort"], "low", "{listed}");
    Ok(())
}
