//! What Claude Code's window runs with: settings of its launch's own, and its
//! role as a system prompt of its own.

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::{make_folder, write_file, FileError, Mkdir};
use cf_base::{js, path};
use cf_proto::agents::Harness;
use serde_json::json;

use crate::contract::{Launch, LaunchId};
use crate::shared::launch_files::launch_folder;
use crate::shared::role::write_role;
use crate::shared::shell::cf_hook;
use crate::shared::specmint;

/// How long a member's question waits for the board's answer.
const QUESTION_HOOK_SECONDS: u32 = 3600;

/// Writes the settings file every Claude window launches with, one per
/// launch (like Devin's and Pi's integrations), and says how the window
/// loads it. Each turn ends in a Stop hook, so Claude records
/// `stop_hook_summary` for every finished turn, and a turn is over by it
/// where its own `turn_duration` is not written: 2.1.274 left it off every
/// Calliope turn and 2.1.285 most turns of one long interactive session, a
/// headless `claude -p` never writes it (2.1.304), and 2.1.305, the newest
/// seen, holds it back while swarm tasks or detached tool calls run, so the
/// summary is then the only end record. Without the hook those answers never
/// counted as done. A member's question tool is
/// answered from the board (`board_questions`); the chief's shows Claude's
/// own dialog, where the human answers it.
///
/// A member of a project that chose a size for its Claude windows to compact at
/// is given it here, as `autoCompactWindow` (`compaction`, in tokens): live,
/// Claude Code fires at that window less its own margin, the launch's file
/// outranks the owner's saved setting and `/autocompact` typed in the window
/// (T-1016, 2.1.316 on macOS, 2.1.312 on Windows), and nothing is written to the
/// owner's files. With none the file is as it always was.
pub(crate) fn settings(
    env: &Env,
    launch: &LaunchId,
    board_questions: bool,
    compaction: Option<u32>,
    pane_cf: &str,
    directory: Option<&str>,
) -> Result<Vec<String>, String> {
    let root = launch_folder("claude", env, launch)?;
    let said = |error: FileError| error.to_string();
    make_folder(Path::new(&root), 0o700, Mkdir::Promise).map_err(said)?;
    let turn_end = json!({ "hooks": [{ "type": "command", "command": "exit 0" }] });
    // Claude's question tool prompts even in full-permission mode. A
    // member's is answered from the board through this hook, run by the
    // window's own `cf` named in full (a login shell, as Claude Code's Git Bash
    // is on Windows, can put another `cf` ahead of it on the PATH); when nobody
    // answers within the hour, the hook is cancelled and the window shows
    // Claude's own dialog.
    let question = json!({
        "matcher": "AskUserQuestion",
        "hooks": [{ "type": "command", "command": cf_hook(pane_cf, "claude", &[]), "timeout": QUESTION_HOOK_SECONDS }],
    });
    let settings = path::join(&[&root, "settings.json"]);
    let mut file = json!({
        // Full permission (the window's flags) without its one-time
        // acceptance dialog, which no one could answer in a host-started pane.
        "permissions": { "defaultMode": "bypassPermissions" },
        "skipDangerousModePermissionPrompt": true,
        // The classic renderer writes to the terminal's own scrollback, which
        // the dock scrolls; the fullscreen one draws on the alternate screen,
        // which has none.
        "tui": "default",
        "hooks": {
            "PreToolUse": if board_questions { vec![question] } else { Vec::new() },
            "Stop": [turn_end],
        },
    });
    if let Some(window) = compaction {
        file["autoCompactWindow"] = json!(window);
    }
    // On a Mac, the two libraries that no coding task needs and that macOS asks about in the
    // app's name (T-1194). Nothing on Windows, where the file is as it always was.
    let reads = directory.map_or_else(Vec::new, |directory| private_reads(env, directory));
    if !reads.is_empty() {
        file["permissions"]["deny"] = json!(reads);
    }
    let text = js::stringify(&file);
    write_file(Path::new(&settings), text.as_bytes(), 0o600).map_err(said)?;
    Ok(vec!["--settings".to_owned(), settings])
}

