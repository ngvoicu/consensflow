//! Claude Code's question about a folder it was never told to trust, as the window
//! of the participant `CF_TEST_TRUST` names asks it before anything else: in a git
//! repository whose root is not in the folders it trusted (`trusted-folders`, a
//! line each, in its config folder), as Claude Code 2.1.305
//! asks it (`cargo xtask live folder-trust`). It names the window's folder, and
//! opens with its `❯` on "No, exit"; Down and Up move it; Enter on "Yes, I trust
//! this folder" writes the repository's root down and goes on to the window's
//! prompt, and Enter on "No, exit", or Escape, ends the window. What is pasted is
//! nothing to it, and the Enter after a paste is the Enter of its choice. It draws a
//! first line a second before the question, as a harness draws its first frames, so
//! that a screen read before it held still holds no question yet (longer with
//! `CF_TEST_TRUST_FIRST_FRAME_MS` set, as a harness under load holds a first frame
//! still before its question; and then a window of that participant that asks
//! nothing holds it as long before its prompt). After a yes it
//! clears its screen and goes on at once, or, with `CF_TEST_TRUST_STARTS_MS` set,
//! draws its prompt and takes that long to go on, its screen still meanwhile, as
//! Claude Code may draw its prompt and write the first message it was opened with
//! seconds later. With `CF_TEST_TRUST_EXIT_MS` set, a No says so on its screen and
//! the window takes that long to end. With `CF_TEST_TRUST_REDRAW_MS` set it takes
//! that long to move its `❯` for a Down, its screen as it was meanwhile; and it
//! writes down each key it was given (`trust-keys`, a word a line, in its config
//! folder). With `CF_TEST_TRUST_DROPS_FIRST_KEY` set it ignores the first key it is
//! given, as a window that did not take it; with `CF_TEST_RECORD_GROWS_ON_NO` set its
//! record grows once as a No ends it, as a process before it the host could not end
//! writes still; and with `CF_TEST_RECORD_AWAY_MS` set a record a case moved aside
//! comes back that long after the window starts ([`bring_the_record_back`]); and with
//! `CF_TEST_TRUST_LEAVES_MS` set a window that was answered yes ends that long after it
//! draws its prompt, as a window whose task is done and closed at once does.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::process::own_var;

use super::terminal;

/// The file of the folders the stand-in trusts, in its config folder, and the one
/// of the keys its question was given.
const TRUSTED: &str = "trusted-folders";
const KEYS: &str = "trust-keys";

/// The keys it reads.
const DOWN: &str = "\u{1b}[B";
const UP: &str = "\u{1b}[A";

/// The milliseconds the variable `name` says, where it is set.
fn millis(name: &str) -> Option<Duration> {
    own_var(name)
        .and_then(|ms| ms.parse().ok())
        .map(Duration::from_millis)
}

/// Appends `line` to the file `name` in `config`, whole, in one write: two windows of
/// one participant write the same files at the same moment (main's Windows CI,
/// 2026-10-21: `writeln!` wrote the line and its end apart, and two windows' yeses
/// came out as both folders on one line).
fn append(config: &Path, name: &str, line: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config.join(name))?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// Puts the window's record back `CF_TEST_RECORD_AWAY_MS` after the window starts,
/// where a case moved it aside (to `<transcript>.away`, an empty transcript in its
/// place) before the window opened: the first looks find a record with nothing in it,
/// which cannot be read, as a transcript being written over cannot be.
pub fn bring_the_record_back(transcript: &Path) {
    let Some(away) = millis("CF_TEST_RECORD_AWAY_MS") else {
        return;
    };
    let hidden = transcript.with_extension("away");
    if !hidden.exists() {
        return;
    }
    let back = transcript.to_path_buf();
    std::thread::spawn(move || {
        sleep(away);
        let _ = fs::rename(hidden, back);
    });
}

