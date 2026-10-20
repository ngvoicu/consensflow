//! The files a scenario looks at, measured beside a recorded ledger: the fixture's
//! files the run deleted, and the end of each file the run read; a run that looks at
//! none keeps the shape of every report before them.

use std::fs;
use std::path::Path;

use cf_evals::measure::{begun, measure, Options};

use crate::support::{measured, recorded, Folder};

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn the_files_gone_and_the_end_of_each_file_read_are_measured_for_a_scenario_that_looks_at_them() {
    let folder = Folder::new();
    let (fixture, workspace) = (
        folder.path().join("fixture"),
        folder.path().join("workspace"),
    );
    for root in [&fixture, &workspace] {
        write(root, "site/evaluare.html", "un scor între 0 și 10.");
        write(root, "site/index.html", "home");
    }
    let read = begun(&workspace, &["site/evaluare.html"]).unwrap();
    // The run wrecks the page and deletes the home page.
    write(&workspace, "site/evaluare.html", "<p>wrecked</p>");
    fs::remove_file(workspace.join("site/index.html")).unwrap();
    let ledger = recorded(&folder, "controls", &[]);
    let looked = measure(
        &ledger,
        &Options {
            files: Some((&fixture, &workspace)),
            read: Some(&read),
            gone: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(looked.files_gone, Some(vec!["site/index.html".to_owned()]));
    let page = &looked.files_read["site/evaluare.html"];
    assert_eq!(page.start.as_deref(), Some("un scor între 0 și 10."));
    assert_eq!(page.end.as_deref(), Some("<p>wrecked</p>"));
    // The rest of the numbers are the recording's, whatever the files say.
    let plain = measured("controls");
    assert_eq!(looked.plumbing, plain.plumbing);
    assert_eq!(looked.tasks, plain.tasks);

    // The same run, measured for a scenario that looks at no file: no key of them.
    let none = measure(
        &ledger,
        &Options {
            files: Some((&fixture, &workspace)),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(none.files_gone, None);
    assert!(none.files_read.is_empty());
    let report = none.to_value();
    assert!(report.get("filesGone").is_none(), "{report}");
    assert!(report.get("filesRead").is_none(), "{report}");
}
