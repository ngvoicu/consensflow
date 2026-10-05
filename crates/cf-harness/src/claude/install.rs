//! What Claude Code's window runs with (`src/claude-install.js`, and
//! Claude's branch of `roleConfiguration`, `src/role-skills.js`): settings
//! of its launch's own, and its role as a system prompt of its own.

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::{make_folder, write_file, FileError};
use cf_base::{js, path};
use cf_proto::agents::Harness;
use serde_json::json;

use crate::contract::{Launch, LaunchId};
use crate::shared::launch_files::launch_folder;
use crate::shared::role::write_role;

/// How long a member's question waits for the board's answer.
const QUESTION_HOOK_SECONDS: u32 = 3600;

/// Writes the settings file every Claude window launches with, one per
/// launch (like Devin's and Pi's integrations), and says how the window
/// loads it. Each turn ends in a Stop hook, so Claude records
/// `stop_hook_summary` for every finished turn: its own `turn_duration`
/// record is missing on some turns (every Calliope turn on 2.1.274), and
/// those answers never counted as done. A member's question tool is
/// answered from the board (`board_questions`); the chief's shows Claude's
/// own dialog, where the human answers it.
pub(super) fn settings(
    env: &Env,
    launch: &LaunchId,
    board_questions: bool,
) -> Result<Vec<String>, String> {
    let root = launch_folder("claude", env, launch)?;
    let said = |error: FileError| error.to_string();
    make_folder(Path::new(&root), 0o700).map_err(said)?;
    let turn_end = json!({ "hooks": [{ "type": "command", "command": "exit 0" }] });
    // Claude's question tool prompts even in full-permission mode. A
    // member's is answered from the board through this hook (`cf` is first
    // on a pane's PATH); when nobody answers within the hour, the hook is
    // cancelled and the window shows Claude's own dialog.
    let question = json!({
        "matcher": "AskUserQuestion",
        "hooks": [{ "type": "command", "command": "cf hook claude", "timeout": QUESTION_HOOK_SECONDS }],
    });
    let settings = path::join(&[&root, "settings.json"]);
    let text = js::stringify(&json!({
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
    }));
    write_file(Path::new(&settings), text.as_bytes(), 0o600).map_err(said)?;
    Ok(vec!["--settings".to_owned(), settings])
}

/// Writes the window's role and says how Claude loads it: the role's folder
/// added to what it reads, its text appended to the system prompt, and no
/// snapshot of that prompt, which a resumed conversation otherwise keeps
/// from its first turn.
pub(super) fn role(env: &Env, launch: &Launch) -> Result<Vec<String>, String> {
    let written = write_role(
        Harness::Claude,
        launch.role,
        env,
        launch.id,
        launch.instructions,
    )?;
    Ok(vec![
        "--add-dir".to_owned(),
        written.root,
        "--append-system-prompt-file".to_owned(),
        written.file,
        "--system-prompt-snapshot".to_owned(),
        "off".to_owned(),
    ])
}

#[cfg(test)]
mod tests;
