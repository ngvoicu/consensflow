//! What puts the files back: whatever the runs came to.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, SystemTime};

use super::*;

#[test]
fn a_plant_is_put_back_when_a_run_ends_in_an_error_and_its_copies_are_kept() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(|_| {
        Err(Error::Files {
            what: "read what was written by",
            path: PathBuf::from("first"),
            cause: std::io::Error::other("the pipe broke"),
        })
    });
    let chosen: Vec<&Plant> = family.plants.iter().collect();
    let mut out = Vec::new();
    let status = run(&context, &family, &chosen, &mut runner, &mut out);
    assert!(status.is_err());
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    // Nothing ended well: the folder the run said first holds what was copied.
    let folder = copies_folder(&String::from_utf8(out).unwrap());
    assert_eq!(
        fs::read_to_string(folder.join("0-src_a.rs")).unwrap(),
        "one\ntwo\n"
    );
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_plant_is_put_back_when_a_run_panics() {
    let (_folder, context) = checkout();
    let family = family(vec![plant(
        "p: two files",
        &[("src/a.rs", "one", "ONE"), ("src/b.rs", "three", "THREE")],
        &[("first", false)],
    )]);
    let mut runner = Fake::new(|_| {
        assert_eq!(text(&context, "src/b.rs"), "THREE\n", "planted when it ran");
        panic!("a run that goes wrong");
    });
    let chosen: Vec<&Plant> = family.plants.iter().collect();
    let mut out = Vec::new();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        run(&context, &family, &chosen, &mut runner, &mut out)
    }));
    assert!(caught.is_err());
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    assert_eq!(text(&context, "src/b.rs"), "three\n");
    let folder = copies_folder(&String::from_utf8(out).unwrap());
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_file_that_is_not_as_it_was_after_it_is_put_back_is_said_with_where_its_copy_is() {
    let (_folder, context) = checkout();
    let copy = context.path("copy");
    fs::write(&copy, "not what it was").unwrap();
    fs::write(context.path("src/a.rs"), "ONE\ntwo\n").unwrap();
    let planting = Planting {
        copies: vec![Copy {
            path: context.path("src/a.rs"),
            copy: copy.clone(),
            original: b"one\ntwo\n".to_vec(),
            planted: "ONE\ntwo\n".into(),
        }],
    };
    let said = planting.restore().unwrap_err();
    assert_eq!(
        said.to_string(),
        format!(
            "{} is not as it was: its copy is {}",
            context.path("src/a.rs").display(),
            copy.display()
        )
    );
    // What it was is not in the copy, so what is written there is all there is.
    assert_eq!(text(&context, "src/a.rs"), "not what it was");
}

#[test]
fn a_file_that_cannot_be_put_back_leaves_none_of_the_others_planted_and_each_is_said() {
    let (_folder, context) = checkout();
    fs::write(context.path("src/c.rs"), "five\n").unwrap();
    let saved = tempfile::tempdir().unwrap();
    let three = plant(
        "p",
        &[
            ("src/a.rs", "one", "1"),
            ("src/b.rs", "three", "3"),
            ("src/c.rs", "five", "5"),
        ],
        &[("x", false)],
    );
    let planting = Planting::make(saved.path(), three.planted(&context).unwrap()).unwrap();
    // A run made the first file a folder: it cannot be written, and the others can.
    fs::remove_file(context.path("src/a.rs")).unwrap();
    fs::create_dir(context.path("src/a.rs")).unwrap();
    let said = planting.restore().unwrap_err().to_string();
    assert!(
        said.starts_with(&format!(
            "{} is still planted: put it back from its copy, {} (",
            context.path("src/a.rs").display(),
            saved.path().join("0-src_a.rs").display()
        )),
        "{said}"
    );
    assert_eq!(text(&context, "src/b.rs"), "three\n");
    assert_eq!(text(&context, "src/c.rs"), "five\n");
    // Two that cannot: both are said.
    fs::write(context.path("src/b.rs"), "3\n").unwrap();
    fs::remove_file(context.path("src/c.rs")).unwrap();
    fs::create_dir(context.path("src/c.rs")).unwrap();
    let said = planting.restore().unwrap_err().to_string();
    assert!(
        said.starts_with("2 files of a plant are not as they were:\n"),
        "{said}"
    );
    for (path, copy) in [("src/a.rs", "0-src_a.rs"), ("src/c.rs", "2-src_c.rs")] {
        assert!(
            said.contains(&format!(
                "{} is still planted: put it back from its copy, {} (",
                context.path(path).display(),
                saved.path().join(copy).display()
            )),
            "{said}"
        );
    }
    assert_eq!(text(&context, "src/b.rs"), "three\n");
    // Once it can be, dropped, the plant is put back whole.
    fs::remove_dir(context.path("src/a.rs")).unwrap();
    fs::remove_dir(context.path("src/c.rs")).unwrap();
    drop(planting);
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    assert_eq!(text(&context, "src/c.rs"), "five\n");
}

