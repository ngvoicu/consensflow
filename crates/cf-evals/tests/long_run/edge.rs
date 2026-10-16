//! Everything the long run prints, logs or keeps leaves it through its edge
//! (`src/long_run/edge.rs`), where what looks like a key or a token is hidden: no line
//! of the long run writes around it. The console is the edge's floor; the report is
//! kept by it; what stops the run is mapped by it before `cf-evals` writes it on its
//! error output; and what the rig says of itself, dropped unclosed, goes through the
//! masker the edge gives the rig's configuration.

use std::path::{Path, PathBuf};

/// The Rust files at `path`: the file, or every one in the folder and the folders in it.
fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|kind| kind == "rs") {
            vec![path.to_owned()]
        } else {
            Vec::new()
        };
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(path)
        .unwrap()
        .flat_map(|entry| rust_files(&entry.unwrap().path()))
        .collect();
    found.sort();
    found
}

/// Whether a line of Rust sends text out of the run: prints it, writes it to a file
/// or a stream, or makes a console of its own. A comment says.
fn writes_out(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    [
        "println!",
        "eprintln!",
        "print!(",
        "eprint!(",
        "stdout()",
        "stderr()",
        "files::write",
        "fs::write",
        "File::create",
        "OpenOptions",
        "write_all",
        "writeln!",
        "write!(",
        "Floor::new(",
        "Floor::masked(",
    ]
    .iter()
    .any(|word| code.contains(word))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn nothing_of_the_long_run_writes_around_its_edge() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = root.join("src").join("long_run");
    let edge = [folder.join("edge.rs"), folder.join("edge").join("tests.rs")];
    let mut files = rust_files(&folder);
    files.extend(rust_files(&root.join("src").join("long_run.rs")));
    for read in ["conduct.rs", "report.rs", "stage.rs", "edge.rs"] {
        assert!(files.contains(&folder.join(read)), "{read} is read");
    }
    let around: Vec<String> = files
        .iter()
        .filter(|file| !edge.contains(file))
        .flat_map(|file| {
            let shown = file
                .strip_prefix(root)
                .unwrap_or(file)
                .display()
                .to_string();
            read(file)
                .lines()
                .enumerate()
                .filter(|(_, line)| writes_out(line))
                .map(|(at, line)| format!("{shown}:{}: {}", at + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(around.is_empty(), "{}", around.join("\n"));
    // The edge itself writes, and hides first.
    let edge = read(&folder.join("edge.rs"));
    assert!(edge.contains("files::write(&text_file, self.shown(text))"));
    assert!(edge.contains("move |line| edge.shown(line),"));
    assert!(edge.contains("move |screen| rows.screen(screen),"));
    // The chief's screen a wait that gave up shows goes through the floor's screen mask.
    let chief = read(&root.join("src").join("run").join("chief.rs"));
    assert!(chief.contains("let shown = floor.screen(&shown).join(\" ⏎ \");"));
}

#[test]
fn what_stops_the_run_and_what_its_rig_says_leave_through_the_edge() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = root.join("src").join("long_run");
    // What stops the run is the edge's before `cf-evals` writes it.
    let conduct = read(&folder.join("conduct.rs"));
    assert!(conduct.contains("run(words, out, &edge).map_err(|failed| edge.failure(failed))"));
    assert!(read(&root.join("src").join("lib.rs")).contains("long_run::command(rest, out)"));
    // Every rig the run starts is given the edge's masker: the first in setup, and the
    // one a restart starts again on the same configuration.
    let starts: Vec<PathBuf> = rust_files(&folder)
        .into_iter()
        .filter(|file| read(file).contains("Rig::start("))
        .collect();
    assert_eq!(starts, [folder.join("setup.rs"), folder.join("stage.rs")]);
    let setup = read(&folder.join("setup.rs"));
    assert!(setup.contains(".masked_by(masker)"));
    assert!(read(&folder.join("stage.rs")).contains("Rig::start(self.config.clone()"));
    // And the rig keeps the masker its configuration is given, and says what it says of
    // itself, dropped, through it.
    let rig = root.join("..").join("cf-e2e").join("src").join("rig");
    assert!(read(&rig.join("launch.rs")).contains("masker: config.masker,"));
    let report = read(&rig.join("report.rs"));
    let written: Vec<&str> = report
        .lines()
        .filter(|line| line.contains("stderr()"))
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(
        written[0].contains("told(rig.masker.as_ref(), report)"),
        "{written:?}"
    );
}

#[test]
fn what_stops_the_long_run_is_written_on_the_error_output_with_a_secret_hidden() {
    // A run refused before it begins: `cf-evals` writes why on its error output.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let words = ["long", "--chief", "password=hunter2hunter"].map(std::ffi::OsString::from);
    let status = cf_evals::command(&words, &mut out, &mut err);
    let said = String::from_utf8(err).unwrap();
    assert_eq!(status, 2, "{said}");
    assert!(!said.contains("hunter2hunter"), "{said}");
    assert!(said.contains("password=[masked]"), "{said}");
}

#[test]
fn the_words_it_looks_for_are_found_where_they_are() {
    for line in [
        "eprintln!(\"{failed}\");",
        "let _ = std::io::stderr().write_all(report.as_bytes());",
        "files::write(&json_file, pretty)?;",
        "let mut floor = Floor::new(out);",
        "writeln!(out, \"{line}\")?;",
    ] {
        assert!(writes_out(line), "{line}");
    }
    for line in [
        "let mut floor = edge.floor(out);",
        "floor.say(line)?;",
        "// a writeln! of its own is never made",
    ] {
        assert!(!writes_out(line), "{line}");
    }
}
