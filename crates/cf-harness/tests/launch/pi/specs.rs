//! The Specmint skill a Pi chief is given where its project has specs on: written
//! beside the role's skill in the launch's folder and loaded as a second `--skill`,
//! or the human's own, where Pi has the variant installed, and then none is
//! written and none is given.

use std::fs;

use cf_base::path;
use cf_harness::contract::{Adapter, LaunchId, Prepared, Skill};
use cf_harness::testing::unrooted;
use cf_proto::specs::Variant;

use super::{adapter, prepare, Home, Request, LAUNCH};

/// A chief's launch: the role of its own, no first message.
fn chief() -> Request {
    Request {
        role: "chief",
        message: None,
        ..Request::default()
    }
}

#[test]
fn a_chief_given_specs_is_given_the_skill_as_a_second_one_after_its_role_s_and_the_rest_of_its_launch_is_as_it_was(
) {
    for variant in Variant::ALL {
        // Two homes of one make: what differs between the launches is the specs.
        let (off_home, home) = (Home::new(), Home::new());
        let (off_adapter, _fakes) = adapter(&off_home);
        let (adapter, _fakes) = adapter(&home);
        let off = prepare(&off_adapter, &chief()).unwrap();
        let request = Request {
            specs: Some(variant),
            ..chief()
        };
        let on = prepare(&adapter, &request).unwrap();
        let skill = path::join(&[
            &home.launch_folder(),
            "role",
            ".claude",
            "skills",
            variant.skill_name(),
            "SKILL.md",
        ]);
        let (mut expected, env, drop_env) = unrooted(&off, &off_home.root);
        assert_eq!(expected[3], "--skill", "the role's skill comes first");
        expected.splice(
            5..5,
            ["--skill".to_owned(), skill.replace(&home.root, "<home>")],
        );
        assert_eq!(
            unrooted(&on, &home.root),
            (expected, env, drop_env),
            "{variant:?}"
        );
        assert!(
            fs::read_to_string(&skill)
                .unwrap()
                .starts_with(&format!("---\nname: {}\n", variant.skill_name())),
            "{variant:?}"
        );
        assert_eq!(
            adapter
                .specmint(variant, &LaunchId::new(LAUNCH).unwrap())
                .unwrap(),
            Skill::Own(skill),
            "the place the role text names is the place written"
        );
    }
}

#[test]
fn a_pi_that_has_the_variant_installed_is_given_no_skill_of_ours_and_an_installed_variant_that_is_not_the_one_is_left_alone(
) {
    let home = Home::new();
    let install = |folder: &str, name: &str| {
        let dir = path::join(&[&home.root, "home", folder, name]);
        fs::create_dir_all(&dir).unwrap();
        let file = path::join(&[&dir, "SKILL.md"]);
        fs::write(&file, format!("---\nname: {name}\n---\nthe human's own\n")).unwrap();
        file
    };
    let own = install(".pi/agent/skills", "specmint-core-html");
    let shared = install(".agents/skills", "specmint-tdd-html");
    let (adapter, _fakes) = adapter(&home);
    let id = LaunchId::new(LAUNCH).unwrap();
    let skills_given = |plan: &Prepared| plan.argv.iter().filter(|arg| *arg == "--skill").count();
    let off = prepare(&adapter, &chief()).unwrap();
    assert_eq!(skills_given(&off), 1, "the role's");
    for (variant, file) in [(Variant::CoreHtml, &own), (Variant::TddHtml, &shared)] {
        assert_eq!(
            adapter.specmint(variant, &id).unwrap(),
            Skill::Global(file.clone())
        );
        let request = Request {
            specs: Some(variant),
            ..chief()
        };
        let on = prepare(&adapter, &request).unwrap();
        assert_eq!(
            (skills_given(&on), on.argv.len()),
            (1, off.argv.len()),
            "{variant:?}: Pi finds its own, and is told of no other"
        );
        assert!(!on.argv.iter().any(|arg| arg.contains("specmint")));
    }
    let skills = path::join(&[&home.launch_folder(), "role", ".claude", "skills"]);
    assert_eq!(
        fs::read_dir(&skills).unwrap().count(),
        1,
        "the role's alone"
    );
    let before = (fs::read(&own).unwrap(), fs::read(&shared).unwrap());
    let request = Request {
        specs: Some(Variant::Core),
        ..chief()
    };
    prepare(&adapter, &request).unwrap();
    assert_eq!(fs::read_dir(&skills).unwrap().count(), 2);
    assert_eq!(
        (fs::read(&own).unwrap(), fs::read(&shared).unwrap()),
        before
    );
}
