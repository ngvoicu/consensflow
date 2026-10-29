//! `A removed harness is named nowhere`: no file of the repository names the harness the removal took out
//! (RMOC-21 of the removal, T-570), whatever the case it is written in, in a text, in a name, or
//! inside a recording (`*.json.gz`, which a plain search does not read), but in the two places the
//! removal keeps it on purpose ([`KEPT`]).
//!
//! The check is a function of a tree, so the tests plant what it is for (the word in a text, in
//! another case, in a recording and in a file's name; the word where it is kept) in a tree of
//! their own and see each one found, or not.

use std::io::{Read, Write};
use std::path::Path;

use cf_e2e::{checkout, files};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use super::off_node::{tree, under, Found};
use crate::Outcome;

/// The removed harness's name, written in halves so that this file does not name it.
fn word() -> String {
    ["open", "code"].concat()
}

/// The only files that may name it, and why.
const KEPT: [&str; 2] = [
    // The eval history, as the removal's Decision Log keeps it: the only shared copy of it.
    "evals/RESULTS.md",
    // The field's own word: the ledger a 3.0.6 install holds names the harness it ran, and the
    // regression test of an install whose harness this build does not run holds that shape.
    "crates/cf-e2e/tests/regressions/a_ledger_of_a_harness_this_build_does_not_run.rs",
];

/// Whether `bytes` name the word, in any case.
fn names_it(bytes: &[u8]) -> bool {
    let word = word().into_bytes();
    bytes
        .to_ascii_lowercase()
        .windows(word.len())
        .any(|at| at == word)
}

/// The `listed` files of the tree `root` that name the word, in their names or their contents (a
/// recording read unzipped), but for [`KEPT`].
fn named(root: &Path, listed: &[String]) -> Found<Vec<String>> {
    let mut found = Vec::new();
    for file in listed {
        if KEPT.contains(&file.as_str()) {
            continue;
        }
        if names_it(file.as_bytes()) {
            found.push(file.clone());
            continue;
        }
        let path = under(root, file);
        // A link is listed as the link and not read through (the commit gate's copy holds some).
        if !checkout::is_plain_file(&path) {
            continue;
        }
        let mut bytes = files::read(&path)?;
        if file.ends_with(".gz") {
            let mut unzipped = Vec::new();
            GzDecoder::new(bytes.as_slice()).read_to_end(&mut unzipped)?;
            bytes = unzipped;
        }
        if names_it(&bytes) {
            found.push(file.clone());
        }
    }
    Ok(found)
}

#[test]
fn names_it_nowhere_in_the_repository_but_in_the_two_places_that_keep_it() -> Outcome {
    let root = checkout::root();
    assert_eq!(
        named(&root, &tree(&root)?)?,
        Vec::<String>::new(),
        "the harness is gone: say what a file did in words that do not name it"
    );
    Ok(())
}

#[test]
fn keeps_the_word_only_where_it_still_is() -> Outcome {
    let root = checkout::root();
    for file in KEPT {
        let bytes = files::read(&under(&root, file))?;
        assert!(
            names_it(&bytes),
            "{file} no longer names it: take it off KEPT"
        );
    }
    Ok(())
}

mod what_the_check_is_for_planted_in_a_tree_of_its_own {
    use super::*;

    /// A recording of `text`, as the repository's are written: gzipped JSON.
    fn zipped(text: &str) -> Found<Vec<u8>> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(text.as_bytes())?;
        Ok(gz.finish()?)
    }

    /// A tree of the `planted` files, as text, and the list of them.
    fn on_tree(planted: &[(&str, Vec<u8>)]) -> Found<(tempfile::TempDir, Vec<String>)> {
        let root = tempfile::tempdir()?;
        for (file, contents) in planted {
            files::write(&under(root.path(), file), contents)?;
        }
        Ok((
            root,
            planted.iter().map(|(file, _)| (*file).to_owned()).collect(),
        ))
    }

    #[test]
    fn finds_nothing_in_a_tree_that_has_none_of_it() -> Outcome {
        let (root, listed) = on_tree(&[
            ("README.md", b"Claude Code, Codex, Pi and Devin\n".to_vec()),
            ("traces/a.json.gz", zipped("{\"harness\":\"pi\"}")?),
        ])?;
        assert_eq!(named(root.path(), &listed)?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn finds_it_in_a_text_in_any_case_in_a_recording_and_in_a_name() -> Outcome {
        let (upper, lower) = (word().to_uppercase(), word());
        let mixed = format!("{}{}", &upper[..4], &lower[4..]);
        let (root, listed) = on_tree(&[
            ("a/readme.md", format!("runs on {mixed}\n").into_bytes()),
            ("b/shout.txt", format!("{upper}\n").into_bytes()),
            (
                "c/trace.json.gz",
                zipped(&format!("{{\"harness\":\"{lower}\"}}"))?,
            ),
            (format!("d/{lower}.md").as_str(), Vec::new()),
            ("e/clean.md", b"Pi and Devin\n".to_vec()),
        ])?;
        assert_eq!(
            named(root.path(), &listed)?,
            [
                "a/readme.md".to_owned(),
                "b/shout.txt".to_owned(),
                "c/trace.json.gz".to_owned(),
                format!("d/{lower}.md"),
            ]
        );
        Ok(())
    }

    #[test]
    fn leaves_the_two_files_that_keep_it_alone() -> Outcome {
        let said = format!("{} 1.19\n", word());
        let planted: Vec<(&str, Vec<u8>)> = KEPT
            .iter()
            .map(|file| (*file, said.clone().into_bytes()))
            .collect();
        let (root, listed) = on_tree(&planted)?;
        assert_eq!(named(root.path(), &listed)?, Vec::<String>::new());
        Ok(())
    }
}
