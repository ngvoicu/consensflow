//! How a case ended: equal, skipped where a CLI could not answer, or the places
//! the two sides differ at.

use std::sync::LazyLock;

use regex::Regex;

use crate::compare::{describe, differences};
use crate::normalize::Normal;
use crate::{Case, Outcome};

/// What became of a case.
pub(super) enum Verdict {
    Equal,
    /// The CLI could not answer, and both sides were refused in its words.
    Skipped(String),
    Differs(Vec<String>),
}

/// The ways Node's adapters say a CLI could not answer a question of a plan
/// (a version or a help it was asked, the instructions of Codex, the
/// throwaway server of OpenCode): it ran out of time, would not start, was
/// too old, or said what no one can read.
static UNANSWERED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(could not ask Codex whether it has its native queue",
        r"|Cannot read native Codex instructions safely",
        r"|(This Codex|Codex \S+) has no native queue",
        r"|opencode serve (failed to start|exited early)",
        r"|opencode session (timed out|transport failed|unauthorized|rejected with status|returned |failed to stop)",
        r"|Devin \S+ or newer is required",
        r"|Command failed: ",
        r"|spawn )",
    ))
    .unwrap()
});

/// How a case ended, by what both sides did.
pub(super) fn verdict(case: &Case, node: &Normal, rust: &Normal) -> Verdict {
    let mut found = differences(node, rust);
    // A CLI that could not answer says nothing of the plan, nor of the
    // refusal a case was made for, which only comes once it has answered.
    if let Outcome::Refused(sentence) = &node.outcome {
        if UNANSWERED.is_match(sentence) {
            return if found.is_empty() {
                Verdict::Skipped(sentence.clone())
            } else {
                Verdict::Differs(found)
            };
        }
    }
    if let Some(begins) = &case.refuses {
        for (side, normal) in [("node", node), ("rust", rust)] {
            match &normal.outcome {
                Outcome::Refused(sentence) if sentence.starts_with(begins.as_str()) => {}
                other => found.push(format!(
                    "  {side} was to be refused with \"{begins}…\" and {}",
                    describe(other)
                )),
            }
        }
    }
    if found.is_empty() {
        Verdict::Equal
    } else {
        Verdict::Differs(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{spellings_of, LAUNCH};
    use crate::normalize::{normalize, Raw};
    use crate::{Change, Plan};

    #[test]
    fn a_refusal_is_the_clis_for_a_cli_that_could_not_answer_and_never_for_a_launch_s_own() {
        for unanswered in [
            "could not ask Codex whether it has its native queue: it did not answer in time",
            "Cannot read native Codex instructions safely",
            "Codex 0.150.0 has no native queue, which ConsensFlow needs to reach its window: update Codex.",
            "This Codex has no native queue, which ConsensFlow needs to reach its window: update Codex.",
            "opencode serve failed to start",
            "opencode serve exited early",
            "opencode session timed out",
            "opencode session rejected with status 500",
            "opencode session returned the wrong directory",
            "Devin 3000.10.21 or newer is required for complete worker replies. Update Devin before opening this pane.",
            "Command failed: /bin/devin --version\n",
            "spawn /bin/devin ENOENT",
        ] {
            assert!(UNANSWERED.is_match(unanswered), "{unanswered}");
        }
        for ours in [
            "the chief window needs its role text",
            "opencode session needs a working directory",
            "ConsensFlow's Pi extension could not be installed: EACCES",
            "Cannot read native Devin configuration; the original was preserved",
            "OpenCode has a custom OPENCODE_TUI_CONFIG; its settings were preserved.",
        ] {
            assert!(!UNANSWERED.is_match(ours), "{ours}");
        }
    }

    fn case(refuses: Option<&str>) -> Case {
        serde_json::from_value(serde_json::json!({
            "kind": "codex",
            "name": "member-fresh",
            "refuses": refuses,
            "launch": {
                "launchId": LAUNCH, "project": 7, "handle": "rhea", "role": "worker",
                "resume": null, "message": null, "agent": null, "instructions": "x",
            },
            "node": {
                "env": [], "directory": "/work",
                "outcome": { "refused": "unused" }, "changes": [], "owned": {}, "ms": 1.0,
            },
            "rust": { "env": [], "directory": "/work" },
        }))
        .unwrap()
    }

    fn planned(argv: &[&str]) -> Normal {
        Normal {
            setting: vec!["HOME=$ROOT/home".to_owned()],
            outcome: Outcome::Plan(Plan {
                argv: argv.iter().map(|&argument| argument.to_owned()).collect(),
                env: vec![("A".to_owned(), "1".to_owned())],
                drop_env: Vec::new(),
                native_session: None,
            }),
            changes: Vec::new(),
            pi_hashes: Vec::new(),
        }
    }

    fn refused(sentence: &str) -> Normal {
        Normal {
            outcome: Outcome::Refused(sentence.to_owned()),
            ..planned(&[])
        }
    }

    fn differs(verdict: Verdict) -> String {
        match verdict {
            Verdict::Differs(found) => found.join("\n"),
            Verdict::Equal => panic!("equal"),
            Verdict::Skipped(reason) => panic!("skipped: {reason}"),
        }
    }

    #[test]
    fn two_roots_of_one_shape_are_one_setting_and_a_root_that_is_not_is_said() {
        let [node, rust] = ["node", "rust"].map(|side| {
            let root = format!("/tmp/consensflow launch %#-X/{side}");
            let env = vec![
                ("HOME".to_owned(), format!("{root}/home")),
                ("TMPDIR".to_owned(), format!("{root}/tmp")),
            ];
            normalize(
                &Raw {
                    spellings: &spellings_of(side),
                    env: &env,
                    directory: &format!("{root}/work"),
                    outcome: &Outcome::Refused(String::new()),
                    changes: &[],
                },
                &[],
            )
        });
        assert_eq!(
            node.setting,
            [
                "HOME=$ROOT/home",
                "TMPDIR=$ROOT/tmp",
                "directory=$ROOT/work"
            ]
        );
        assert_eq!(node.setting, rust.setting);
        let odd = Normal {
            setting: vec!["HOME=$ROOT/elsewhere".to_owned()],
            ..planned(&["a"])
        };
        let said = differs(verdict(&case(None), &planned(&["a"]), &odd));
        assert!(said.contains("setting[0]"), "{said}");
    }

    #[test]
    fn two_plans_are_equal_and_a_place_they_differ_at_is_said() {
        let plain = case(None);
        assert!(matches!(
            verdict(&plain, &planned(&["a", "b"]), &planned(&["a", "b"])),
            Verdict::Equal
        ));
        let said = differs(verdict(
            &plain,
            &planned(&["a", "b", "c"]),
            &planned(&["a", "x"]),
        ));
        assert!(
            said.contains("argv[1]:\n    node: \"b\"\n    rust: \"x\""),
            "{said}"
        );
        assert!(
            said.contains("argv[2]:\n    node: \"c\"\n    rust: (none)"),
            "{said}"
        );
        let mut moved = planned(&["a"]);
        if let Outcome::Plan(plan) = &mut moved.outcome {
            plan.env = vec![
                ("B".to_owned(), "2".to_owned()),
                ("A".to_owned(), "1".to_owned()),
            ];
        }
        let said = differs(verdict(&plain, &planned(&["a"]), &moved));
        assert!(
            said.contains("env[0]"),
            "an environment's order counts: {said}"
        );
    }

    #[test]
    fn a_tree_is_held_to_its_paths_its_modes_and_its_texts() {
        let file = |path: &str, mode: u32, text: &str| {
            let mut change = Change::new(path, "file");
            change.mode = Some(mode);
            change.text = Some(text.to_owned());
            change
        };
        let with = |changes: Vec<Change>| Normal {
            changes,
            ..planned(&["a"])
        };
        let node = with(vec![
            file("a", 0o600, "x"),
            file("b", 0o600, "y"),
            file("c", 0o600, "z"),
        ]);
        let rust = with(vec![
            file("a", 0o644, "x"),
            file("b", 0o600, "Y"),
            file("d", 0o600, "z"),
        ]);
        let said = differs(verdict(&case(None), &node, &rust));
        assert!(said.contains("a:\n    mode: node 600, rust 644"), "{said}");
        assert!(
            said.contains("b:\n    text:\n      node: \"y\"\n      rust: \"Y\""),
            "{said}"
        );
        assert!(said.contains("c: only node changed it"), "{said}");
        assert!(said.contains("d: only rust changed it"), "{said}");
    }

    #[test]
    fn pi_s_bundle_is_held_to_the_name_it_is_published_under() {
        let mut node = planned(&["a"]);
        let mut rust = planned(&["a"]);
        node.pi_hashes = vec!["a".repeat(64)];
        rust.pi_hashes = vec!["b".repeat(64)];
        let said = differs(verdict(&case(None), &node, &rust));
        assert!(
            said.contains("Pi's bundle is published under another name"),
            "{said}"
        );
    }

    #[test]
    fn a_cli_that_could_not_answer_is_skipped_where_rust_could_not_either_and_a_difference_where_it_could(
    ) {
        let sentence = "opencode session timed out";
        assert!(matches!(
            verdict(&case(None), &refused(sentence), &refused(sentence)),
            Verdict::Skipped(reason) if reason == sentence
        ));
        let said = differs(verdict(&case(None), &refused(sentence), &planned(&["a"])));
        assert!(said.contains("node was refused"), "{said}");
        let said = differs(verdict(
            &case(None),
            &refused(sentence),
            &refused("opencode serve exited early"),
        ));
        assert!(said.contains("refusal:"), "{said}");
        // A refusal of ConsensFlow's own, in the same words, is an answer: equal.
        let ours = "the chief window needs its role text";
        assert!(matches!(
            verdict(&case(None), &refused(ours), &refused(ours)),
            Verdict::Equal
        ));
    }

    #[test]
    fn a_case_that_is_to_be_refused_is_refused_by_both_sides_in_its_words() {
        let sentence = "Private pi integration differs from this build";
        let wanted = case(Some(sentence));
        assert!(matches!(
            verdict(&wanted, &refused(sentence), &refused(sentence)),
            Verdict::Equal
        ));
        let said = differs(verdict(&wanted, &planned(&["a"]), &planned(&["a"])));
        assert!(
            said.contains("node was to be refused") && said.contains("rust was to be refused"),
            "{said}"
        );
        let said = differs(verdict(
            &wanted,
            &refused(sentence),
            &refused("another sentence"),
        ));
        assert!(said.contains("rust was to be refused"), "{said}");
        // A CLI that could not answer is no refusal the case wanted, nor a failure of it.
        let timed_out = "opencode session timed out";
        assert!(matches!(
            verdict(&wanted, &refused(timed_out), &refused(timed_out)),
            Verdict::Skipped(reason) if reason == timed_out
        ));
        let said = differs(verdict(&wanted, &refused(timed_out), &refused(sentence)));
        assert!(said.contains("refusal:"), "{said}");
    }
}
