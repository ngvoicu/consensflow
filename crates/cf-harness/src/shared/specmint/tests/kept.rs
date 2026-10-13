//! What ConsensFlow's own copy of a variant is as a launch finds it and leaves
//! it: the same place for every launch, a file written only where it is missing
//! or differs, and kept when a launch's files go.

use super::*;

/// The copy of `variant` ConsensFlow keeps for Claude, written for a launch of a
/// chief in the home `fake` made: where its `SKILL.md` is.
fn written_for(fake: &Fake, id: &LaunchId, variant: Variant) -> OwnCopy {
    let env = home_env(fake);
    let skill = place(Harness::Claude, &env, variant, None).expect("a place");
    let specs = Specs {
        variant,
        skill: &skill,
    };
    write_own(Harness::Claude, &env, &launch(id, Some(specs)))
        .expect("written")
        .expect("a copy")
}

#[test]
fn the_copy_is_the_same_for_every_launch_and_a_file_that_is_what_the_build_ships_is_not_written_again(
) {
    let fake = Fake::new();
    let first = written_for(&fake, &id(), Variant::Tdd);
    let second_id = LaunchId::new("22222222-2222-4222-8222-222222222222").expect("a launch id");
    // Nothing of it may be written again: a file made read-only has no write
    // to take, and a launch that tried one would fail.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for (name, _) in files::of(Variant::Tdd) {
            let file = path::join(&[&first.root, ".claude", "skills", "specmint-tdd", name]);
            fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).expect("read-only");
        }
    }
    let second = written_for(&fake, &second_id, Variant::Tdd);
    assert_eq!(second, first, "the same place at another launch");
}

#[test]
fn a_file_that_is_missing_or_differs_is_written_again_and_what_else_is_there_is_left() {
    let fake = Fake::new();
    let copy = written_for(&fake, &id(), Variant::Core);
    let skill = |name: &str| path::join(&[&copy.root, ".claude", "skills", "specmint-core", name]);
    let shipped = |name: &str| {
        files::of(Variant::Core)
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, bytes)| bytes.to_vec())
            .expect("a shipped file")
    };
    fs::write(skill("SKILL.md"), "changed by hand\n").expect("a change");
    fs::remove_file(skill("references/researcher.md")).expect("a removal");
    fs::write(skill("notes-of-the-human.md"), "mine\n").expect("a file of their own");
    let again = written_for(&fake, &id(), Variant::Core);
    assert_eq!(again, copy);
    for name in ["SKILL.md", "references/researcher.md", "LICENSE"] {
        assert_eq!(
            fs::read(skill(name)).expect("a file"),
            shipped(name),
            "{name}"
        );
    }
    assert_eq!(
        fs::read_to_string(skill("notes-of-the-human.md")).expect("a file"),
        "mine\n",
        "a file the build does not ship is none of its business"
    );
}

#[test]
fn neither_a_launch_s_end_nor_a_start_s_sweep_removes_the_copy_and_each_variant_has_its_own() {
    let fake = Fake::new();
    let id = id();
    let tdd = written_for(&fake, &id, Variant::Tdd);
    let core = written_for(&fake, &id, Variant::Core);
    assert_ne!(tdd.root, core.root);
    // The launch's own files, as a window leaves them.
    let env = home_env(&fake);
    let launch_folder =
        crate::shared::launch_files::launch_folder("claude", &env, &id).expect("a launch's folder");
    fs::create_dir_all(&launch_folder).expect("a folder");
    fs::write(path::join(&[&launch_folder, "settings.json"]), "{}").expect("a file");
    crate::shared::launch_files::forget_launch(&fake.at("cf"), &id).expect("forgotten");
    assert!(!exists(&launch_folder), "the launch's files went");
    fs::create_dir_all(&launch_folder).expect("a folder");
    assert_eq!(
        crate::shared::launch_files::sweep_launches(&fake.at("cf")).expect("swept"),
        1,
        "a start sweeps the launch's folder, and nothing else"
    );
    for copy in [&tdd, &core] {
        assert!(exists(&copy.file()), "{} stays", copy.file());
    }
}
