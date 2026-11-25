//! A Codex window's role. Codex is given the role's text itself, appended to
//! the instructions it would have read anyway: the native resolver is asked for
//! them, so the profile and project layering is kept, and no version is read.
//!
//! The text goes in a file of the launch's own, which the launch's environment
//! names (`CF_CODEX_ROLE_FILE`). On Unix it is 0600 in a folder 0700, readable by
//! this user alone; Windows takes no mode there (cf-base's `make_folder` and
//! `write_file`), and the file is as private as the folder it is in, under
//! ConsensFlow's own in the user's profile (`AppData`), whose inherited permissions give
//! the user, SYSTEM and administrators alone, as Devin's role file is. The window's supervisor
//! reads it before Codex starts, and its broker gives it to the TUI's main starts and to
//! the resumes and forks of the window's own threads as their developer instructions
//! (`developerInstructions`, which replace
//! what Codex's configuration says for that thread: probed on Codex 0.164.0, so the
//! text holds Codex's own as well). It is on no command line, where every user of
//! the machine could read it (`ps` is setuid root on macOS) and a pattern matching
//! its words matched the window; a role's length is no command line's limit either.
//! The file goes with the launch's folder, when its window closes.
//!
//! The broker gives it to the resumes and forks of the window's own threads too (the one
//! it was launched to resume, those its main starts began, and their forks: never a
//! sub-agent's, T-1402): a thread's turns read
//! the developer message its first turn wrote, which a resumed thread keeps
//! ([`crate::contract::Adapter::keeps_first_instructions`], which the Codex adapter
//! answers yes to), but a compaction rebuilds that message from the instructions of
//! the session that compacts, and a resume given none lost the role there (T-1388,
//! T-1394). A role that changed since the thread began reaches its turns as a
//! message the engine says.
//!
//! A sub-agent the window's agent starts (Codex's `spawn_agent`) is a thread the
//! app-server starts itself, and it began with the window's role: it held the
//! member's role in its developer message, on every route (T-766, Codex 0.164.0),
//! and one given something it could not answer asked the board as the member, 4
//! children of 4. So a launch writes a second file beside the role's, the
//! configuration a child of each of Codex's built-in agent types starts with
//! (`default`, `worker`, `explorer`; Codex 0.166.0 has no other): Codex's own
//! instructions, then a sentence that says the child is not the window's agent and
//! never runs `cf` ([`children`]). The supervisor names it to the app-server alone
//! (`-c agents.<type>.config_file=…`), which starts each such child from it in
//! place of the window's role. A child forked with the window's whole history
//! (`fork_context`) carries the role in that history, and one of an agent type the
//! owner defined in their own configuration starts from what they wrote: those are
//! what the role text's own sentence for a sub-agent is for (`skill/core`).
//!
//! A child's commands have the window's board token all the same (T-766 found
//! nothing that tells a child's shell from the window's own, on any harness):
//! ConsensFlow scopes no token to who holds it, and keeps the role, not the token,
//! from a child.

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{make_folder, write_file, Mkdir};
use cf_base::js;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use cf_proto::codex::agent_types::AGENT_TYPES;
use cf_proto::codex::{ROLE_VARIABLE, SUBAGENT_TYPES_VARIABLE, SUBAGENT_VARIABLE};
use serde_json::{json, Value};

use crate::contract::LaunchId;
use crate::seams::processes::{Child, Ending, Program, Streams};
use crate::seams::{arm, Services};
use crate::shared::launch_files::{launch_folder, CODEX_FOLDER};

/// Why a launch is refused when Codex's own instructions could not be read.
const UNREAD: &str = "Cannot read native Codex instructions safely";

/// How long Codex's app-server has to answer `initialize`, from the moment it starts.
const START_WITHIN_MS: u64 = 10_000;

/// How long it then has to read its configuration: its first start in a home indexes
/// that home's sessions before it answers, and that grows with their number.
const READ_WITHIN_MS: u64 = 60_000;

