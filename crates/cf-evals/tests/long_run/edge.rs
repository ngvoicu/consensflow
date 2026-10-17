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
    // And the rig keeps the masker its configuration is given, hands it what its
    // windows print as it starts, and says what it says of itself, dropped, through it.
    let rig = root.join("..").join("cf-e2e").join("src").join("rig");
    let launch = read(&rig.join("launch.rs"));
    assert!(launch.contains("masker: config.masker,"));
    assert!(launch.contains("masker.read(Printed::of(&shared));"));
    let report = read(&rig.join("report.rs"));
    let written: Vec<&str> = report
        .lines()
        .filter(|line| line.contains("stderr()"))
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(written[0].contains("tell.text(&report)"), "{written:?}");
    assert!(report.contains("let tell = Tell(rig.masker.as_ref());"));
}

/// Whether a line of Rust takes a part of a text: its first characters or its last,
/// its last lines or items, the lines of a screen the pane host cut to its last, or its
/// lines or words split apart. A comment says.
fn takes_part(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    let taken = code
        .match_indices(".take(")
        .any(|(at, _)| !code[at..].starts_with(".take()"));
    taken
        || [
            "js::first(",
            "saturating_sub(",
            "snapshot(",
            "[\"tail\"]",
            ".truncate(",
            "split_at(",
            "split_off(",
            ".lines()",
            ".split(",
            "split_once(",
            "split_whitespace(",
            "chars().rev()",
        ]
        .iter()
        .any(|word| code.contains(word))
}

/// Each line of the long run's, and of the rig's report, that takes a part of a text,
/// and why what it takes is masked first, or is no text it says. A line new here takes
/// a part of a text unmasked until it is shown to take one masked whole first.
const TAKEN: [(&str, &str, &str); 31] = [
    (
        "long_run/mask.rs",
        "crate::js::first(&mask(text, names), most)",
        "the cut, of a text masked whole",
    ),
    (
        "long_run/record.rs",
        "crate::js::first(&crate::js::squeeze(&self.text), 160)",
        "an entry's words, cut when read",
    ),
    (
        "long_run/record.rs",
        "said[said.len().saturating_sub(count)..]",
        "the last entries, each masked whole when read",
    ),
    (
        "long_run/record.rs",
        ".lines()",
        "a record's lines, each an entry of its own",
    ),
    (
        "long_run/invariants/work.rs",
        "items[items.len().saturating_sub(count)..]",
        "the last items, each masked whole and then cut",
    ),
    (
        "long_run/processes.rs",
        "said.lines()",
        "what the system says of processes",
    ),
    (
        "long_run/processes.rs",
        "let (pid, began) = line.trim().split_once(char::is_whitespace)?;",
        "the same",
    ),
    ("long_run/staff.rs", ".split(',')", "the run's own words"),
    (
        "long_run/staff.rs",
        "let Some((name, list)) = part.split_once('=') else {",
        "the same",
    ),
    ("long_run/staff.rs", ".split('+')", "the same"),
    (
        "long_run/staff.rs",
        ".split(|at: char| !at.is_ascii_digit())",
        "the same",
    ),
    ("long_run/options.rs", ".split(',')", "the same"),
    (
        "long_run/play.rs",
        "sleep(every.min(within.saturating_sub(elapsed)));",
        "a time",
    ),
    (
        "long_run/tracker.rs",
        "self.take(&due, now, there)",
        "the tracker's own, of processes",
    ),
    (
        "long_run/tracker.rs",
        "self.take(&all, now, there)",
        "the same",
    ),
    (
        "long_run/watcher.rs",
        "let snapshot = rig.snapshot(pane, TAIL);",
        "the pane host's cut: the edge reads every window whole",
    ),
    (
        "long_run/watcher.rs",
        ".map(|answer| exits::lines(&answer[\"tail\"]))",
        "the same",
    ),
    (
        "long_run/watcher/exits.rs",
        "tail: lines(&body[\"tail\"]),",
        "the same",
    ),
    (
        "long_run/stage.rs",
        ".and_then(|rig| rig.snapshot(pane, SCREEN))",
        "the same",
    ),
    (
        "long_run/stage.rs",
        ".map(|answer| lines(&answer[\"tail\"]))",
        "the same",
    ),
    (
        "long_run/edge.rs",
        "let rows: Vec<String> = printed.screen(&pane).lines().map(str::to_owned).collect();",
        "a window's whole, read as one screen",
    ),
    (
        "rig/report.rs",
        "let rows: Vec<String> = screen.lines().map(str::to_owned).collect();",
        "a window's whole, told as one screen",
    ),
    (
        "rig/report.rs",
        "let lines: Vec<&str> = log.lines().collect();",
        "the daemon's log, told whole just before",
    ),
    (
        "rig/report.rs",
        "let kept: Vec<char> = screen.chars().rev().take(SCREEN).collect();",
        "a window's screen, told whole just before",
    ),
    (
        "rig/report.rs",
        "let cut: String = line.chars().take(SAID).collect();",
        "what is said is told whole first: each file whole, each text of an answer",
    ),
    (
        "rig/report.rs",
        ".lines()",
        "a conversation's records, of a file told whole",
    ),
    (
        "rig/report.rs",
        "for record in &records[records.len().saturating_sub(RECORDS)..] {",
        "the same",
    ),
    (
        "rig/report.rs",
        "for message in &messages[messages.len().saturating_sub(MESSAGES)..] {",
        "the messages of an answer told whole",
    ),
    (
        "rig/report.rs",
        "lines[lines.len().saturating_sub(LOG)..]",
        "the daemon's log, told whole just before",
    ),
    (
        "rig/report.rs",
        "let rows: Option<Vec<String>> = exit[\"tail\"].as_array().and_then(|rows| {",
        "an exit's lines, read to be told as one screen",
    ),
    (
        "rig/report.rs",
        "told[\"tail\"] = json!(self.screen(&rows));",
        "the same, told",
    ),
];

