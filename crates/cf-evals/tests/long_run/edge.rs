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
        "fs::copy",
        "VACUUM INTO",
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
    assert!(chief.contains("let lines = floor.screen(&self.last_lines(&pane, 40));"));
}

#[test]
fn what_stops_the_run_and_what_its_rig_says_leave_through_the_edge() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let folder = root.join("src").join("long_run");
    // What stops the run is the edge's before `cf-evals` writes it.
    let conduct = read(&folder.join("conduct.rs"));
    assert!(conduct.contains(
        "run(words, out, &edge).map_err(|failed| edge.failure(failed, &reports_of(words)))"
    ));
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
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| line.contains("stderr()") || line.contains("eprint"))
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    // What it says of itself is kept in a file, told first; its error output says where.
    assert!(
        written[0].contains(r#"eprint!("{where_kept}")"#),
        "{written:?}"
    );
    assert!(report.contains(
        "let where_kept = keep(&rig.root, &tell.text(&report), rig.reports.as_deref());"
    ));
    assert!(report.contains("let tell = Tell(rig.masker.as_ref());"));
}

/// Whether a line of Rust takes a part of a text: its first characters or its last,
/// its last lines or items, a slice of it (`&text[..100]`, `[at..end]`), the lines of a
/// screen the pane host cut to its last, or its lines or words split apart. A comment
/// says.
fn takes_part(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    let taken = code
        .match_indices(".take(")
        .any(|(at, _)| !code[at..].starts_with(".take()"));
    let sliced = code.match_indices('[').any(|(at, _)| {
        code[at..]
            .split(']')
            .next()
            .is_some_and(|inside| inside.contains(".."))
    });
    taken
        || sliced
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

/// Each line that takes a part of a text in the files the guard reads, and why what it
/// takes is masked first, is no text the run says, or is a cut accepted. A line new
/// here takes a part of a text unmasked until it is shown to take one masked whole
/// first.
///
/// What it reads: the long run's own sources, the rig's report (what a rig dropped
/// unclosed says), and the shared helpers its console and its report pass through
/// (`run/chief.rs`'s wait that gave up, `run/floor.rs`, `js.rs`'s cut). What it cannot
/// see: a cut inside a function it does not read (another crate's, or `plan.rs`'s
/// `last_lines`, which the chief's wait takes a window's last 40 lines with, a cut
/// accepted as the pane host's is), a cut it does not know the word of, and whether a
/// text it lists reaches the run's output at all. It reads lines, not meaning.
const TAKEN: [(&str, &str, &str); 60] = [
    (
        "long_run/stalls.rs",
        "self.prober.take(listed, pid, others, label)",
        "the system asked of a stalled window's processes, no text taken",
    ),
    (
        "long_run/stalls.rs",
        "taking.take(pid, &others, label)",
        "the same",
    ),
    (
        "long_run/conduct.rs",
        "self.stalls.take(&look, self.prober.as_mut());",
        "the system asked of a stalled window's processes, no text taken",
    ),
    (
        "long_run/stalls/held.rs",
        "said.lines()",
        "the system's list of processes, read; each line kept is masked whole and cut in stalls.rs",
    ),
    (
        "long_run/stalls/held.rs",
        "for row in said.lines() {",
        "the same, as Windows says it",
    ),
    (
        "long_run/stalls/held.rs",
        "let mut fields = row.split_whitespace();",
        "the same: a process's numbers read, and its line kept whole, masked after",
    ),
    (
        "long_run/stalls/held.rs",
        "if let [pid, state, wait] = numbers[..] {",
        "a thread's numbers, no text",
    ),
    (
        "long_run/disruptions.rs",
        "if let Some(next) = self.planned[place + 1..]",
        "the planned disruptions, no text",
    ),
    (
        "long_run/invariants/deliveries.rs",
        "quoting_until = body(id).map_or(end, |said| end + said_until(&text[end..], said));",
        "a check reading a prompt, nothing it says",
    ),
    (
        "long_run/invariants/deliveries.rs",
        ".find(&text[start..])",
        "where a delivery begins in a prompt, read and never said",
    ),
    (
        "long_run/invariants/deliveries.rs",
        "let inner = &text[start + tag.end()..];",
        "the same",
    ),
    (
        "long_run/stalls.rs",
        ".take(CHANGES)",
        "the last changes of a window's state, each masked whole and then cut",
    ),
    (
        "long_run/stalls.rs",
        ".take(EVENTS)",
        "the ledger's last events, each masked whole and then cut",
    ),
    (
        "long_run/invariants/order.rs",
        "let end = items[prompt + 1..]",
        "a check reading a conversation's items, nothing it says",
    ),
    (
        "long_run/invariants/order.rs",
        "last_turn(&items[prompt..end]).ok_or(Unheld::NoAnswer)",
        "the same",
    ),
    (
        "long_run/invariants/order.rs",
        "let turn = &items[prompt + 1..];",
        "the same",
    ),
    (
        "long_run/invariants/order.rs",
        "let mut answer = said(&turn[after..]);",
        "the same",
    ),
    (
        "long_run/invariants/order.rs",
        "let mut progress = said(&turn[..after]);",
        "the same",
    ),
    (
        "long_run/mask.rs",
        "for span in hidden[first..]",
        "the masker's walk over a screen it masks as one text",
    ),
    (
        "long_run/mask.rs",
        "shown.push_str(&joined[at..span.start.max(at)]);",
        "the same",
    ),
    (
        "long_run/mask.rs",
        "shown.push_str(&joined[at..place.end]);",
        "the same",
    ),
    (
        "long_run/mask.rs",
        "format!(\"{}{MASK}{}\", &whole[..before], &whole[after..])",
        "the masker hiding a secret it found in the whole",
    ),
    (
        "long_run/watcher.rs",
        ".add_claude(&String::from_utf8_lossy(&more[..whole]), names);",
        "a record's whole lines, each entry masked as it is read",
    ),
    (
        "run/chief.rs",
        "lines[lines.len().saturating_sub(12)..].to_vec()",
        "a screen's last rows, of rows masked as one text just before",
    ),
    (
        "run/chief.rs",
        "word.chars().take(most).collect()",
        "a word of a command line, masked whole before it is given (started_as)",
    ),
    (
        "js.rs",
        "format!(\"{text}{}\", \" \".repeat(width.saturating_sub(length(text))))",
        "padding to a width, no cut",
    ),
    (
        "js.rs",
        "format!(\"{}{text}\", \" \".repeat(width.saturating_sub(length(text))))",
        "the same",
    ),
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
        "the pane host's own cut, before the run has the text: accepted (mask.rs's note)",
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
    (
        "long_run/watcher/exits.rs",
        "let screen_told = body[\"tail\"]",
        "whether an exit says its screen's last lines, which proves its program gone: none of them is read",
    ),
    (
        "rig/report.rs",
        "let tail = &printed[printed.len() - most..];",
        "the end of what a window printed, read to be told as one screen and then cut to its last 1,500 characters (T-139): a secret split where it is read from is 64 KiB before them",
    ),
    (
        "rig/report.rs",
        ".map_or(tail, |at| &tail[at + 1..])",
        "the same, from the start of a line",
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
    for shared in ["run/chief.rs", "run/floor.rs", "js.rs"] {
        files.push((shared.to_owned(), source.join(shared)));
    }
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
    // it cuts.
    let report = read(&rig.join("report.rs"));
    for told in [
        "let log = tell.text(log);",
        "let screen = tell.screen_text(screen);",
        "return format!(\"  sessions/{name}: not JSON: {}\\n\", said(&tell.text(text)));",
        "return format!(\"    not JSON: {}\\n\", said(&tell.text(line)));",
        "format!(\"    {kind}: {}\\n\", said(&tell.text(text)))",
        "rig.page(method, body).map(|answer| tell.value(&answer))",
        "told[\"tail\"] = json!(self.screen(&rows));",
    ] {
        assert!(report.contains(told), "{told}");
    }
}

#[test]
fn the_parts_it_looks_for_are_found_where_they_are() {
    for line in [
        "let tail: String = screen.chars().rev().take(1500).collect();",
        "crate::js::first(&text, 200)",
        "lines[lines.len().saturating_sub(30)..]",
        "rig.snapshot(pane, 12)",
        "for row in text.lines() {",
        "let said = &text[..100];",
        "shown.push_str(&joined[at..place.end]);",
    ] {
        assert!(takes_part(line), "{line}");
    }
    for line in [
        "let then = self.then.take();",
        "// a line's .lines() of its own",
        "let shown = cut(text, 200, names);",
        "let first = rows[0].clone();",
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