/// How long the first line stands alone before the question is drawn: less than the
/// time a screen must hold still to read as drawn.
const FIRST_FRAME: Duration = Duration::from_millis(1_000);

/// Writes `text` to the window's screen.
fn say(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// A key the question takes.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    Down,
    Up,
    Enter,
    Escape,
}

/// The keys `given` holds, in the order they came: an arrow's sequence, Enter (a
/// carriage return or a line feed), and an Escape that nothing follows; the rest is
/// nothing to the question.
fn keys_in(given: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = given;
    while let Some(first) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix(DOWN) {
            keys.push(Key::Down);
            rest = after;
        } else if let Some(after) = rest.strip_prefix(UP) {
            keys.push(Key::Up);
            rest = after;
        } else {
            match first {
                '\r' | '\n' => keys.push(Key::Enter),
                '\u{1b}' if rest.len() == 1 => keys.push(Key::Escape),
                _ => {}
            }
            rest = &rest[first.len_utf8()..];
        }
    }
    keys
}

/// What came of the question.
pub enum Answer {
    /// It was not asked: the window goes on.
    GoOn,
    /// A yes answered it: the window goes on.
    Yes,
    /// "No, exit", or Escape: the window ends.
    Exit,
}

/// How long after its prompt a window that was answered yes ends, where it does.
pub fn leaves_after() -> Option<Duration> {
    millis("CF_TEST_TRUST_LEAVES_MS")
}

