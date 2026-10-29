//! A probe of what a program makes of a pane on Windows (T-792): the input modes its
//! console is in once it has started, the modes the console host asks of the pane in
//! what it writes, and what the program draws after each of the keys the pane host
//! writes, as the case file names them. It is a test that runs only when asked
//! (`--ignored`) and when `CF_PANE_PROBE` names a case file (from the workspace's root),
//! so what it found can be found again: `cargo test -p cf-panes --lib probe --
//! --ignored --nocapture` with `CF_PANE_PROBE=crates/cf-panes/probes/<file>.json`.
//!
//! A case file is `{"cases": [{"name", "argv", "env", "steps"}]}`. Each step waits
//! `after_ms` (reading what the pane prints meanwhile), then does one thing: `paste`
//! (in brackets, its Enter after `enter_after_ms`, as the pane host pastes), `write`
//! (bytes as they are), `keys` (Windows key events as win32-input-mode writes them:
//! `[virtual key, scan code, character, control state]`, each down and up), or
//! nothing; then prints the screen's last lines and, with `"mode": true`, the console's
//! input mode. `${TEMP}` in an argument or a value is a folder of the case's own, and
//! `${NAME}` the test's own variable.

#![allow(clippy::expect_used, clippy::print_stdout, clippy::disallowed_methods)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::PtySize;
use serde::Deserialize;

use super::{write_paste_via, PaneEnvironment, PaneKey, PaneOutput, PaneTable, StreamedPane};

#[derive(Deserialize)]
struct Probe {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    argv: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default = "rows")]
    rows: u16,
    /// The case runs in the folder of the case before it, whose programs' files it finds.
    #[serde(default)]
    temp_of_previous: bool,
}

const fn rows() -> u16 {
    40
}

#[derive(Deserialize, Default)]
struct Step {
    #[serde(default)]
    after_ms: u64,
    #[serde(default)]
    paste: Option<String>,
    #[serde(default = "enter_after")]
    enter_after_ms: u64,
    #[serde(default)]
    write: Option<String>,
    #[serde(default)]
    keys: Option<Vec<[u32; 4]>>,
    #[serde(default)]
    mode: bool,
    /// Print what every `.session.lock` in the case's folder says, while its program runs.
    #[serde(default)]
    locks: bool,
    #[serde(default)]
    note: Option<String>,
}

const fn enter_after() -> u64 {
    300
}

/// `text` with `${TEMP}` as `temp` (`${TEMP_JSON}` as a JSON string spells it), `${LAST_SESSION}` as the newest Muse session in its
/// data folder (`data/muse/sessions/YYYY/MM/DD/<id>`), and every other `${NAME}` as
/// the test's own variable.
fn expand(text: &str, temp: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('}') else {
            break;
        };
        let name = &rest[at + 2..at + end];
        if name == "TEMP" {
            out.push_str(temp);
        } else if name == "TEMP_JSON" {
            out.push_str(&temp.replace('\\', "\\\\"));
        } else if name == "LAST_SESSION" {
            out.push_str(&last_session(std::path::Path::new(temp)).unwrap_or_default());
        } else {
            out.push_str(&std::env::var(name).unwrap_or_default());
        }
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}

/// The newest session Muse began in `temp`'s data folder, by its id (a UUIDv7, which
/// sorts by when it began).
fn last_session(temp: &std::path::Path) -> Option<String> {
    let mut found = Vec::new();
    let mut folders = vec![(temp.join("data").join("muse").join("sessions"), 0)];
    while let Some((folder, depth)) = folders.pop() {
        for entry in std::fs::read_dir(&folder).ok()?.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            if depth == 3 {
                found.push(entry.file_name().to_string_lossy().into_owned());
            } else {
                folders.push((entry.path(), depth + 1));
            }
        }
    }
    found.into_iter().max()
}

