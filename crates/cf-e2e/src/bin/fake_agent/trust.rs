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
//! comes back that long after the window starts ([`bring_the_record_back`]). With
//! `CF_TEST_TRUST_CURSOR` set it draws its cursor as those characters, not `❯`, as a
//! Claude Code that changed its glyph would. With `CF_TEST_TRUST_GRANTS` set it asks
//! instead as Claude Code 2.1.309 asks of a folder trusted by a folder above it whose
//! own settings grant more (T-354): wherever the folder a yes records (the root of the
//! repository the window's folder is in, or the folder itself out of one, as Claude Code
//! records it, T-381) is not among the folders it trusts, its no "No, continue without
//! these permissions", on which Enter, or Escape, goes on into the window with nothing
//! written down, and a yes writes that folder down. With `CF_TEST_TRUST_FROZEN_AFTER_YES_MS`
//! set, a yes leaves its screen as it was that long before anything else is drawn, as a
//! screen the host shows late; what is typed meanwhile goes to what it asks next.
//! With `CF_TEST_TRUST_SERVER` set to servers' names (a comma between two, or `mcp.json`
//! for those of the `.mcp.json` of the window's folder), once past that question (or
//! where it asks none), it asks as Claude Code 2.1.309 asks of servers of the project's
//! `.mcp.json` it was never told to use (T-383), those the `.claude/settings.local.json`
//! of the window's folder names neither used nor left out. One: "New MCP server found in
//! this project:" and its name, its `❯` on the last of three choices, Up and Down moving
//! it round as Claude Code's does; Enter on "Use this MCP server" writes its name in
//! `enabledMcpjsonServers` there (and on the second choice `enableAllProjectMcpServers`
//! too), Enter on "Continue without using this MCP server", or Escape, writes it in
//! `disabledMcpjsonServers`. More: "N new MCP servers found in this project", a ticked
//! box a server, its `❯` on the first, then "Enable selected", Up and Down stopping at
//! the ends; Enter on a box unticks it, and on "Enable selected" uses the ticked ones
//! and leaves out the rest. Either way the window goes on. Its keys are written down as
//! the question's are; with `CF_TEST_TRUST_SERVER_DROPS_FIRST_KEY` set it ignores the
//! first key it is given, and with `CF_TEST_TRUST_SERVER_REFUSES` an Enter leaves every
//! server out, whatever the cursor is on.

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

/// The question's no, and what it says of the grants it would leave out, where it is
/// asked of a trusted folder's grants (`CF_TEST_TRUST_GRANTS`).
const NO_EXIT: &str = "No, exit";
const NO_GRANTS: &str = "No, continue without these permissions";
const GRANTS: &str = " \u{26a0} This folder pre-approves 1 tool permission in .claude/settings.json:\r\n   Bash(ls:*)\r\n These will apply without asking. Only proceed if you trust this configuration.\r\n\r\n";

/// The question, its cursor (`❯`, or `CF_TEST_TRUST_CURSOR`) on the yes where `yes`,
/// asked of a trusted folder's grants where `grants`.
fn draw(folder: &Path, yes: bool, grants: bool) -> io::Result<()> {
    let cursor = own_var("CF_TEST_TRUST_CURSOR").unwrap_or_else(|| "❯".to_owned());
    let no = if grants { NO_GRANTS } else { NO_EXIT };
    let (no_line, yes_line) = if yes {
        (
            format!("   {no}"),
            format!(" {cursor} Yes, I trust this folder"),
        )
    } else {
        (
            format!(" {cursor} {no}"),
            "   Yes, I trust this folder".to_owned(),
        )
    };
    let granted = if grants { GRANTS } else { "" };
    let mut out = io::stdout().lock();
    write!(
        out,
        "\u{1b}[2J\u{1b}[H Accessing workspace:\r\n {}\r\n\r\n Quick safety check: Is this a project you created or one you trust?\r\n{granted} Security guide\r\n\r\n{no_line}\r\n{yes_line}\r\n\r\n Enter to confirm · Esc to cancel\r\n",
        folder.display()
    )?;
    out.flush()
}

