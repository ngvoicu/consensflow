//! The Specmint skill a Claude chief is given where its project has specs on:
//! ConsensFlow's own copy, kept where it is the same for every launch and given
//! to Claude as one more folder to read (a second `--add-dir`) so that the skill
//! loads with the role's, and nothing else of the launch moves; or the human's
//! own, where Claude has the variant installed, and then none is written.

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_harness::contract::{Adapter, Skill};
use cf_proto::specs::Variant;

use super::{prepare, Home, Request};
use crate::kept;

/// The names in the skills folder of a launch's role: the role's own skill,
/// and nothing of Specmint, which is kept elsewhere.
fn role_skills(home: &Home) -> Vec<String> {
    kept::names_in(&path::join(&[
        &home.launch_folder(),
        "role",
        ".claude",
        "skills",
    ]))
}

#[test]
fn a_chief_given_specs_has_the_skill_kept_for_every_launch_and_claude_is_given_that_folder_as_one_more_and_nothing_else_of_its_launch_moves(
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
        let root = kept::root(home.var("CONSENSFLOW_HOME"), "claude", variant);
        let at = off.argv.iter().position(|arg| arg == "--add-dir").unwrap() + 2;
        let mut expected = off.argv.clone();
        expected.splice(at..at, ["--add-dir".to_owned(), root.clone()]);
        assert_eq!(
            on.argv, expected,
            "{variant:?}: the role's folder, then the folder the copy is kept in"
        );
        assert_eq!((on.env, on.drop_env), (off.env, off.drop_env));
        assert_eq!(
            role_skills(&home),
            ["consensflow-chief"],
            "{variant:?}: the role's folder is the role's alone"
        );
        let skill = kept::skill(home.var("CONSENSFLOW_HOME"), "claude", variant);
        assert_eq!(
            kept::names_in(&path::join(&[&root, ".claude", "skills"])),
            [variant.skill_name()],
            "{variant:?}: Claude finds the skill under the folder's own `.claude/skills`"
        );
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
        assert_eq!(
            home.adapter().specmint(variant).unwrap(),
            Skill::Own(path::join(&[&skill, "SKILL.md"])),
            "{variant:?}: the place the role text names is the place written"
        );
    }
}

#[test]
fn a_second_launch_of_the_chief_is_given_the_same_folder_and_writes_nothing_of_it_again() {
    let home = Home::new();
    let request = Request {
        specs: Some(Variant::Tdd),
        ..Request::chief()
    };
    let first = prepare(&home.adapter(), &request).unwrap();
    let root = kept::root(home.var("CONSENSFLOW_HOME"), "claude", Variant::Tdd);
    // Nothing is to be written: a file made read-only has no write to take.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let skill = kept::skill(home.var("CONSENSFLOW_HOME"), "claude", Variant::Tdd);
        let file = path::join(&[&skill, "SKILL.md"]);
        fs::set_permissions(file, fs::Permissions::from_mode(0o400)).unwrap();
    }
    let second = prepare(&home.adapter(), &request).unwrap();
    assert_eq!(second.argv, first.argv);
    assert!(second.argv.contains(&root));
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
    // The variant the project chose is installed: that copy is the window's.
    assert_eq!(
        adapter.specmint(Variant::Tdd).unwrap(),
        Skill::Global(tdd.clone())
    );
    // An adapter of its own for each launch: the session it names is its own too.
    let off = prepare(&home.adapter(), &Request::chief()).unwrap();
    let request = Request {
        specs: Some(Variant::Tdd),
        ..Request::chief()
    };
    let on = prepare(&home.adapter(), &request).unwrap();
    assert_eq!(on.argv, off.argv, "Claude is told no more than it was");
    assert!(
        !Path::new(home.var("CONSENSFLOW_HOME"))
            .join("integrations")
            .join("specmint")
            .exists(),
        "nothing of ConsensFlow's is written"
    );
    // Another variant is chosen: the installed one is no copy of it, and is not touched.
    let request = Request {
        specs: Some(Variant::Core),
        ..Request::chief()
    };
    let before = fs::read(&tdd).unwrap();
    assert!(matches!(
        adapter.specmint(Variant::Core).unwrap(),
        Skill::Own(_)
    ));
    let own = prepare(&home.adapter(), &request).unwrap();
    assert_eq!(own.argv.len(), off.argv.len() + 2);
    assert_eq!(
        kept::names_in(&kept::skills(
            home.var("CONSENSFLOW_HOME"),
            "claude",
            Variant::Core
        )),
        ["specmint-core"]
    );
    assert_eq!(
        fs::read(&tdd).unwrap(),
        before,
        "the human's own is as it was"
    );
}