/// The console input mode of the console `pid` is attached to: this process leaves its
/// own console, joins that one for the moment it reads its input's mode, and leaves it.
fn console_mode(pid: u32) -> Result<u32, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleMode};

    // SAFETY: leaving and joining a console touches no memory of Rust's; the probe runs
    // alone, holding the PTY tests' lock.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(format!(
                "AttachConsole({pid}): {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    let read = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")
        .map_err(|error| format!("CONIN$: {error}"))
        .and_then(|input| {
            let mut mode = 0u32;
            // SAFETY: the handle is the open CONIN$'s, `mode` a u32 of this frame.
            let ok = unsafe { GetConsoleMode(input.as_raw_handle(), &mut mode) };
            if ok == 0 {
                Err(format!(
                    "GetConsoleMode: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(mode)
            }
        });
    // SAFETY: as above.
    unsafe {
        FreeConsole();
    }
    read
}

/// The names of the flags of a console input mode.
fn mode_names(mode: u32) -> String {
    const FLAGS: [(u32, &str); 9] = [
        (0x1, "PROCESSED_INPUT"),
        (0x2, "LINE_INPUT"),
        (0x4, "ECHO_INPUT"),
        (0x8, "WINDOW_INPUT"),
        (0x10, "MOUSE_INPUT"),
        (0x20, "INSERT_MODE"),
        (0x40, "QUICK_EDIT_MODE"),
        (0x80, "EXTENDED_FLAGS"),
        (0x200, "VIRTUAL_TERMINAL_INPUT"),
    ];
    let names: Vec<&str> = FLAGS
        .iter()
        .filter(|(flag, _)| mode & flag != 0)
        .map(|(_, name)| *name)
        .collect();
    format!("{mode:#06x} [{}]", names.join(" "))
}

/// The control sequences `bytes` holds that set or ask a mode of the terminal: private
/// modes (`CSI ? … h/l`), keyboard protocols (`CSI > … u`, `CSI = … u`, `CSI < u`,
/// `CSI > 4 ; … m`), and the host's queries (`CSI … n`, `CSI c`).
fn modes(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 2 < bytes.len() {
        if bytes[at] == 0x1b && bytes[at + 1] == b'[' {
            let start = at + 2;
            let mut end = start;
            while end < bytes.len() && (0x20..0x40).contains(&bytes[end]) {
                end += 1;
            }
            if end < bytes.len() {
                let params = &bytes[start..end];
                let last = bytes[end];
                let private = params.first().is_some_and(|b| b"?><=".contains(b));
                let wanted = (private && b"hlum".contains(&last))
                    || last == b'n'
                    || (last == b'c'
                        && params
                            .iter()
                            .all(|b| b.is_ascii_digit() || b"?>".contains(b)));
                if wanted {
                    found.push(format!(
                        "ESC[{}{}",
                        String::from_utf8_lossy(params),
                        char::from(last)
                    ));
                }
                at = end + 1;
                continue;
            }
        }
        at += 1;
    }
    found
}

/// The bytes win32-input-mode writes for `key` (`[vk, scan, char, control state]`),
/// down and up.
fn key_bytes(key: [u32; 4]) -> Vec<u8> {
    let [vk, scan, ch, state] = key;
    format!("\x1b[{vk};{scan};{ch};1;{state};1_\x1b[{vk};{scan};{ch};0;{state};1_").into_bytes()
}

struct Watch<'a> {
    table: &'a PaneTable,
    pane: &'a StreamedPane,
    raw: Vec<u8>,
}

impl Watch<'_> {
    /// Reads what the pane prints for `wait`, feeding its screen and acknowledging it.
    fn read_for(&mut self, wait: Duration) {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            let Ok(chunk) = self.pane.output.recv_timeout(left) else {
                return;
            };
            self.take(&chunk);
        }
    }

    fn take(&mut self, chunk: &PaneOutput) {
        self.table
            .ack(&self.pane.key, chunk.seq)
            .expect("ack output");
        self.pane.screen.feed(&chunk.bytes);
        self.raw.extend_from_slice(&chunk.bytes);
    }

    fn key(&self) -> &PaneKey {
        &self.pane.key
    }

    fn screen(&self, what: &str) {
        let drawn = self.pane.screen.bottom(40);
        println!("--- screen {what}:");
        for line in drawn.lines.iter().filter(|line| !line.trim().is_empty()) {
            println!("  | {line}");
        }
    }
}

