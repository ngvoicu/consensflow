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
//! that a screen read before it held still holds no question yet. After a yes it
//! clears its screen and goes on at once, or, with `CF_TEST_TRUST_STARTS_MS` set,
//! draws its prompt and takes that long to go on, its screen still meanwhile, as
//! Claude Code may draw its prompt and write the first message it was opened with
//! seconds later.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::process::own_var;

use super::terminal;

/// The file of the folders the stand-in trusts, in its config folder.
const TRUSTED: &str = "trusted-folders";

/// The keys it reads.
const DOWN: &str = "\u{1b}[B";
const UP: &str = "\u{1b}[A";

/// How long the first line stands alone before the question is drawn: less than the
/// time a screen must hold still to read as drawn.
const FIRST_FRAME: Duration = Duration::from_millis(1_000);

/// Writes `text` to the window's screen.
fn say(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// What came of the question.
pub enum Answer {
    /// It was not asked, or a yes answered it: the window goes on.
    GoOn,
    /// "No, exit", or Escape: the window ends.
    Exit,
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
pub fn ask(config: &Path, wanted: bool) -> Result<Answer, String> {
    let failed = |cause: io::Error| cause.to_string();
    let folder = std::env::current_dir().map_err(failed)?;
    let Some(root) = repository_of(&folder).filter(|_| wanted) else {
        return Ok(Answer::GoOn);
    };
    if trusted(config, &root) {
        return Ok(Answer::GoOn);
    }
    terminal::raw_mode();
    say("fake agent: opening the folder\r\n").map_err(failed)?;
    sleep(FIRST_FRAME);
    let mut yes = false;
    draw(&folder, yes).map_err(failed)?;
    let mut stdin = io::stdin().lock();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stdin.read(&mut chunk).map_err(failed)?;
        if read == 0 {
            return Ok(Answer::Exit);
        }
        let keys = String::from_utf8_lossy(&chunk[..read]).into_owned();
        // A paste is nothing to a question but its markers' Escapes.
        let keys = keys.replace("\u{1b}[200~", "").replace("\u{1b}[201~", "");
        if keys.contains(DOWN) {
            yes = true;
        } else if keys.contains(UP) {
            yes = false;
        } else if keys.contains('\r') {
            if !yes {
                return Ok(Answer::Exit);
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(config.join(TRUSTED))
                .map_err(failed)?;
            writeln!(file, "{}", root.display()).map_err(failed)?;
            let mut out = io::stdout().lock();
            out.write_all(b"\x1b[2J\x1b[H").map_err(failed)?;
            // One that starts slowly draws its prompt at once, and starts after.
            let starts = own_var("CF_TEST_TRUST_STARTS_MS").and_then(|ms| ms.parse().ok());
            if starts.is_some() {
                out.write_all(b" fake agent: the folder is trusted\r\n")
                    .map_err(failed)?;
            }
            out.flush().map_err(failed)?;
            drop(out);
            sleep(Duration::from_millis(starts.unwrap_or(0)));
            return Ok(Answer::GoOn);
        } else if keys == "\u{1b}" {
            return Ok(Answer::Exit);
        } else {
            continue;
        }
        draw(&folder, yes).map_err(failed)?;
    }
}
