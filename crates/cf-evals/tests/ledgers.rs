//! The frozen ledgers a recording of a real run holds the folder its project worked
//! in and the listings its windows made, so the home folder and the user of the
//! machine that made it: neither is a fixture's to carry. A recording names the
//! home folder `/home/user` and the user `user` (`tests/fixtures/ledgers/README.md`).

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// Every file of the folder of recordings, by name, with its text.
fn recordings() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("ledgers");
    let mut files: Vec<(String, String)> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// A user's name: words, hyphens and dots between them, so the quote or the stop
/// after it is not part of it.
const NAME: &str = r"([\w-]+(?:\.[\w-]+)*)";

/// The patterns that find a person's home folder, on Mac and Linux and on Windows.
fn homes() -> &'static [Regex; 2] {
    static HOMES: OnceLock<[Regex; 2]> = OnceLock::new();
    HOMES.get_or_init(|| {
        [
            Regex::new(&format!("/(?:Users|home)/{NAME}")).unwrap(),
            Regex::new(&format!(r"[A-Za-z]:[\\/]+Users[\\/]+{NAME}")).unwrap(),
        ]
    })
}

/// The owner column of an `ls -l` listing: `drwxr-xr-x@ 3 owner  group   96 Oct  8 15:31 name`.
fn owners() -> &'static Regex {
    static OWNER: OnceLock<Regex> = OnceLock::new();
    OWNER.get_or_init(|| {
        Regex::new(r"(?m)(?:^|[\s'])[-dlcbps][-rwxsStT]{9}[@+.]? +\d+ +(\S+) +\S+ +\d").unwrap()
    })
}

/// What `pattern` finds in a text, by its first group.
fn found(text: &str, pattern: &Regex) -> Vec<String> {
    pattern
        .captures_iter(text)
        .map(|hit| hit[1].to_owned())
        .collect()
}

fn homes_found(text: &str) -> Vec<String> {
    homes()
        .iter()
        .flat_map(|pattern| found(text, pattern))
        .collect()
}

#[test]
fn there_are_recordings_to_hold_and_one_of_a_real_run_is_among_them() {
    let files = recordings();
    assert!(files.len() >= 20, "{} files", files.len());
    assert!(files.iter().any(|(name, _)| name == "eval-round-trip.sql"));
}

#[test]
fn the_frozen_ledgers_say_nothing_of_who_made_them_the_home_folder_is_home_user_and_the_user_is_user(
) {
    for (name, text) in recordings() {
        let people: Vec<String> = homes_found(&text)
            .into_iter()
            .filter(|owner| owner != "user")
            .collect();
        assert!(
            people.is_empty(),
            "{name} holds the home folder of a person: {people:?}"
        );
        let listed: Vec<String> = found(&text, owners())
            .into_iter()
            .filter(|owner| owner != "user")
            .collect();
        assert!(
            listed.is_empty(),
            "{name} holds a listing made by a person: {listed:?}"
        );
    }
}

#[test]
fn the_frozen_ledgers_are_held_to_that_by_a_pattern_that_finds_a_person_a_home_folder_and_a_listing(
) {
    let real = r"'/Users/someone/work', 'C:\Users\someone\work', '/home/someone/x'";
    assert_eq!(
        homes_found(real),
        ["someone", "someone", "someone"].map(str::to_owned)
    );
    let listing = "'\ndrwxr-xr-x@ 3 someone  staff   96 Oct  8 15:31 content\n-rw-r--r-- 1 user  staff  1.0K Oct  8 15:31 a'";
    assert_eq!(
        found(listing, owners()),
        ["someone", "user"].map(str::to_owned)
    );
}