fn run(case: &Case, previous: Option<std::path::PathBuf>) -> std::path::PathBuf {
    let _pty_guard = super::serial_pty_test();
    let temp = match previous {
        Some(folder) if case.temp_of_previous => folder,
        _ => tempfile_dir(&case.name),
    };
    let temp_text = temp.to_string_lossy().into_owned();
    let argv: Vec<String> = case
        .argv
        .iter()
        .map(|arg| expand(arg, &temp_text))
        .collect();
    let env: HashMap<String, String> = case
        .env
        .iter()
        .map(|(name, value)| (name.clone(), expand(value, &temp_text)))
        .collect();
    println!("=== case {} argv {argv:?}", case.name);
    let table = Arc::new(PaneTable::new());
    let pane = table
        .open_streamed(
            &temp,
            &argv,
            PaneEnvironment::new(&env, &[]),
            PtySize {
                rows: case.rows,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            },
            1 << 22,
        )
        .expect("open the pane");
    println!("pid {:?}", pane.pid);
    // Each console host makes its own keys of the same bytes (`keys.rs`): which one this is.
    println!("console host: {:?}", super::conpty::host_library());
    let mut watch = Watch {
        table: &table,
        pane: &pane,
        raw: Vec::new(),
    };
    for (number, step) in case.steps.iter().enumerate() {
        watch.read_for(Duration::from_millis(step.after_ms));
        let before = watch.raw.len();
        let what = step.note.clone().unwrap_or_default();
        if let Some(text) = &step.paste {
            let (table, key) = (watch.table, watch.key().clone());
            let wait = Duration::from_millis(step.enter_after_ms);
            write_paste_via(table, &key, text.as_bytes(), || std::thread::sleep(wait))
                .expect("paste");
            println!("step {number}: pasted {text:?} ({what})");
        } else if let Some(bytes) = &step.write {
            table.write(watch.key(), bytes.as_bytes()).expect("write");
            println!("step {number}: wrote {bytes:?} ({what})");
        } else if let Some(keys) = &step.keys {
            for key in keys {
                table
                    .write(watch.key(), &key_bytes(*key))
                    .expect("write a key");
            }
            println!("step {number}: keys {keys:?} ({what})");
        } else {
            println!("step {number}: looked ({what})");
        }
        if step.mode {
            match pane.pid.map(console_mode) {
                Some(Ok(mode)) => {
                    println!("step {number}: console input mode {}", mode_names(mode))
                }
                Some(Err(error)) => println!("step {number}: console input mode unread: {error}"),
                None => println!("step {number}: no pid"),
            }
        }
        if step.locks {
            locks(&temp);
        }
        watch.read_for(Duration::from_millis(1_500));
        let printed = modes(&watch.raw[before.min(watch.raw.len())..]);
        if !printed.is_empty() {
            println!("step {number}: modes printed since: {printed:?}");
        }
        watch.screen(&format!("after step {number}"));
    }
    println!("all modes printed: {:?}", modes(&watch.raw));
    println!(
        "first bytes: {:?}",
        String::from_utf8_lossy(&watch.raw[..watch.raw.len().min(400)])
    );
    table.kill(watch.key()).expect("close the pane");
    written(&temp, 0);
    temp
}

/// What each `.session.lock` under `folder` says, or why it cannot be read.
fn locks(folder: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            locks(&path);
        } else if path.file_name().is_some_and(|name| name == ".session.lock") {
            match std::fs::read(&path) {
                Ok(bytes) => println!(
                    "lock {}: {:?}",
                    path.display(),
                    String::from_utf8_lossy(&bytes)
                ),
                Err(error) => println!("lock {}: unread: {error}", path.display()),
            }
        }
    }
}

/// What the case's programs wrote in its folder: every file, and the head of each log.
fn written(folder: &std::path::Path, depth: usize) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("{}{}", "  ".repeat(depth + 1), path.display());
        if path.is_dir() && depth < 8 {
            written(&path, depth + 1);
        } else if path.extension().is_some_and(|ext| ext == "txt") {
            let bytes = std::fs::read(&path).unwrap_or_default();
            println!(
                "      = {:?}",
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(3000)
                    .collect::<String>()
            );
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for line in text.lines().take(8) {
                println!("      > {}", line.chars().take(500).collect::<String>());
            }
        }
    }
}

/// A folder of the case's own in the system's temporary folder, new for each run.
fn tempfile_dir(name: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let folder = std::env::temp_dir().join(format!("cf-pane-probe-{name}-{stamp}"));
    std::fs::create_dir_all(&folder).expect("the case's folder");
    folder
}

#[test]
#[ignore = "a probe, run by hand on a Windows machine with CF_PANE_PROBE naming its cases"]
fn probe() {
    let Ok(file) = std::env::var("CF_PANE_PROBE") else {
        println!("CF_PANE_PROBE names no case file: nothing probed");
        return;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(&file)).expect("the case file");
    let probe: Probe = serde_json::from_str(&text).expect("the case file's JSON");
    let mut previous = None;
    for case in &probe.cases {
        previous = Some(run(case, previous));
    }
}

#[cfg(test)]
mod tests {
    use super::{expand, key_bytes, modes};

    #[test]
    fn the_modes_a_pane_printed_are_its_private_modes_keyboard_protocols_and_queries() {
        let printed = b"\x1b[?9001h\x1b[?1004h\x1b[2J\x1b[>1u\x1b[?25l\x1b[6n\x1b[1;1H";
        assert_eq!(
            modes(printed),
            ["ESC[?9001h", "ESC[?1004h", "ESC[>1u", "ESC[?25l", "ESC[6n"]
        );
    }

    #[test]
    fn a_key_is_written_down_and_up() {
        assert_eq!(
            key_bytes([13, 28, 13, 0]),
            b"\x1b[13;28;13;1;0;1_\x1b[13;28;13;0;0;1_".to_vec()
        );
    }

    #[test]
    fn a_value_names_the_case_s_folder_and_the_test_s_variables() {
        assert_eq!(expand("${TEMP}\\data", "C:\\t"), "C:\\t\\data");
        assert_eq!(expand("${TEMP_JSON}", "C:\\t"), "C:\\\\t");
        assert_eq!(expand("a${CF_NO_SUCH_VARIABLE_HERE}b", "x"), "ab");
    }
}
