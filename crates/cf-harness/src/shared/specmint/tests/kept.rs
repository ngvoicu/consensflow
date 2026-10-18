//! What ConsensFlow's own copy of a variant is as a launch finds it and leaves
//! it: the same place for every launch, a file written only where it is missing
//! or differs, and kept when a launch's files go.

use super::*;

/// The copy of `variant` ConsensFlow keeps for Claude, written for a launch of a
/// chief in the home `fake` made, or why it could not be.
fn write_for(fake: &Fake, id: &LaunchId, variant: Variant) -> Result<Option<OwnCopy>, String> {
    let env = home_env(fake);
    let skill = place(Harness::Claude, &env, variant, None).expect("a place");
    let specs = Specs {
        variant,
        skill: &skill,
    };
    write_own(Harness::Claude, &env, &launch(id, Some(specs)))
}

/// [`write_for`], written: where its `SKILL.md` is.
fn written_for(fake: &Fake, id: &LaunchId, variant: Variant) -> OwnCopy {
    write_for(fake, id, variant)
        .expect("written")
        .expect("a copy")
}

/// `root` and every folder below it, made so that nothing can be made in them.
#[cfg(unix)]
fn locked_folders(root: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let mut folders = vec![root.to_path_buf()];
    let mut at = 0;
    while at < folders.len() {
        for entry in fs::read_dir(&folders[at]).expect("a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                folders.push(path);
            }
        }
        at += 1;
    }
    for folder in &folders {
        fs::set_permissions(folder, fs::Permissions::from_mode(0o500)).expect("locked");
    }
    folders
}

/// The folders [`locked_folders`] locked, as they were.
#[cfg(unix)]
fn unlocked(folders: &[PathBuf]) {
    use std::os::unix::fs::PermissionsExt;
    for folder in folders {
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700)).expect("unlocked");
    }
}

#[test]
fn the_copy_is_the_same_for_every_launch_and_a_file_that_is_what_the_build_ships_is_not_written_again(
) {
    let fake = Fake::new();
    let first = written_for(&fake, &id(), Variant::Tdd);
    let second_id = LaunchId::new("22222222-2222-4222-8222-222222222222").expect("a launch id");
    // Nothing of it may be written again: in a folder nothing can be made in, a
    // file has no write to take, in place or put there beside another, and a
    // launch that tried one would fail.
    #[cfg(unix)]
    let locked = locked_folders(Path::new(&first.root));
    let second = write_for(&fake, &second_id, Variant::Tdd);
    #[cfg(unix)]
    unlocked(&locked);
    assert_eq!(
        second.expect("written").expect("a copy"),
        first,
        "the same place at another launch"
    );
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

#[cfg(unix)]
#[test]
fn a_file_that_differs_is_replaced_and_not_written_in_place_so_a_reader_that_has_it_open_reads_the_file_it_opened_whole(
) {
    use std::io::Read;
    let fake = Fake::new();
    let copy = written_for(&fake, &id(), Variant::Core);
    fs::write(copy.file(), "changed by hand\n").expect("a change");
    // A window of another launch has the file open, and is reading it.
    let mut reading = fs::File::open(copy.file()).expect("an open file");

    let second_id = LaunchId::new("22222222-2222-4222-8222-222222222222").expect("a launch id");
    written_for(&fake, &second_id, Variant::Core);
    let mut held = String::new();
    reading.read_to_string(&mut held).expect("read");
    assert_eq!(
        held, "changed by hand\n",
        "a file rewritten in place would be read as it is half way through"
    );
    let shipped = files::of(Variant::Core)
        .iter()
        .find(|(name, _)| *name == "SKILL.md")
        .map(|(_, bytes)| bytes.to_vec())
        .expect("a shipped file");
    assert_eq!(fs::read(copy.file()).expect("the file"), shipped);
}

/// The names of the files in `folder` that end `.tmp`.
fn temporaries_in(folder: &str) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(folder)
        .expect("a folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    found.sort();
    found
}

#[test]
fn a_write_that_cannot_be_put_in_place_leaves_no_temporary_and_what_was_there() {
    let fake = Fake::new();
    let copy = written_for(&fake, &id(), Variant::Core);
    let folder = path::join(&[&copy.root, ".claude", "skills", "specmint-core"]);
    // A folder where the file goes: nothing can be renamed over it.
    fs::remove_file(copy.file()).expect("a removal");
    fs::create_dir(copy.file()).expect("a folder in the way");
    let env = home_env(&fake);
    let skill = place(Harness::Claude, &env, Variant::Core, None).expect("a place");
    let specs = Specs {
        variant: Variant::Core,
        skill: &skill,
    };
    let failed = write_own(Harness::Claude, &env, &launch(&id(), Some(specs)))
        .expect_err("a folder is no file");
    assert!(failed.contains("rename"), "{failed}");
    assert_eq!(temporaries_in(&folder), Vec::<String>::new());
    assert!(
        Path::new(&copy.file()).is_dir(),
        "what was there is as it was"
    );
}

#[test]
fn a_write_is_made_to_a_name_of_the_launch_s_own_and_another_launch_s_is_left_alone() {
    let fake = Fake::new();
    let copy = written_for(&fake, &id(), Variant::Core);
    let folder = path::join(&[&copy.root, ".claude", "skills", "specmint-core"]);
    // Another launch is in the middle of writing the same file.
    let other = format!("{}.33333333-3333-4333-8333-333333333333.tmp", copy.file());
    fs::write(&other, "half of the other's").expect("the other's temporary");
    fs::write(copy.file(), "changed by hand\n").expect("a change");

    // The name this launch writes to is its own: where it is taken (by a folder,
    // which nothing is written to), the write says so, as no other name would.
    let mine = format!("{}.{}.tmp", copy.file(), id().as_str());
    fs::create_dir(&mine).expect("a folder at the launch's name");
    let env = home_env(&fake);
    let skill = place(Harness::Claude, &env, Variant::Core, None).expect("a place");
    let specs = Specs {
        variant: Variant::Core,
        skill: &skill,
    };
    write_own(Harness::Claude, &env, &launch(&id(), Some(specs)))
        .expect_err("the launch's own name is taken");
    assert_eq!(
        fs::read_to_string(copy.file()).expect("the file"),
        "changed by hand\n",
        "what was there is as it was"
    );
    fs::remove_dir(&mine).expect("the folder goes");

    written_for(&fake, &id(), Variant::Core);
    assert_eq!(
        fs::read_to_string(&other).expect("the other's temporary"),
        "half of the other's",
        "no launch writes to another's name, nor removes it"
    );
    assert_eq!(
        temporaries_in(&folder),
        [Path::new(&other)
            .file_name()
            .expect("its name")
            .to_string_lossy()
            .into_owned()],
        "and none of this launch's own is left"
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