/// The choices of the dialog of a new server, on the last of which it opens.
const SERVER_CHOICES: [&str; 3] = [
    "Use this MCP server",
    "Use this and all future MCP servers in this project",
    "Continue without using this MCP server",
];

/// The local settings of the window's `folder`, where Claude Code keeps what it was
/// told of the project's servers.
fn local_settings(folder: &Path) -> PathBuf {
    folder.join(".claude").join("settings.local.json")
}

/// The servers `CF_TEST_TRUST_SERVER` names, a comma between two: those of the
/// `.mcp.json` of the window's `folder` where it says `mcp.json`.
fn servers_named(folder: &Path) -> Vec<String> {
    let named = own_var("CF_TEST_TRUST_SERVER").unwrap_or_default();
    if named == "mcp.json" {
        return fs::read_to_string(folder.join(".mcp.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|config| config["mcpServers"].as_object().cloned())
            .map(|servers| servers.keys().cloned().collect())
            .unwrap_or_default();
    }
    named
        .split(',')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The dialog of the new server `name`, its cursor on the choice `on`.
fn draw_server(name: &str, on: usize) -> io::Result<()> {
    let choices: Vec<String> = SERVER_CHOICES
        .iter()
        .enumerate()
        .map(|(at, choice)| format!(" {} {choice}", if at == on { '❯' } else { ' ' }))
        .collect();
    let mut out = io::stdout().lock();
    write!(
        out,
        "\u{1b}[2J\u{1b}[H New MCP server found in this project: {name}\r\n\r\n MCP servers may execute code or access system resources. All tool calls require approval.\r\n\r\n{}\r\n\r\n Enter to confirm · Esc to cancel\r\n",
        choices.join("\r\n")
    )?;
    out.flush()
}

/// The dialog of the new servers `names`, those `ticked` ticked, its cursor on the line
/// `on` (one a server, then "Enable selected").
fn draw_servers(names: &[String], ticked: &[bool], on: usize) -> io::Result<()> {
    let cursor = |at: usize| if at == on { '❯' } else { ' ' };
    let mut choices: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            let tick = if ticked[at] { "[✔]" } else { "[ ]" };
            format!(" {} {tick} {name}", cursor(at))
        })
        .collect();
    choices.push(format!(" {} Enable selected", cursor(names.len())));
    let mut out = io::stdout().lock();
    write!(
        out,
        "\u{1b}[2J\u{1b}[H {} new MCP servers found in this project\r\n Select any you wish to enable.\r\n\r\n MCP servers may execute code or access system resources. All tool calls require approval.\r\n\r\n{}\r\n\r\n Space to select · Esc to reject all\r\n",
        names.len(),
        choices.join("\r\n")
    )?;
    out.flush()
}

/// Asks of the servers `CF_TEST_TRUST_SERVER` names that the local settings of the
/// window's folder name neither used nor left out, where the window `wanted`: one in
/// the dialog of one, more in the dialog of several. Reads the answer from its
/// terminal; whatever it is, the window goes on.
pub fn ask_server(
    config: &Path,
    wanted: bool,
    terminal: &mut terminal::Keys,
) -> Result<(), String> {
    let failed = |cause: io::Error| cause.to_string();
    if !wanted {
        return Ok(());
    }
    let folder = std::env::current_dir().map_err(failed)?;
    let file = local_settings(&folder);
    let mut settings: serde_json::Value = fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let named = |list: &str, name: &str| {
        settings[list]
            .as_array()
            .is_some_and(|names| names.iter().any(|named| named == name))
    };
    let pending: Vec<String> = servers_named(&folder)
        .into_iter()
        .filter(|name| {
            !named("enabledMcpjsonServers", name) && !named("disabledMcpjsonServers", name)
        })
        .collect();
    if pending.is_empty() || settings["enableAllProjectMcpServers"] == true {
        return Ok(());
    }
    say("fake agent: reading the project's servers\r\n").map_err(failed)?;
    sleep(FIRST_FRAME);
    let one = pending.len() == 1;
    // The cursor, and the boxes ticked: the dialog of one opens on its last choice, the
    // dialog of several on its first box, every box ticked.
    let mut on = if one { 2 } else { 0 };
    let mut ticked = vec![true; pending.len()];
    let draw = |on: usize, ticked: &[bool]| {
        if one {
            draw_server(&pending[0], on)
        } else {
            draw_servers(&pending, ticked, on)
        }
    };
    draw(on, &ticked).map_err(failed)?;
    let mut drops = own_var("CF_TEST_TRUST_SERVER_DROPS_FIRST_KEY").is_some();
    // Where it answers, which servers it uses; the rest it leaves out.
    let using: Vec<bool> = 'asked: loop {
        let Some(given) = terminal.chunk() else {
            return Ok(());
        };
        let given = String::from_utf8_lossy(&given)
            .replace("\u{1b}[200~", "")
            .replace("\u{1b}[201~", "");
        if drops && !given.is_empty() {
            drops = false;
            append(config, KEYS, "dropped").map_err(failed)?;
            continue;
        }
        let mut moved = false;
        for key in keys_in(&given) {
            match key {
                // The dialog of one goes round; the dialog of several stops at its ends.
                Key::Down => {
                    append(config, KEYS, "down").map_err(failed)?;
                    on = if one {
                        (on + 1) % SERVER_CHOICES.len()
                    } else {
                        (on + 1).min(pending.len())
                    };
                    moved = true;
                }
                Key::Up => {
                    append(config, KEYS, "up").map_err(failed)?;
                    on = if one {
                        (on + SERVER_CHOICES.len() - 1) % SERVER_CHOICES.len()
                    } else {
                        on.saturating_sub(1)
                    };
                    moved = true;
                }
                Key::Enter => {
                    append(config, KEYS, "enter").map_err(failed)?;
                    // A dialog that leaves them out whatever its cursor is on.
                    if own_var("CF_TEST_TRUST_SERVER_REFUSES").is_some() {
                        break 'asked vec![false; pending.len()];
                    }
                    if one {
                        if on == 1 {
                            settings["enableAllProjectMcpServers"] = serde_json::json!(true);
                        }
                        break 'asked vec![on != 2];
                    }
                    if on == pending.len() {
                        break 'asked ticked.clone();
                    }
                    // Enter on a box unticks it, as a space does.
                    ticked[on] = !ticked[on];
                    moved = true;
                }
                Key::Escape => {
                    append(config, KEYS, "escape").map_err(failed)?;
                    break 'asked vec![false; pending.len()];
                }
            }
        }
        if moved {
            draw(on, &ticked).map_err(failed)?;
        }
    };
    for (name, used) in pending.iter().zip(using) {
        let list = if used {
            "enabledMcpjsonServers"
        } else {
            "disabledMcpjsonServers"
        };
        let mut names = settings[list].as_array().cloned().unwrap_or_default();
        names.push(serde_json::json!(name));
        settings[list] = serde_json::json!(names);
    }
    fs::create_dir_all(file.parent().unwrap_or(&folder)).map_err(failed)?;
    fs::write(&file, settings.to_string()).map_err(failed)?;
    say("\u{1b}[2J\u{1b}[H").map_err(failed)
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
    // Of a trusted folder's grants: the repository's root, or the folder out of one,
    // wherever it is not trusted.
    let grants = own_var("CF_TEST_TRUST_GRANTS").is_some();
    let asks = if grants {
        Some(repository_of(&folder).unwrap_or_else(|| folder.clone()))
            .filter(|root| !trusted(config, root))
    } else {
        repository_of(&folder).filter(|root| !trusted(config, root))
    };
    let Some(root) = asks else {
        // One that asks nothing holds its first frame as long, before its prompt.
        if let Some(held) = first_frame {
            opening(held)?;
        }
        return Ok(Answer::GoOn);
    };
    opening(first_frame.unwrap_or(FIRST_FRAME))?;
    let mut yes = false;
    draw(&folder, yes, grants).map_err(failed)?;
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
            // The no to a trusted folder's grants goes on without them.
            if grants {
                say("\u{1b}[2J\u{1b}[H").map_err(failed)?;
                return Ok(Answer::GoOn);
            }
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
                    if let Some(frozen) = millis("CF_TEST_TRUST_FROZEN_AFTER_YES_MS") {
                        sleep(frozen);
                    }
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
                    return Ok(Answer::GoOn);
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
        draw(&folder, yes, grants).map_err(failed)?;
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
