//! The staff of a long run: the models, the tiers, and who is left out and why.

use std::path::PathBuf;

use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn each_harness_runs_on_the_models_it_has_unless_the_words_name_others() {
    let models = models_of(None).unwrap();
    assert_eq!(models.len(), 5);
    assert_eq!(models[0].1, ["claude-sonnet-5-5", "claude-opus-5-5"]);
    let named = models_of(Some(
        "claude=claude-fable-5-1 + claude-haiku-4-5, devin=swe-2-medium",
    ))
    .unwrap();
    assert_eq!(named[0].1, ["claude-fable-5-1", "claude-haiku-4-5"]);
    assert_eq!(named[4].1, ["swe-2-medium"]);
    assert_eq!(named[1].1, models[1].1, "codex is left as it was");
    for (line, said) in [
        (
            "claude",
            "--models names a harness and its models: claude=a+b, not claude",
        ),
        ("kimi=k2", "no such harness: kimi"),
        ("claude=", "--models gives claude no model"),
    ] {
        let refused = models_of(Some(line)).unwrap_err();
        assert!(matches!(refused, Error::Usage(_)));
        assert_eq!(refused.to_string(), said);
    }
}

#[test]
fn a_harness_not_on_the_machine_or_an_opencode_of_2_is_left_out_with_why() {
    let (found, left) = find(
        &names(&["claude", "codex", "opencode", "devin"]),
        |name| match name {
            "codex" => Err("codex was not found on the PATH".to_owned()),
            "opencode" => Ok((PathBuf::from("/bin/opencode"), "2.0.29".to_owned())),
            other => Ok((
                PathBuf::from(format!("/bin/{other}")),
                "1.0.0 (Claude Code)".to_owned(),
            )),
        },
    );
    let found: Vec<&str> = found.iter().map(|one| one.name.as_str()).collect();
    assert_eq!(found, ["claude", "devin"]);
    assert_eq!(
        left,
        [
            LeftOut {
                name: "codex".to_owned(),
                why: "not on this machine: codex was not found on the PATH".to_owned()
            },
            LeftOut {
                name: "opencode".to_owned(),
                why: "OpenCode 2.0.29 is 2.x, which ConsensFlow refuses (OpenCode stays on 1.x)"
                    .to_owned()
            }
        ]
    );
    let (found, left) = find(&names(&["opencode"]), |_| {
        Ok((PathBuf::from("/bin/opencode"), "opencode 1.19.0".to_owned()))
    });
    assert_eq!((found.len(), left.len()), (1, 0));
}

#[test]
fn what_a_harness_says_of_its_version_is_kept_with_a_token_in_it_hidden() {
    // A wrapper that prints a credential beside the version: kept, said on the
    // console and written in the report from here, so hidden here.
    let (found, left) = find(&names(&["pi", "opencode", "codex"]), |name| match name {
        "pi" => Ok((
            PathBuf::from("/bin/pi"),
            "1.2.3 token=abcd1234efgh".to_owned(),
        )),
        "opencode" => Ok((
            PathBuf::from("/bin/opencode"),
            "2.0.1 OPENAI_API_KEY=sk-abcdefghijklmnop1234".to_owned(),
        )),
        _ => Err("codex: password=hunter2hunter".to_owned()),
    });
    assert_eq!(found[0].version, "1.2.3 token=[masked]");
    assert!(
        left[0].why.contains("OPENAI_API_KEY=[masked]") && left[0].why.contains("is 2.x"),
        "{}",
        left[0].why
    );
    assert_eq!(left[1].why, "not on this machine: codex: password=[masked]");
}

#[test]
fn each_model_has_a_worker_and_the_first_an_advisor_and_a_reviewer_all_of_the_harnesss_tier() {
    let models = models_of(Some("claude=s+o,devin=d")).unwrap();
    let staff = staff_of(&names(&["claude", "devin", "codex"]), &models, "medium").unwrap();
    let rows: Vec<(String, &str, String, &str)> = staff
        .agents
        .iter()
        .zip(&staff.members)
        .map(|(agent, member)| {
            (
                agent.id.clone(),
                member.role,
                agent.model.clone(),
                agent.tier,
            )
        })
        .collect();
    let expected: Vec<(String, &str, String, &str)> = [
        ("long-claude-1", "worker", "s", "complex"),
        ("long-claude-2", "worker", "o", "complex"),
        ("long-claude-advisor", "advisor", "s", "complex"),
        ("long-claude-reviewer", "reviewer", "s", "complex"),
        ("long-devin-1", "worker", "d", "light"),
        ("long-devin-advisor", "advisor", "d", "light"),
        ("long-devin-reviewer", "reviewer", "d", "light"),
        ("long-codex-1", "worker", "gpt-5.6-luna", "standard"),
        ("long-codex-2", "worker", "gpt-6.1-sol", "standard"),
        ("long-codex-advisor", "advisor", "gpt-5.6-luna", "standard"),
        (
            "long-codex-reviewer",
            "reviewer",
            "gpt-5.6-luna",
            "standard",
        ),
    ]
    .iter()
    .map(|(id, role, model, tier)| ((*id).to_owned(), *role, (*model).to_owned(), *tier))
    .collect();
    assert_eq!(rows, expected);
    assert_eq!(staff.agents[0].to_value()["workTier"], "complex");
    assert_eq!(staff.agents[0].to_value()["effort"], "medium");
    assert!(
        staff.agents[4].to_value().get("effort").is_none(),
        "Devin has none"
    );
}