/// How many times the app-server is asked, when it is silent.
const ASKS: u32 = 2;

/// The most a line of the app-server's may say.
const LINE_LIMIT: usize = 2 * 1024 * 1024;

/// The name of the role's file in its launch's folder.
const FILE: &str = "developer-instructions.md";

/// The name of the file of the configuration its sub-agents start with, in its
/// launch's folder.
const CHILDREN_FILE: &str = "sub-agents.toml";

/// The developer instructions of a window and of the sub-agents its agent starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Texts {
    /// The window's own: Codex's instructions, then the role.
    pub(super) role: String,
    /// A sub-agent's: Codex's instructions, then that it is not the window's agent.
    pub(super) children: String,
    /// Codex's own agent types whose sub-agents start from [`Texts::children`]: those
    /// the owner's configuration does not define. One it defines is the owner's, as it
    /// was: a sub-agent of it starts as the owner's configuration says (with the role,
    /// unless that sets instructions of its own), and the role's own sentence for a
    /// sub-agent is what it then has.
    pub(super) kinds: Vec<&'static str>,
}

/// The texts a window of `role` is given as developer instructions, its own and its
/// sub-agents': Codex's own instructions for a window opened in `cwd`, then the
/// role's `content` for the window, and for a sub-agent [`children`]'s sentence.
pub(super) async fn text(
    services: &Services,
    role: &str,
    executable: &str,
    cwd: &str,
    content: &str,
) -> Result<Texts, String> {
    if content.is_empty() {
        return Err(format!("the {role} window needs its role text"));
    }
    let (existing, owned) = instructions(services, executable, cwd).await?;
    Ok(Texts {
        role: format!(
            "{existing}\n\nYour ConsensFlow role follows; it is already loaded, not a task.\n\n{content}"
        ),
        children: format!("{existing}\n\n{}", children(role)),
        kinds: AGENT_TYPES
            .iter()
            .map(|kind| kind.name)
            .filter(|name| !owned.contains(name))
            .collect(),
    })
}

/// What a sub-agent of a window of `role` is told in place of the role: "You are a
/// sub-agent of a ConsensFlow {role}; you have no board identity, so never run cf."
/// The release's `live subagent` run checks the behavior with this shorter sentence.
pub(super) fn children(role: &str) -> String {
    format!(
        "You are a sub-agent of a ConsensFlow {role}; you have no board identity, so never run cf."
    )
}

/// Writes `text` as the role of `launch`'s window, in its own folder made private
/// to this user (by its modes on Unix; on Windows by where it is, see the module),
/// and says the variable of the window's environment that names it.
pub(super) fn write(env: &Env, launch: &LaunchId, text: &str) -> Result<(String, String), String> {
    let folder = launch_folder(CODEX_FOLDER, env, launch)?;
    let file = path::join(&[&folder, FILE]);
    let said = |error: cf_base::file::FileError| error.to_string();
    make_folder(Path::new(&folder), 0o700, Mkdir::Promise).map_err(said)?;
    write_file(Path::new(&file), text.as_bytes(), 0o600).map_err(said)?;
    Ok((ROLE_VARIABLE.to_owned(), file))
}

/// Writes the configuration the sub-agents of `launch`'s window start with, whose
/// developer instructions are `texts`' children's, beside its role and as private, and
/// says the variables of the window's environment that name it and the agent types it is
/// for. Where the owner defines all of Codex's own types, nothing is written or said.
pub(super) fn write_children(
    env: &Env,
    launch: &LaunchId,
    texts: &Texts,
) -> Result<Vec<(String, String)>, String> {
    if texts.kinds.is_empty() {
        return Ok(Vec::new());
    }
    let folder = launch_folder(CODEX_FOLDER, env, launch)?;
    let file = path::join(&[&folder, CHILDREN_FILE]);
    let said = |error: cf_base::file::FileError| error.to_string();
    make_folder(Path::new(&folder), 0o700, Mkdir::Promise).map_err(said)?;
    let configuration = format!(
        "developer_instructions = {}\n",
        toml_string(&texts.children)
    );
    write_file(Path::new(&file), configuration.as_bytes(), 0o600).map_err(said)?;
    Ok(vec![
        (SUBAGENT_VARIABLE.to_owned(), file),
        (SUBAGENT_TYPES_VARIABLE.to_owned(), texts.kinds.join(",")),
    ])
}