#[test]
fn what_a_run_without_the_plant_writes_into_a_file_it_touches_is_put_back_at_the_end() {
    // A command that, run without the plant to confirm a catch, writes the planted file
    // again (a fixture it makes, from what it holds): the plant's end puts it back.
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("regenerates", false)])]);
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            fails("m::the_test")
        } else {
            fs::write(context.path("src/a.rs"), "made again\n").unwrap();
            passes()
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 0, "{said}");
    assert_eq!(runner.ran, ["regenerates", "regenerates"]);
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
}

#[test]
fn a_stop_during_a_run_ends_the_run_with_130_after_putting_the_plant_back_and_before_the_next() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        plant(
            "p: two",
            &[("src/b.rs", "three", "THREE")],
            &[("second", false)],
        ),
        upper("p: three", &[("third", false)]),
    ]);
    let mut seen = Vec::new();
    let mut runner = Fake::new(|command| {
        seen.push(text(&context, "src/b.rs"));
        match command[0].as_str() {
            "second" => Ok(Execution::Stopped),
            _ => catches(&context, "m::the_test"),
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    assert_eq!(runner.ran, ["first", "first", "second"]);
    assert_eq!(seen, ["three\n", "three\n", "THREE\n"]);
    assert_eq!(text(&context, "src/b.rs"), "three\n");
    assert!(said.contains("CAUGHT           p: one ("), "{said}");
    assert!(
        said.ends_with("stopped: every file is as it was\n"),
        "{said}"
    );
    assert!(!copies_folder(&said).exists());
}

#[test]
fn a_stop_that_comes_between_plants_ends_the_run_before_the_next_is_planted() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        upper("p: two", &[("second", false)]),
    ]);
    let mut runner = Fake::new(|_| catches(&context, "m::the_test"));
    runner.stops = VecDeque::from([false, true]);
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    assert_eq!(runner.ran, ["first"; 2]);
    assert!(!said.contains("p: two"), "{said}");
}

