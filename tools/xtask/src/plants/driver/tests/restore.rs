//! What puts the files back: whatever the runs came to.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, SystemTime};

use super::*;
use crate::process;

#[test]
fn a_plant_is_put_back_when_a_command_cannot_be_started_and_its_copies_are_kept() {
    let (_folder, context) = checkout();
    let family = family(vec![upper("p: one", &[("first", false)])]);
    let mut runner = Fake::new(|_| {
        Err(Error::Process(process::Failure::NotFound {
            program: "cargo".into(),
        }))
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
    let mut planting = Planting {
        copies: vec![Copy {
            path: context.path("src/a.rs"),
            copy: copy.clone(),
            original: b"one\ntwo\n".to_vec(),
        }],
        restored: false,
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
    assert!(!planting.restored);
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
            _ => fails("m::the_test"),
        }
    });
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    assert_eq!(runner.ran, ["first", "second"]);
    assert_eq!(seen, ["three\n", "THREE\n"]);
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
    let mut runner = Fake::new(|_| fails("m::the_test"));
    runner.stops = VecDeque::from([false, true]);
    let (status, said) = go(&context, &family, &mut runner);
    assert_eq!(status.unwrap(), 130);
    assert_eq!(runner.ran, ["first"]);
    assert!(!said.contains("p: two"), "{said}");
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
    let mut planting = Planting::make(saved.path(), family.planted(&context).unwrap()).unwrap();
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
    let mut planting = Planting::make(saved.path(), plant.planted(&context).unwrap()).unwrap();
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
