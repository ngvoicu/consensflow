//! Names on Windows, which answers to a file by its long name in any case and
//! by its short (8.3) name: a name is read only where it is one of the long
//! names its folder lists, exactly as written, so neither a short name, given
//! by hand included, nor the same name in another case reads a file; and a
//! folder is listed at most once in a look, however many names are asked of it.

use super::super::super::door::Reading;
use super::*;

/// A path the page writes in another case than the folder's names is not
/// found, and says so: the page asks for the names the tree listed, and an
/// HTML file's assets are taken as it writes them.
#[test]
fn a_name_in_another_case_than_the_folder_s_is_not_found() {
    let project = project_with(&[
        ("assets/spec-styles.css", "p {}"),
        ("a/SPEC.html", "<link href=\"../assets/SPEC-STYLES.CSS\">"),
    ]);
    let folder = specs(&project);
    assert_eq!(
        read(&folder, "ASSETS/Spec-Styles.CSS").unwrap_err(),
        "no file ASSETS/Spec-Styles.CSS in the specs folder"
    );
    assert_eq!(
        read(&folder, "assets/SPEC-STYLES.CSS").unwrap_err(),
        "no file assets/SPEC-STYLES.CSS in the specs folder"
    );
    assert_eq!(
        read(&folder, "assets/spec-styles.css").unwrap().content,
        Content::Text("p {}".to_owned())
    );
    let copied: Vec<Vec<String>> = folder
        .for_browser("a/SPEC.html", &|| {})
        .unwrap()
        .files
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(copied, [vec!["a".to_owned(), "SPEC.html".to_owned()]]);
}

/// Names are taken as written, not as any folding of them: `ß.css` is no
/// `SS.CSS`, which Windows keeps apart too.
#[test]
fn a_name_is_not_taken_for_another_that_folds_like_it() {
    let project = project_with(&[("assets/ß.css", "p {}")]);
    let folder = specs(&project);
    assert_eq!(
        read(&folder, "assets/ß.css").unwrap().content,
        Content::Text("p {}".to_owned())
    );
    assert_eq!(
        read(&folder, "assets/SS.CSS").unwrap_err(),
        "no file assets/SS.CSS in the specs folder"
    );
}

/// An HTML file naming sixteen assets the folder does not hold, in a specs
/// folder of many names: the opening lists that folder once, not once an
/// asset, and every name it lists is taken from the look's budget.
#[test]
fn an_opening_lists_a_folder_once_however_many_names_are_asked_of_it() {
    let names: Vec<String> = (0..2000).map(|at| format!("n{at:04}.md")).collect();
    let spec: String = (0..16)
        .map(|at| format!("<link href=\"../assets/missing-{at}.css\">"))
        .collect();
    let mut files: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "")).collect();
    files.push(("a/SPEC.html", spec.as_str()));
    let project = project_with(&files);
    let folder = specs(&project);
    let reading: &Reading = &folder.reading;
    let before = reading.listings();
    let copied = folder.for_browser("a/SPEC.html", &|| {}).unwrap();
    assert_eq!(copied.files.len(), 1, "no asset is there");
    assert_eq!(
        reading.listings() - before,
        2,
        "the specs folder once (2001 names), and its spec's folder once"
    );
    assert_eq!(
        reading.left(),
        MAX_SCANNED - 2001 - 1,
        "every name listed, charged"
    );
}

/// The viewer open, the chief writes a file and a folder of a new spec into
/// the specs folder: each is read, as on Unix, though the folders' long names
/// were listed before they were written.
#[test]
fn a_file_and_a_folder_written_while_the_folder_is_open_are_read() {
    let project = project_with(&[("a/SPEC.md", "# A")]);
    let folder = specs(&project);
    // Read once: the folder's names, and its spec's, are listed now.
    assert!(read(&folder, "a/SPEC.md").is_ok());
    let at = project.path().join(".specs");
    fs::write(at.join("a/research-01.md"), "# Research").unwrap();
    fs::write(at.join("notes.md"), "notes").unwrap();
    fs::create_dir_all(at.join("b")).unwrap();
    fs::write(at.join("b/SPEC.md"), "# B").unwrap();
    for (path, text) in [
        ("a/research-01.md", "# Research"),
        ("notes.md", "notes"),
        ("b/SPEC.md", "# B"),
    ] {
        assert_eq!(
            read(&folder, path).unwrap().content,
            Content::Text(text.to_owned()),
            "{path}"
        );
    }
    assert_eq!(
        read(&folder, "a/gone.md").unwrap_err(),
        "no file a/gone.md in the specs folder"
    );
}

/// A short name given by hand (`fsutil file setshortname`) is any name, with
/// no `~` and digit: `PUBLIC.CSS` for `.private.css`, or `SS.CSS` for
/// `.secret.css` beside an `ß.css`, which a folding of names would take it
/// for. Windows answers to it, and the viewer does not, neither read by the
/// page nor copied for the browser as an asset an HTML file names. Giving a
/// file a short name takes an administrator, and a volume that keeps short
/// names: run as one, with `cargo test -p cf-daemon --lib
/// short_name_given_by_hand -- --ignored`.
#[test]
#[ignore = "fsutil file setshortname takes an administrator's shell, on a volume with short names"]
#[allow(clippy::disallowed_methods)] // The test gives the file the short name it is about.
fn a_short_name_given_by_hand_is_no_name_the_folder_holds() {
    let project = project_with(&[
        ("assets/.private.css", "secret {}"),
        ("assets/.secret.css", "secret too {}"),
        ("assets/ß.css", "p {}"),
        (".hidden/notes.md", "kept out"),
        (
            "a/SPEC.html",
            "<link href=\"../assets/PUBLIC.CSS\"><link href=\"../assets/SS.CSS\">",
        ),
    ]);
    let specs_folder = project.path().join(".specs");
    for (long, short) in [
        ("assets/.private.css", "PUBLIC.CSS"),
        ("assets/.secret.css", "SS.CSS"),
        (".hidden", "OPEN"),
    ] {
        let given = std::process::Command::new("fsutil")
            .args(["file", "setshortname"])
            .arg(specs_folder.join(long))
            .arg(short)
            .output()
            .unwrap();
        assert!(
            given.status.success(),
            "fsutil file setshortname takes an administrator's shell and a volume with short \
             names: {}",
            String::from_utf8_lossy(&given.stdout)
        );
    }
    // Windows answers to the short names.
    assert_eq!(
        fs::read_to_string(specs_folder.join("assets/PUBLIC.CSS")).unwrap(),
        "secret {}"
    );
    assert_eq!(
        fs::read_to_string(specs_folder.join("OPEN/notes.md")).unwrap(),
        "kept out"
    );
    let folder = specs(&project);
    assert_eq!(
        fs::read_to_string(specs_folder.join("assets/SS.CSS")).unwrap(),
        "secret too {}"
    );
    assert_eq!(
        read(&folder, "assets/PUBLIC.CSS").unwrap_err(),
        "no file assets/PUBLIC.CSS in the specs folder"
    );
    assert_eq!(
        read(&folder, "assets/SS.CSS").unwrap_err(),
        "no file assets/SS.CSS in the specs folder"
    );
    assert_eq!(
        read(&folder, "OPEN/notes.md").unwrap_err(),
        "no file OPEN/notes.md in the specs folder"
    );
    let copied: Vec<Vec<String>> = folder
        .for_browser("a/SPEC.html", &|| {})
        .unwrap()
        .files
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(copied, [vec!["a".to_owned(), "SPEC.html".to_owned()]]);
}