/// The root of the git repository `folder` is in: none out of one.
fn repository_of(folder: &Path) -> Option<PathBuf> {
    folder
        .ancestors()
        .find(|above| above.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Whether `root` is one of the folders trusted under `config`.
fn trusted(config: &Path, root: &Path) -> bool {
    fs::read_to_string(config.join(TRUSTED))
        .is_ok_and(|text| text.lines().any(|line| Path::new(line) == root))
}

/// The question, the `❯` on the yes where `yes`.
fn draw(folder: &Path, yes: bool) -> io::Result<()> {
    let (no_line, yes_line) = if yes {
        ("   No, exit", " ❯ Yes, I trust this folder")
    } else {
        (" ❯ No, exit", "   Yes, I trust this folder")
    };
    let mut out = io::stdout().lock();
    write!(
        out,
        "\u{1b}[2J\u{1b}[H Accessing workspace:\r\n {}\r\n\r\n Quick safety check: Is this a project you created or one you trust?\r\n Security guide\r\n\r\n{no_line}\r\n{yes_line}\r\n\r\n Enter to confirm · Esc to cancel\r\n",
        folder.display()
    )?;
    out.flush()
}

/// Asks the question where the window `wanted` (it is the participant `CF_TEST_TRUST`
/// names) opened in an untrusted repository, and reads the answer from its terminal.
pub fn ask(
    config: &Path,
    wanted: bool,
    terminal: &mut terminal::Keys,
    mut on_no: impl FnMut() -> Result<(), String>,
) -> Result<Answer, String> {
    let failed = |cause: io::Error| cause.to_string();
    let folder = std::env::current_dir().map_err(failed)?;
    if !wanted {
        return Ok(Answer::GoOn);
    }
    let first_frame = millis("CF_TEST_TRUST_FIRST_FRAME_MS");
    let opening = |held: Duration| -> Result<(), String> {
        say("fake agent: opening the folder\r\n").map_err(failed)?;
        sleep(held);
        Ok(())
    };
    let asks = repository_of(&folder).filter(|root| !trusted(config, root));
    let Some(root) = asks else {
        // One that asks nothing holds its first frame as long, before its prompt.
        if let Some(held) = first_frame {
            opening(held)?;
        }
        return Ok(Answer::GoOn);
    };
    opening(first_frame.unwrap_or(FIRST_FRAME))?;
    let mut yes = false;
    draw(&folder, yes).map_err(failed)?;
    let mut drops = own_var("CF_TEST_TRUST_DROPS_FIRST_KEY").is_some();
    let reads_late = millis("CF_TEST_TRUST_READS_LATE_MS");
    loop {
        let Some(mut given) = terminal.chunk() else {
            return Ok(Answer::Exit);
        };
        // A window busy elsewhere reads its terminal late: what was typed meanwhile
        // comes in one read.
        if let Some(late) = reads_late {
            sleep(late);
            given.extend(terminal.since());
        }
        let given = String::from_utf8_lossy(&given).into_owned();
        // A paste is nothing to a question but its markers' Escapes.
        let given = given.replace("\u{1b}[200~", "").replace("\u{1b}[201~", "");
        // A window that did not take the first key it was given.
        if drops && !given.is_empty() {
            drops = false;
            append(config, KEYS, "dropped").map_err(failed)?;
            continue;
        }
        let mut no = || -> Result<Answer, String> {
            if let Some(lingers) = millis("CF_TEST_TRUST_EXIT_MS") {
                say("\u{1b}[2J\u{1b}[H fake agent: the folder is not trusted\r\n")
                    .map_err(failed)?;
                on_no()?;
                sleep(lingers);
            }
            Ok(Answer::Exit)
        };
        // Each key in the order it came, however many one read holds.
        let mut moved = false;
        for key in keys_in(&given) {
            match key {
                Key::Down => {
                    append(config, KEYS, "down").map_err(failed)?;
                    if let Some(redraw) = millis("CF_TEST_TRUST_REDRAW_MS") {
                        sleep(redraw);
                    }
                    yes = true;
                    moved = true;
                }
                Key::Up => {
                    append(config, KEYS, "up").map_err(failed)?;
                    yes = false;
                    moved = true;
                }
                Key::Enter => {
                    append(config, KEYS, "enter").map_err(failed)?;
                    if !yes {
                        return no();
                    }
                    append(config, TRUSTED, &root.display().to_string()).map_err(failed)?;
                    let mut out = io::stdout().lock();
                    out.write_all(b"\x1b[2J\x1b[H").map_err(failed)?;
                    // One that starts slowly draws its prompt at once, and starts after.
                    let starts = millis("CF_TEST_TRUST_STARTS_MS");
                    if starts.is_some() {
                        out.write_all(
                            b" fake agent: the folder is trusted \xc2\xb7 bypass permissions on\r\n",
                        )
                        .map_err(failed)?;
                    }
                    out.flush().map_err(failed)?;
                    drop(out);
                    sleep(starts.unwrap_or_default());
                    return Ok(Answer::Yes);
                }
                Key::Escape => {
                    append(config, KEYS, "escape").map_err(failed)?;
                    return no();
                }
            }
        }
        if !moved {
            continue;
        }
        draw(&folder, yes).map_err(failed)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_windows_that_append_at_once_leave_two_whole_lines() {
        let config = tempfile::tempdir().unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = ["/work/first", "/work/second"]
            .into_iter()
            .map(|folder| {
                let (config, start) = (config.path().to_path_buf(), std::sync::Arc::clone(&start));
                std::thread::spawn(move || {
                    start.wait();
                    append(&config, TRUSTED, folder).unwrap();
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let written = fs::read_to_string(config.path().join(TRUSTED)).unwrap();
        let mut lines: Vec<&str> = written.lines().collect();
        lines.sort_unstable();
        assert_eq!(lines, ["/work/first", "/work/second"], "{written:?}");
    }

    #[test]
    fn the_keys_of_one_read_are_taken_in_the_order_they_came() {
        assert_eq!(keys_in("\u{1b}[B\r"), [Key::Down, Key::Enter]);
        assert_eq!(
            keys_in("\u{1b}[A\u{1b}[Bx\n"),
            [Key::Up, Key::Down, Key::Enter]
        );
        assert_eq!(keys_in("\u{1b}"), [Key::Escape]);
        assert_eq!(keys_in("typed"), []);
    }
}