/// `text` as a TOML basic string, quotes included: a backslash, a quote and a line's
/// end escaped, and every other character TOML takes in no basic string (the controls
/// but the tab, and DEL) as its `\uXXXX`.
fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\t' => quoted.push('\t'),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\u{0}'..='\u{1f}' | '\u{7f}' => {
                quoted.push_str(&format!("\\u{:04X}", u32::from(character)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// What of Codex's own instructions could not be had: its app-server was silent too long,
/// at its start or in the read of its configuration, or it was wrong or refused.
#[derive(Debug, PartialEq, Eq)]
enum Unread {
    /// No answer to `initialize` in time: the program did not start answering.
    Silent,
    /// `initialize` was answered, and the read of the configuration was not in time.
    Preparing,
    /// It would not start, or said what is no answer: a protocol error, never retried.
    Wrong,
}

impl Unread {
    fn words(&self) -> String {
        match self {
            Self::Wrong => UNREAD.to_owned(),
            Self::Silent => format!(
                "{UNREAD}: Codex's program did not start answering within {} seconds (a newly installed Codex may still be under macOS's first-run check, Gatekeeper); it was asked twice",
                START_WITHIN_MS / 1000
            ),
            Self::Preparing => format!(
                "{UNREAD}: Codex started, but was still preparing its sessions after {} seconds; it was asked twice",
                READ_WITHIN_MS / 1000
            ),
        }
    }
}

/// The instructions Codex itself would give a window opened in `cwd`, asked of
/// its app-server, which is then asked to end whatever came of it. A silence is
/// asked again once, as the app-server is then warm (it indexed the sessions of
/// its home on its first start, or macOS finished its check of the program).
async fn instructions(
    services: &Services,
    executable: &str,
    cwd: &str,
) -> Result<(String, Vec<&'static str>), String> {
    let mut asked = 0;
    loop {
        asked += 1;
        match ask(services, executable, cwd).await {
            Ok(said) => return Ok(said),
            Err(Unread::Silent | Unread::Preparing) if asked < ASKS => {}
            Err(unread) => return Err(unread.words()),
        }
    }
}

/// One start of the app-server, asked, then asked to end.
async fn ask(
    services: &Services,
    executable: &str,
    cwd: &str,
) -> Result<(String, Vec<&'static str>), Unread> {
    let program = Program {
        executable: PathBuf::from(executable),
        args: vec!["app-server".to_owned()],
        cwd: Some(PathBuf::from(cwd)),
        env: services.env.clone(),
    };
    let child = services
        .processes
        .spawn(program, Streams::Lines)
        .map_err(|_| Unread::Wrong)?;
    let said = dialogue(services, &*child, cwd).await;
    child.terminate(Ending::Asked);
    said
}

/// The two waits of the dialogue, each under its own timer: the reply to
/// `initialize` from the app-server's start, and the read of its configuration, which
/// grows with the sessions it has to index, from that reply.
async fn dialogue(
    services: &Services,
    child: &dyn Child,
    cwd: &str,
) -> Result<(String, Vec<&'static str>), Unread> {
    let start = arm(&*services.time, START_WITHIN_MS);
    let heard = start
        .bound(initialize(child))
        .await
        .ok_or(Unread::Silent)?
        .ok_or(Unread::Wrong)?;
    drop(start);
    let said = match heard {
        Heard::Answered(said) => said,
        Heard::Initialized => {
            let read = arm(&*services.time, READ_WITHIN_MS);
            read.bound(converse(child, cwd))
                .await
                .ok_or(Unread::Preparing)?
        }
    };
    said.ok_or(Unread::Wrong)
}

/// What the app-server's first answer was.
enum Heard {
    /// To `initialize`.
    Initialized,
    /// To the read of the configuration, asked for or not: heard early.
    Answered(Option<(String, Vec<&'static str>)>),
}

/// The app-server is initialized: its first answer to either request. None
/// where it ends or says too much, speaks no JSON, or refuses.
async fn initialize(child: &dyn Child) -> Option<Heard> {
    send(
        child,
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "consensflow-role-config", "version": "3.0.0" },
                "capabilities": { "experimentalApi": true },
            },
        }),
    )
    .await?;
    loop {
        let message = heard(child).await?;
        match message.get("id").and_then(Value::as_f64) {
            Some(1.0) => {
                return if js::truthy(message.get("error")) {
                    None
                } else {
                    Some(Heard::Initialized)
                };
            }
            Some(2.0) => {
                return Some(Heard::Answered(
                    configured(&message).map(|text| (text, owned(&message))),
                ));
            }
            _ => {}
        }
    }
}

async fn send(child: &dyn Child, message: Value) -> Option<()> {
    child.write_line(&js::stringify(&message)).await.ok()
}

/// The next line of the app-server, as a JSON value that is not null.
///
/// Kept from Node on purpose: a line of `null` fails here, where reading its
/// `id` threw a `TypeError` out of the stream's handler; and JSON nested past
/// 127 levels, or holding a number past a double's range, is no JSON here.
async fn heard(child: &dyn Child) -> Option<Value> {
    let line = child.read_line(LINE_LIMIT).await.ok()??;
    from_slice_lossy(line.as_bytes())
        .ok()
        .filter(|message| !message.is_null())
}

/// The app-server's dialogue once it is initialized: it is told so, then asked for its
/// configuration, and the developer instructions in that are the answer. None
/// where it fails to be: it ends or says too much, speaks no JSON, refuses a
/// request, or has no configuration or instructions that are text.
async fn converse(child: &dyn Child, cwd: &str) -> Option<(String, Vec<&'static str>)> {
    ask_configuration(child, cwd).await?;
    loop {
        let message = heard(child).await?;
        match message.get("id").and_then(Value::as_f64) {
            Some(1.0) => {
                if js::truthy(message.get("error")) {
                    return None;
                }
                ask_configuration(child, cwd).await?;
            }
            Some(2.0) => return configured(&message).map(|text| (text, owned(&message))),
            _ => {}
        }
    }
}

async fn ask_configuration(child: &dyn Child, cwd: &str) -> Option<()> {
    send(child, json!({ "method": "initialized", "params": {} })).await?;
    send(
        child,
        json!({
            "id": 2,
            "method": "config/read",
            "params": { "cwd": cwd, "includeLayers": false },
        }),
    )
    .await
}

/// The developer instructions the app-server's answer to the read of its
/// configuration holds: none, as empty text, or none at all where it is no
/// answer or the instructions are no text.
fn configured(message: &Value) -> Option<String> {
    let configuration = message
        .get("result")
        .and_then(|result| result.get("config"));
    if js::truthy(message.get("error")) || !js::truthy(configuration) {
        return None;
    }
    match configuration?.get("developer_instructions") {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => None,
    }
}

/// Codex's own agent types that the configuration the app-server's answer holds defines
/// (`agents.<type>`, a table where it is a role: `agents` holds settings beside them).
fn owned(message: &Value) -> Vec<&'static str> {
    let agents = &message["result"]["config"]["agents"];
    AGENT_TYPES
        .iter()
        .map(|kind| kind.name)
        .filter(|name| agents.get(name).is_some_and(Value::is_object))
        .collect()
}

#[cfg(test)]
mod tests;