#[test]
fn no_cut_split_or_tail_is_taken_of_text_unmasked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("src");
    let rig = root.join("..").join("cf-e2e").join("src").join("rig");
    let mut files: Vec<(String, PathBuf)> = rust_files(&source.join("long_run"))
        .into_iter()
        .chain(rust_files(&source.join("long_run.rs")))
        .filter(|file| file.file_name().is_some_and(|name| name != "tests.rs"))
        .map(|file| {
            let shown = file.strip_prefix(&source).unwrap().display().to_string();
            (shown.replace('\\', "/"), file)
        })
        .collect();
    files.push(("rig/report.rs".to_owned(), rig.join("report.rs")));
    let mut taken = Vec::new();
    for (shown, file) in &files {
        let text = read(file);
        // What is before the file's own tests.
        let code = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        for line in code.lines().filter(|line| takes_part(line)) {
            taken.push((shown.clone(), line.trim().to_owned()));
        }
    }
    let unshown: Vec<String> = taken
        .iter()
        .filter(|(file, line)| {
            !TAKEN
                .iter()
                .any(|(at, known, _)| at == file && known == line)
        })
        .map(|(file, line)| format!("{file}: {line}"))
        .collect();
    assert!(
        unshown.is_empty(),
        "taken of a text not shown masked first:\n{}",
        unshown.join("\n")
    );
    let gone: Vec<&str> = TAKEN
        .iter()
        .filter(|(at, known, _)| !taken.iter().any(|(file, line)| file == at && line == known))
        .map(|(_, known, _)| *known)
        .collect();
    assert!(gone.is_empty(), "no longer there: {gone:?}");
    // Masked first, where each is: the rig's report tells each text and screen before
    // it cuts, the pane host's screens leave in the light of all their windows printed,
    // and a break's screen is kept so.
    let report = read(&rig.join("report.rs"));
    for told in [
        "let log = tell.text(log);",
        "let screen = tell.screen_text(screen);",
        "status_line(&name, &tell.text(&held), process::is_alive)",
        "conversation_lines(&name, &tell.text(&held))",
        "rig.page(method, body).map(|answer| tell.value(&answer))",
        "told[\"tail\"] = json!(self.screen(&rows));",
    ] {
        assert!(report.contains(told), "{told}");
    }
    let edge = read(&source.join("long_run").join("edge.rs"));
    for learned in [
        "screen(rows, &self.learned())",
        "let shown = masked(value, &self.learned());",
        ".reading(move |printed| reads.reads(printed))",
    ] {
        assert!(edge.contains(learned), "{learned}");
    }
    assert!(read(&source.join("long_run").join("conduct.rs")).contains("self.edge.learned()"));
}

#[test]
fn the_parts_it_looks_for_are_found_where_they_are() {
    for line in [
        "let tail: String = screen.chars().rev().take(1500).collect();",
        "crate::js::first(&text, 200)",
        "lines[lines.len().saturating_sub(30)..]",
        "rig.snapshot(pane, 12)",
        "for row in text.lines() {",
    ] {
        assert!(takes_part(line), "{line}");
    }
    for line in [
        "let then = self.then.take();",
        "// a line's .lines() of its own",
        "let shown = cut(text, 200, names);",
    ] {
        assert!(!takes_part(line), "{line}");
    }
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
