use cf_e2e::checkout;

use super::*;

#[test]
fn every_harness_is_the_chief_of_one_project_which_runs_every_step_in_order() {
    let all = projects(&[], &[]);
    let chiefs: Vec<Harness> = all.iter().map(|project| project.chief).collect();
    assert_eq!(chiefs, Harness::ALL);
    assert_eq!(
        all.iter().map(|project| project.number).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );
    for project in &all {
        assert_eq!(project.steps, Step::ALL, "{}", project.name());
    }
    assert_eq!(all[0].name(), "p1 claude");
    assert_eq!(all[4].name(), "p5 codex");
    assert_eq!(all[5].name(), "p6 muse");
}

#[test]
fn the_harnesses_named_are_the_only_chiefs_and_keep_their_place_in_the_rotation() {
    let two = projects(&[Harness::Pi, Harness::Claude], &[]);
    let seen: Vec<(usize, Harness)> = two
        .iter()
        .map(|project| (project.number, project.chief))
        .collect();
    assert_eq!(seen, [(1, Harness::Claude), (3, Harness::Pi)]);
}

#[test]
fn the_steps_asked_for_are_the_only_steps_run_in_their_own_order_and_the_board_brings_its_spec() {
    let only = projects(&[Harness::Claude], &[Step::Small, Step::Off]);
    assert_eq!(only[0].steps, [Step::Off, Step::Small]);
    assert!(only[0].runs(Step::Off) && !only[0].runs(Step::Spec));
    // The board carries out a spec: asked for alone, it has the step that makes one first.
    let board = projects(&[Harness::Claude], &[Step::Board]);
    assert_eq!(board[0].steps, [Step::Spec, Step::Board]);
    let spec = projects(&[Harness::Claude], &[Step::Spec]);
    assert_eq!(spec[0].steps, [Step::Spec]);
}

#[test]
fn a_run_plans_with_the_variant_it_was_asked_for_whether_or_not_the_harness_has_it() {
    let has = |name: &str| name == "tdd";
    assert_eq!(
        chosen_variant(Some("tdd"), has),
        Ok(Chosen {
            name: "tdd",
            installed: true
        })
    );
    assert_eq!(
        chosen_variant(Some("core-html"), has),
        Ok(Chosen {
            name: "core-html",
            installed: false
        })
    );
    let refused = chosen_variant(Some("deluxe"), has).unwrap_err();
    assert_eq!(
        refused,
        "no variant is called deluxe: Specmint's are core, tdd, core-html, tdd-html"
    );
}

#[test]
fn by_default_a_run_plans_with_the_first_variant_the_harness_lacks() {
    let has_core_and_tdd = |name: &str| ["core", "tdd"].contains(&name);
    assert_eq!(chosen_variant(None, |_| false).unwrap().name, "core");
    // It has core and tdd: the next is the one ConsensFlow's own copy is given for.
    assert_eq!(
        chosen_variant(None, has_core_and_tdd),
        Ok(Chosen {
            name: "core-html",
            installed: false
        })
    );
    // A harness that has all four is given the first, and its own copy is the one used.
    assert_eq!(
        chosen_variant(None, |_| true),
        Ok(Chosen {
            name: "core",
            installed: true
        })
    );
}

#[test]
fn lists_name_harnesses_steps_and_a_variant_by_commas_or_blanks_and_a_wrong_word_is_the_error() {
    assert_eq!(
        harnesses_named("claude, pi opencode,,"),
        Ok(vec![Harness::Claude, Harness::Pi, Harness::OpenCode])
    );
    assert_eq!(harnesses_named(""), Ok(Vec::new()));
    assert_eq!(
        harnesses_named("claude,gemini"),
        Err("no harness is called gemini".to_owned())
    );
    assert_eq!(steps_named("board spec"), Ok(vec![Step::Board, Step::Spec]));
    assert_eq!(
        steps_named("interview"),
        Err("no step is called interview".to_owned())
    );
    assert_eq!(variant_named(""), Ok(None));
    assert_eq!(variant_named(" tdd "), Ok(Some("tdd".to_owned())));
    assert!(variant_named("tdd,core").is_err());
}

#[test]
fn the_variants_are_the_ones_the_repository_vendors() {
    let manifest = checkout::path("skill/specmint/manifest.json");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut vendored: Vec<&str> = parsed
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    vendored.sort_unstable();
    let mut listed = VARIANTS.to_vec();
    listed.sort_unstable();
    assert_eq!(listed, vendored);
}

#[test]
fn the_steps_are_called_by_the_words_the_brief_gives_them() {
    let names: Vec<&str> = Step::ALL.into_iter().map(Step::name).collect();
    assert_eq!(names, ["off", "source", "small", "spec", "board"]);
}