#[test]
fn a_stop_during_the_run_without_the_plant_ends_the_run_with_130_and_every_file_back() {
    let (_folder, context) = checkout();
    let family = family(vec![
        upper("p: one", &[("first", false)]),
        upper("p: two", &[("second", false)]),
    ]);
    let mut runner = Fake::new(|_| {
        if planted(&context) {
            fails("m::the_test")
        } else {
            Ok(Execution::Stopped)
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    assert_eq!(runner.ran, ["first", "first"]);
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    assert!(!said.contains("CAUGHT"), "{said}");
    assert!(
        said.ends_with("stopped: every file is as it was\n"),
        "{said}"
    );
}

#[test]
fn every_file_of_a_plant_is_copied_in_the_order_it_names_them_and_every_one_goes_back() {
    let (_folder, context) = checkout();
    let saved = tempfile::tempdir().unwrap();
    let family = plant(
        "p",
        &[
            ("src/b.rs", "three", "3"),
            ("src/a.rs", "one", "1"),
            ("src/b.rs", "3", "III"),
        ],
        &[("x", false)],
    );
    let planting = Planting::make(saved.path(), family.planted(&context).unwrap()).unwrap();
    assert_eq!(text(&context, "src/b.rs"), "III\n");
    assert_eq!(text(&context, "src/a.rs"), "1\ntwo\n");
    assert_eq!(
        fs::read_to_string(saved.path().join("0-src_b.rs")).unwrap(),
        "three\n"
    );
    assert_eq!(
        fs::read_to_string(saved.path().join("1-src_a.rs")).unwrap(),
        "one\ntwo\n"
    );
    planting.restore().unwrap();
    assert_eq!(text(&context, "src/b.rs"), "three\n");
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
}

#[test]
fn a_file_put_back_is_no_older_than_the_plant_made_in_it_so_that_what_was_built_of_it_is_built_again(
) {
    // A source is as old as the checkout it came with. A system that copies a file's times
    // with it (macOS and Windows do) would give it back that old, older than whatever cargo
    // built from the plant, and cargo would take the planted build for current.
    let (_folder, context) = checkout();
    let path = context.path("src/a.rs");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let saved = tempfile::tempdir().unwrap();
    let plant = plant("p", &[("src/a.rs", "one", "1")], &[("x", false)]);
    let planting = Planting::make(saved.path(), plant.planted(&context).unwrap()).unwrap();
    let planted = fs::metadata(&path).unwrap().modified().unwrap();
    planting.restore().unwrap();
    let restored = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        planted > old,
        "the plant is as new as the write that made it"
    );
    assert!(
        restored >= planted,
        "{restored:?} is older than {planted:?}"
    );
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
}

#[test]
fn a_run_left_running_still_puts_its_plant_back_and_one_it_cannot_ends_the_run_naming_it_and_its_copy(
) {
    // Both reviewers' look at round 7: a run whose program left a process running ended
    // the plant's trial before its files were put back, and what then failed to put one
    // back (a process of it still holding the file) was dropped unsaid.
    let (_folder, context) = checkout();
    let left = |command: &[String]| Error::LeftRunning {
        command: command.join(" "),
        said: "what the program started did not end: process 7 is left".into(),
    };
    // Put back, the run's error is the run's end.
    let one = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(|command| Err(left(command)));
    let (status, said) = go(&context, &one, &mut runner);
    assert_eq!(
        status.unwrap_err().to_string(),
        "first: what the program started did not end: process 7 is left"
    );
    assert_eq!(text(&context, "src/a.rs"), "one\ntwo\n");
    fs::remove_dir_all(copies_folder(&said)).unwrap();

    // What the run left holds the file (here it made it a folder): the run ends with an
    // error that names it, still planted, and where its copy is, beside the run's own.
    let two = family(vec![plant(
        "p: two files",
        &[("src/a.rs", "one", "ONE"), ("src/b.rs", "three", "THREE")],
        &[("first", false)],
    )]);
    let mut runner = Fake::new(|command| {
        fs::remove_file(context.path("src/a.rs")).unwrap();
        fs::create_dir(context.path("src/a.rs")).unwrap();
        Err(left(command))
    });
    let (status, said) = go(&context, &two, &mut runner);
    let error = status.unwrap_err().to_string();
    let copy = copies_folder(&said).join("0-src_a.rs");
    assert!(
        error.starts_with(&format!(
            "{} is still planted: put it back from its copy, {} (",
            context.path("src/a.rs").display(),
            copy.display()
        )),
        "{error}"
    );
    assert!(
        error.ends_with(
            "\nand before that: first: what the program started did not end: process 7 is left"
        ),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&copy).unwrap(), "one\ntwo\n");
    assert_eq!(text(&context, "src/b.rs"), "three\n", "the other is back");
    fs::remove_dir(context.path("src/a.rs")).unwrap();
    fs::write(context.path("src/a.rs"), "one\ntwo\n").unwrap();
    fs::remove_dir_all(copies_folder(&said)).unwrap();

    // A file that could not be put back is what ended the trial: it is said once, as it
    // is at the end.
    let three = family(vec![upper("p: three", &[("first", false)])]);
    let mut runner = Fake::new(|_| {
        fs::remove_file(context.path("src/a.rs")).unwrap();
        fs::create_dir(context.path("src/a.rs")).unwrap();
        fails("m::the_test")
    });
    let (status, said) = go(&context, &three, &mut runner);
    let error = status.unwrap_err().to_string();
    let copy = copies_folder(&said).join("0-src_a.rs");
    assert!(
        error.starts_with(&format!(
            "{} is still planted: put it back from its copy, {} (",
            context.path("src/a.rs").display(),
            copy.display()
        )) && !error.contains("\n"),
        "{error}"
    );
    fs::remove_dir(context.path("src/a.rs")).unwrap();
    fs::remove_dir_all(copies_folder(&said)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_plant_that_cannot_be_made_whole_names_a_file_it_could_not_put_back_and_its_copy() {
    // Calliope's look at round 8: a plant of two files whose second write failed was
    // put back by the drop alone, whose error went unsaid. Here the second file's copy
    // is written over the first's (its path leads there), and the second file cannot be
    // written: the first, put back from a copy that is not its own, is not as it was.
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (_folder, context) = checkout();
    let saved = tempfile::tempdir().unwrap();
    let two = plant(
        "p",
        &[("src/a.rs", "one", "1"), ("src/b.rs", "three", "3")],
        &[("x", false)],
    );
    let files = two.planted(&context).unwrap();
    symlink(
        saved.path().join("0-src_a.rs"),
        saved.path().join("1-src_b.rs"),
    )
    .unwrap();
    let b = context.path("src/b.rs");
    fs::set_permissions(&b, fs::Permissions::from_mode(0o444)).unwrap();
    let said = Planting::make(saved.path(), files)
        .err()
        .unwrap()
        .to_string();
    fs::set_permissions(&b, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        said.starts_with(&format!(
            "{} is not as it was: its copy is {}\nand before that: could not write {}: ",
            context.path("src/a.rs").display(),
            saved.path().join("0-src_a.rs").display(),
            b.display()
        )),
        "{said}"
    );
    assert_eq!(text(&context, "src/b.rs"), "three\n", "never planted");
}