/// The reads Claude's tools are denied in a window's settings file on a Mac (T-1194): the Apple
/// Music library and the Photos libraries. Touching either raises a macOS dialog in the app's
/// name on the Mac's screen, and the command waits until someone answers there. Claude Code
/// applies a deny rule in the permission mode the windows run in, to its Read, Grep and Glob and
/// to the path commands of a Bash call that name the folder; a walk that starts above it (`find
/// ~`) is not stopped by it. A project that lives in one of them is not denied its own folder.
/// Nothing is denied on Windows. Never in the owner's own settings: the file is the launch's.
pub(crate) fn private_reads(env: &Env, directory: &str) -> Vec<&'static str> {
    if env.on_windows() {
        return Vec::new();
    }
    let home = env
        .path("HOME")
        .map(|home| home.to_string_lossy().trim_end_matches('/').to_owned());
    let in_music = home.as_deref().is_some_and(|home| {
        let real = cf_base::file::real_path(Path::new(home))
            .to_string_lossy()
            .into_owned();
        [home, real.as_str()]
            .iter()
            .any(|home| within(directory, &format!("{home}/Music")))
    });
    let in_photos = directory
        .split('/')
        .any(|part| part.ends_with(".photoslibrary"));
    let mut reads = Vec::new();
    if !in_music {
        reads.extend(["Read(~/Music)", "Read(~/Music/**)"]);
    }
    if !in_photos {
        reads.extend([
            "Read(~/Pictures/*.photoslibrary)",
            "Read(~/Pictures/*.photoslibrary/**)",
        ]);
    }
    reads
}

/// Whether `folder` is `inside` or a folder in it.
fn within(folder: &str, inside: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    folder == inside || folder.starts_with(&format!("{inside}/"))
}

/// Writes the window's role and says how Claude loads it: the role's folder
/// added to what it reads, its text appended to the system prompt, and no
/// snapshot of that prompt, which a resumed conversation otherwise keeps
/// from its first turn. A chief given specs has ConsensFlow's own copy of its
/// Specmint skill written where it is kept for every launch, and that folder
/// is added to what Claude reads as well, so the skill loads with the role's.
///
/// The role's file is a skill in the role's folder (the layout of 3.0.0's first
/// windows, which loaded the role as a skill alone), and Claude lists every skill it
/// reads to a sub-agent: a `general-purpose` sub-agent of a member listed the role's
/// and, asked to, loaded the whole role (T-766, Claude Code 2.1.310). Nothing has the
/// model load it, for the window has the role in its system prompt from the first
/// turn; so its front matter says the model may not ([`not_for_the_model`]), and the
/// window keeps the role where a sub-agent never sees it. A sub-agent forked from the
/// window carries the window's whole prompt, the role with it: the role text's own
/// sentence for a sub-agent is for that one (`skill/core`).
pub(super) fn role(env: &Env, launch: &Launch) -> Result<Vec<String>, String> {
    let written = write_role(
        Harness::Claude,
        launch.role,
        env,
        launch.id,
        &not_for_the_model(launch.instructions),
    )?;
    let mut args = vec!["--add-dir".to_owned(), written.root];
    if let Some(copy) = specmint::write_own(Harness::Claude, env, launch)? {
        args.extend(["--add-dir".to_owned(), copy.root]);
    }
    args.extend([
        "--append-system-prompt-file".to_owned(),
        written.file,
        "--system-prompt-snapshot".to_owned(),
        "off".to_owned(),
    ]);
    Ok(args)
}

/// The front matter line of a skill the model may not load: Claude lists it to no
/// agent, sub-agents included, and only a person may run it.
const NOT_FOR_THE_MODEL: &str = "disable-model-invocation: true";

/// `text`, a role's, with [`NOT_FOR_THE_MODEL`] first in its front matter: in the one
/// it opens with, or in one of its own put before a text that has none (a card's).
/// An empty text stays empty, for the role refuses it.
fn not_for_the_model(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    match text.strip_prefix("---\n") {
        Some(rest) => format!("---\n{NOT_FOR_THE_MODEL}\n{rest}"),
        None => format!("---\n{NOT_FOR_THE_MODEL}\n---\n\n{text}"),
    }
}

#[cfg(test)]
mod tests;
