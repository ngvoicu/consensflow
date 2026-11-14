//! ConPTY helpers the Windows tests of the pane table and the arbiter share:
//! they expect, as the tests do.
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::PtySize;

use super::{PaneEnvironment, PaneKey, PaneOutput, PaneTable};

#[allow(clippy::disallowed_methods)] // A test's own Windows folder.
pub fn system(program: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\{program}")
}

pub fn powershell(script: &str) -> Vec<String> {
    vec![
        system(r"WindowsPowerShell\v1.0\powershell.exe"),
        "-NoProfile".into(),
        "-Command".into(),
        script.into(),
    ]
}

pub fn open(
    table: &Arc<PaneTable>,
    argv: &[String],
    env: &HashMap<String, String>,
    backlog: usize,
) -> (PaneKey, Receiver<PaneOutput>) {
    let streamed = table
        .open_streamed(
            &std::env::temp_dir(),
            argv,
            PaneEnvironment::new(env, &[]),
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
            backlog,
        )
        .expect("open a ConPTY pane");
    (streamed.key, streamed.output)
}

/// Everything the pane printed until `needle`, acknowledged as read.
/// ConPTY wraps the child's text in its own escape sequences, and its
/// output does not end when the child does, so a test reads for text. It
/// may open by asking where the cursor is (a console made to inherit its cursor does;
/// the pane host's are not) and hold the child's output
/// until a terminal answers; the page's xterm does, and so does this.
pub fn read_until(
    table: &PaneTable,
    key: &PaneKey,
    output: &Receiver<PaneOutput>,
    needle: &str,
) -> String {
    // A minute: a busy runner's PowerShell has twice taken longer than 20 s
    // to print its READY (2026-10-01 and 10-02); output on time costs nothing.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = String::new();
    while !seen.contains(needle) {
        let left = deadline.saturating_duration_since(Instant::now());
        let chunk = output
            .recv_timeout(left)
            .unwrap_or_else(|error| panic!("no {needle:?} in the pane ({error}): {seen:?}"));
        seen.push_str(&take(table, key, &chunk));
    }
    seen
}

/// What the pane printed in `chunk`, as text, once it is acknowledged as read
/// and, where it asks where the cursor is, answered.
pub fn take(table: &PaneTable, key: &PaneKey, chunk: &PaneOutput) -> String {
    table.ack(key, chunk.seq).expect("ack output");
    if chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[6n") {
        table
            .write(key, b"\x1b[1;1R")
            .expect("answer the cursor query");
    }
    String::from_utf8_lossy(&chunk.bytes).into_owned()
}

/// A child that says READY, reads one line and prints it back as `GOT=[…]#`.
pub fn line_echo() -> Vec<String> {
    powershell(
        "Write-Output READY; $line = [Console]::In.ReadLine(); Write-Output \"GOT=[$line]#\"",
    )
}
