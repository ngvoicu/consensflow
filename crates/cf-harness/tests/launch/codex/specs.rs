//! The Specmint skill a Codex chief is given where its project has specs on:
//! written in the launch's own folder, which Codex has for nothing else (it
//! keeps no role there), and read by its path, which the role text names; or the
//! human's own, where Codex has the variant installed, and then nothing is written.

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_harness::contract::{Adapter, LaunchId, Skill};
use cf_harness::testing::unrooted;
use cf_proto::specs::Variant;

use super::{adapter, prepare, Home, Request, LAUNCH};

/// Where the launch's own files for Codex are: it keeps no role there, and
/// keeps the Specmint skill of a chief given specs.
fn launch_folder(home: &Home) -> String {
    path::join(&[
        &home.root,
        "consensflow",
        "integrations",
        "codex",
        LAUNCH,
        "role",
        ".claude",
        "skills",
    ])
}

#[test]
fn a_chief_given_specs_has_the_skill_written_where_its_role_text_names_it_and_codex_is_told_nothing_more(
) {
    for variant in Variant::ALL {
        // Two homes of one make: what differs between the launches is the specs.
        let (off_home, home) = (Home::new(), Home::new());
        let (off_adapter, _fakes) = adapter(&off_home);
        let (adapter, _fakes) = adapter(&home);
        let off = prepare(&off_adapter, &Request::chief()).unwrap();
        assert!(
            !Path::new(&launch_folder(&off_home)).exists(),
            "with specs off Codex has no folder of its own for the launch"
        );
        let request = Request {
            specs: Some(variant),
            ..Request::chief()
        };
        let on = prepare(&adapter, &request).unwrap();
        assert_eq!(
            unrooted(&on, &home.root),
            unrooted(&off, &off_home.root),
            "{variant:?}: Codex reads it by its path, and is registered nothing"
        );
        let skill = path::join(&[&launch_folder(&home), variant.skill_name(), "SKILL.md"]);
        assert_eq!(
            adapter
                .specmint(variant, &LaunchId::new(LAUNCH).unwrap())
                .unwrap(),
            Skill::Own(skill.clone()),
            "{variant:?}: the place the role text names is the place written"
        );
        assert!(
            fs::read_to_string(&skill)
                .unwrap()
                .starts_with(&format!("---\nname: {}\n", variant.skill_name())),
            "{variant:?}"
        );
        assert_eq!(
            fs::read_dir(launch_folder(&home)).unwrap().count(),
            1,
            "the skill alone: Codex is given its role as text"
        );
    }
}

#[test]
fn a_codex_that_has_the_variant_installed_in_the_agents_skills_or_its_own_is_given_nothing_to_write_and_another_variant_installed_is_left_alone(
) {
    let home = Home::new();
    let install = |folder: &str, name: &str| {
        let dir = path::join(&[&home.root, "home", folder, name]);
        fs::create_dir_all(&dir).unwrap();
        let file = path::join(&[&dir, "SKILL.md"]);
        fs::write(&file, format!("---\nname: {name}\n---\nthe human's own\n")).unwrap();
        file
    };
    let agents = install(".agents/skills", "specmint-tdd");
    let own = install(".codex/skills", "specmint-core-html");
    let (adapter, _fakes) = adapter(&home);
    let id = LaunchId::new(LAUNCH).unwrap();
    assert_eq!(
        adapter.specmint(Variant::Tdd, &id).unwrap(),
        Skill::Global(agents.clone())
    );
    assert_eq!(
        adapter.specmint(Variant::CoreHtml, &id).unwrap(),
        Skill::Global(own.clone())
    );
    for variant in [Variant::Tdd, Variant::CoreHtml] {
        let request = Request {
            specs: Some(variant),
            ..Request::chief()
        };
        prepare(&adapter, &request).unwrap();
    }
    assert!(
        !Path::new(&launch_folder(&home)).exists(),
        "an installed copy is the harness's own: nothing is written"
    );
    // Neither installed variant is the core one: ConsensFlow's own is written.
    assert!(matches!(
        adapter.specmint(Variant::Core, &id).unwrap(),
        Skill::Own(_)
    ));
    let (before_agents, before_own) = (fs::read(&agents).unwrap(), fs::read(&own).unwrap());
    let request = Request {
        specs: Some(Variant::Core),
        ..Request::chief()
    };
    prepare(&adapter, &request).unwrap();
    assert!(Path::new(&launch_folder(&home))
        .join("specmint-core")
        .exists());
    assert_eq!(fs::read(&agents).unwrap(), before_agents);
    assert_eq!(fs::read(&own).unwrap(), before_own);
}
