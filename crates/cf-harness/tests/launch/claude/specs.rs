//! The Specmint skill a Claude chief is given where its project has specs on:
//! written beside the role's skill in the folder Claude is already given for it
//! (`--add-dir`), so that nothing else of the launch moves; or the human's own,
//! where Claude has the variant installed, and then none is written.

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_harness::contract::{Adapter, LaunchId, Skill};
use cf_proto::specs::Variant;

use super::{prepare, Home, Request, LAUNCH};

/// The names in the skills folder of a launch's role: the role's own skill,
/// and the Specmint skill of a chief given specs.
fn role_skills(home: &Home) -> Vec<String> {
    let skills = path::join(&[&home.launch_folder(), "role", ".claude", "skills"]);
    let mut names: Vec<String> = fs::read_dir(skills)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_chief_given_specs_has_the_skill_written_beside_its_role_in_the_folder_claude_is_given_and_nothing_else_of_its_launch_moves(
) {
    for variant in Variant::ALL {
        let home = Home::new();
        let off = prepare(&home.adapter(), &Request::chief()).unwrap();
        assert_eq!(
            role_skills(&home),
            ["consensflow-chief"],
            "with specs off the launch holds the role alone"
        );
        let request = Request {
            specs: Some(variant),
            ..Request::chief()
        };
        let on = prepare(&home.adapter(), &request).unwrap();
        assert_eq!(
            on.argv, off.argv,
            "{variant:?}: Claude is told no more than it was"
        );
        assert_eq!((on.env, on.drop_env), (off.env, off.drop_env));
        assert_eq!(
            role_skills(&home),
            ["consensflow-chief", variant.skill_name()],
            "{variant:?}: the skill is beside the role's, in the folder Claude adds"
        );
        let added = on.argv.iter().position(|arg| arg == "--add-dir").unwrap();
        let skill = path::join(&[
            &on.argv[added + 1],
            ".claude",
            "skills",
            variant.skill_name(),
        ]);
        assert_eq!(
            fs::read(path::join(&[&skill, "SKILL.md"])).unwrap(),
            fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../skill/specmint")
                    .join(variant.as_str())
                    .join("SKILL.md")
            )
            .unwrap(),
            "{variant:?}: the variant as it is vendored"
        );
    }
}

#[test]
fn a_chief_whose_claude_has_the_variant_installed_is_given_nothing_of_it_and_a_variant_installed_that_is_not_the_one_is_left_as_it_is(
) {
    let home = Home::new();
    let skills = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "skills"]);
    let skill = |name: &str| {
        let folder = path::join(&[&skills, name]);
        fs::create_dir_all(&folder).unwrap();
        let file = path::join(&[&folder, "SKILL.md"]);
        fs::write(
            &file,
            format!("---\nname: {name}\n---\nthe human's own {name}\n"),
        )
        .unwrap();
        file
    };
    let tdd = skill("specmint-tdd");
    let adapter = home.adapter();
    let id = LaunchId::new(LAUNCH).unwrap();
    // The variant the project chose is installed: that copy is the window's.
    assert_eq!(
        adapter.specmint(Variant::Tdd, &id).unwrap(),
        Skill::Global(tdd.clone())
    );
    let request = Request {
        specs: Some(Variant::Tdd),
        ..Request::chief()
    };
    prepare(&adapter, &request).unwrap();
    assert_eq!(
        role_skills(&home),
        ["consensflow-chief"],
        "nothing of ConsensFlow's is written"
    );
    // Another variant is chosen: the installed one is no copy of it, and is not touched.
    let request = Request {
        specs: Some(Variant::Core),
        ..Request::chief()
    };
    let before = fs::read(&tdd).unwrap();
    assert!(matches!(
        adapter.specmint(Variant::Core, &id).unwrap(),
        Skill::Own(_)
    ));
    prepare(&adapter, &request).unwrap();
    assert_eq!(role_skills(&home), ["consensflow-chief", "specmint-core"]);
    assert_eq!(
        fs::read(&tdd).unwrap(),
        before,
        "the human's own is as it was"
    );
}
