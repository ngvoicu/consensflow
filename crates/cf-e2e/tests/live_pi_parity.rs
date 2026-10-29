//! Live: a Pi member's window has what the same Pi has opened bare, by a person, in
//! the same folder with the same config (Gabriel, 2026-10-25: members have every
//! feature of their harness's CLI; T-549). It is opt-in (`cargo xtask live
//! pi-parity`): it opens real Pi windows, so it is ignored unless asked for and is
//! no part of `check` or of CI. It spends no model quota: no message is sent, and Pi
//! runs offline (`PI_OFFLINE`) on a provider key that is none.
//!
//! The windows run in the pane host of a rig, with a home of the run's own as the
//! user's home, whose Pi folder (`~/.pi/agent`) holds one of each thing a person
//! keeps there: settings (a default model and thinking level), an `AGENTS.md`, an
//! `APPEND_SYSTEM.md`, a skill (and one in `~/.agents/skills`), an extension that
//! adds a tool and a command, a prompt template and an MCP server. The window's
//! folder holds the project's own of each (`.pi/...`, `.agents/skills`, its
//! `AGENTS.md`), which Pi loads in a trusted folder: the user trusts every folder
//! (`defaultProjectTrust`), as a person who answered Pi's question does. The owner's
//! own `~/.pi` is never read or written.
//!
//! One more extension of the user's is the probe ([`PROBE`]): once the window has
//! started and its MCP servers have connected, it writes what the window has (its
//! tools and the active ones, its commands, the markers of each file found in its
//! system prompt, its model and thinking level) to the file `PROBE_OUT` names, and
//! shuts the window. Each case opens Pi bare (`pi`, nothing else) and then a worker's
//! window as the adapter prepares it (`harness-ask pi_launch`), and the worker's must
//! have everything the bare one has. The cases: the user's `APPEND_SYSTEM.md` alone,
//! then the project's beside it, which Pi reads instead: given an
//! `--append-system-prompt`, Pi reads neither by itself, and before T-549 a member had
//! neither.
//!
//! ```text
//! cargo xtask live pi-parity
//! cargo xtask windows --host zeewin -- cargo xtask live pi-parity
//! ```

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::machine::live_folder_in;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Member, Open};
use cf_e2e::live::{self, ask, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The folder the windows open in, below the live folders of the run's home.
const FOLDER: &str = "pi-parity";

/// How long a window has to write what it has: Pi's start, and its MCP servers'.
const WRITTEN: Duration = Duration::from_secs(90);

/// The probe, an extension of the user's: what the window has, written to
/// `PROBE_OUT` once every `session_start` handler is done and the MCP servers had
/// time to connect, and then the window shut.
const PROBE: &str = r#"import { writeFileSync } from 'node:fs'
const MARKERS = ['MARK_USER_AGENTS', 'MARK_PROJECT_AGENTS', 'MARK_USER_APPEND',
  'MARK_PROJECT_APPEND', 'user-skill', 'agents-skill', 'project-skill', 'project-agents-skill']
export default function (pi) {
  pi.on('session_start', (_event, ctx) => {
    setTimeout(() => {
      const prompt = ctx.getSystemPrompt()
      writeFileSync(process.env.PROBE_OUT, JSON.stringify({
        tools: pi.getAllTools().map((t) => t.name).sort(),
        active: [...pi.getActiveTools()].sort(),
        commands: pi.getCommands().map((c) => `${c.source}:${c.name}`).sort(),
        markers: MARKERS.filter((m) => prompt.includes(m)),
        model: ctx.model ? `${ctx.model.provider}/${ctx.model.id}` : null,
        thinking: pi.getThinkingLevel(),
        trusted: ctx.isProjectTrusted(),
      }))
      ctx.shutdown()
    }, 8000)
  })
}
"#;

/// A stdio MCP server with one tool, `<name>_tool`, its name the first argument.
const MCP_SERVER: &str = r#"import { createInterface } from 'node:readline'
const name = process.argv[2]
const send = (msg) => process.stdout.write(`${JSON.stringify(msg)}\n`)
createInterface({ input: process.stdin }).on('line', (line) => {
  let req
  try { req = JSON.parse(line) } catch { return }
  if (req.id === undefined) return
  const result = req.method === 'initialize'
    ? { protocolVersion: req.params?.protocolVersion ?? '2025-06-18', capabilities: { tools: {} },
        serverInfo: { name, version: '1.0.0' } }
    : req.method === 'tools/list'
      ? { tools: [{ name: `${name}_tool`, description: 'probe', inputSchema: { type: 'object', properties: {} } }] }
      : {}
  send({ jsonrpc: '2.0', id: req.id, result })
})
"#;

/// The model the user's settings choose, on the provider whose key is none.
const PROVIDER: &str = "openrouter";
const MODEL: &str = "anthropic/claude-sonnet-4.5";

/// A case: whether the project has an `APPEND_SYSTEM.md` of its own, and the marker
/// of the one Pi then appends.
struct Case {
    name: &'static str,
    project_append: bool,
    appended: &'static str,
}

const CASES: [Case; 2] = [
    Case {
        name: "the user's APPEND_SYSTEM.md",
        project_append: false,
        appended: "MARK_USER_APPEND",
    },
    Case {
        name: "the project's APPEND_SYSTEM.md",
        project_append: true,
        appended: "MARK_PROJECT_APPEND",
    },
];

/// A skill called `name` in the skills folder `folder`.
fn skill(folder: &Path, name: &str) -> Result {
    files::write(
        &folder.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} of the parity run\n---\n\nbody\n"),
    )
}

/// An extension that adds the tool `<name>_tool` and the command `<name>-cmd`.
fn extension(file: &Path, name: &str) -> Result {
    files::write(
        file,
        format!(
            "export default function (pi) {{\n  pi.registerCommand('{name}-cmd', {{ description: 'probe', handler: async () => {{}} }})\n  pi.registerTool({{ name: '{name}_tool', label: '{name}', description: 'probe', parameters: {{ type: 'object', properties: {{}} }}, execute: async () => ({{ content: [] }}) }})\n}}\n"
        ),
    )
}

/// An `mcp.json` with the server `name`, whose tools are declared to the model.
fn mcp(file: &Path, server: &Path, name: &str) -> Result {
    let config = json!({ "mcpServers": { name: {
        "command": "node", "args": [server, name], "exposure": "direct",
    } } });
    files::write(file, config.to_string())
}

/// One of each thing a person keeps in Pi's folder of `home`, the probe among them.
fn user_config(home: &Path) -> Result {
    let agent = home.join(".pi").join("agent");
    let settings = json!({
        "defaultProvider": PROVIDER, "defaultModel": MODEL, "defaultThinkingLevel": "low",
        "defaultProjectTrust": "always", "lastChangelogVersion": "99.0.0",
    });
    files::write(&agent.join("settings.json"), settings.to_string())?;
    files::write(&agent.join("AGENTS.md"), "MARK_USER_AGENTS\n")?;
    files::write(&agent.join("APPEND_SYSTEM.md"), "MARK_USER_APPEND\n")?;
    skill(&agent.join("skills"), "user-skill")?;
    skill(&home.join(".agents").join("skills"), "agents-skill")?;
    extension(&agent.join("extensions").join("user-ext.ts"), "user_ext")?;
    files::write(&agent.join("extensions").join("probe.ts"), PROBE)?;
    files::write(
        &agent.join("prompts").join("user-prompt.md"),
        "---\ndescription: probe\n---\nhi\n",
    )?;
    let server = home.join("mcp-server.mjs");
    files::write(&server, MCP_SERVER)?;
    mcp(&agent.join("mcp.json"), &server, "user_mcp")
}

/// The project's own of each in `folder`, with an `APPEND_SYSTEM.md` where `append`.
fn project_config(folder: &Path, server: &Path, append: bool) -> Result {
    let pi = folder.join(".pi");
    files::write(&folder.join("AGENTS.md"), "MARK_PROJECT_AGENTS\n")?;
    skill(&pi.join("skills"), "project-skill")?;
    skill(
        &folder.join(".agents").join("skills"),
        "project-agents-skill",
    )?;
    extension(&pi.join("extensions").join("project-ext.ts"), "project_ext")?;
    files::write(
        &pi.join("prompts").join("project-prompt.md"),
        "---\ndescription: probe\n---\nhi\n",
    )?;
    mcp(&pi.join("mcp.json"), server, "project_mcp")?;
    let appended = pi.join("APPEND_SYSTEM.md");
    if append {
        files::write(&appended, "MARK_PROJECT_APPEND\n")
    } else {
        match std::fs::remove_file(&appended) {
            Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(Error::File {
                action: "remove",
                path: appended,
                source,
            }),
            _ => Ok(()),
        }
    }
}

/// What the probe of a window wrote to `out`, waited for.
fn written(out: &Path) -> Result<Value> {
    let began = Instant::now();
    while began.elapsed() < WRITTEN {
        if let Ok(text) = files::read_string(out) {
            if let Ok(value) = serde_json::from_str(&text) {
                return Ok(value);
            }
        }
        sleep(Duration::from_millis(500));
    }
    Err(Error::Daemon(format!(
        "no window wrote what it has to {} within {} s",
        out.display(),
        WRITTEN.as_secs()
    )))
}

/// Pi opened bare in `folder`, as a person opens it: `pi`, nothing else.
fn bare(rig: &Rig, install: &Installed, folder: &Path, id: &str, out: &Path) -> Result {
    let program = install.command().to_string_lossy().into_owned();
    let argv = ask::program(&[program], rig.environment())?;
    let opened = rig.host(
        "pane.open",
        json!({
            "id": id, "generation": 1, "cwd": folder, "argv": argv,
            "env": { "PROBE_OUT": out }, "dropEnv": [],
            "size": { "rows": 40, "cols": 120 },
        }),
    )?;
    if opened["ok"] != true {
        return Err(Error::Daemon(format!("bare Pi did not open: {opened}")));
    }
    Ok(())
}

/// What the member's window lacks of what the bare one has: each tool, active
/// tool, command and marker it has not, and a model, thinking level or trust that
/// differs.
fn lost(bare: &Value, member: &Value) -> Vec<String> {
    let mut lost = Vec::new();
    for list in ["tools", "active", "commands", "markers"] {
        let theirs = member[list].as_array().cloned().unwrap_or_default();
        for item in bare[list].as_array().into_iter().flatten() {
            if !theirs.contains(item) {
                lost.push(format!("{list}: {item}"));
            }
        }
    }
    for value in ["model", "thinking", "trusted"] {
        if bare[value] != member[value] {
            lost.push(format!("{value}: {} -> {}", bare[value], member[value]));
        }
    }
    lost
}

/// The checks of one case: the bare window has what the run put there (or the
/// probe saw nothing), and the member's has all of it.
fn checks_of(case: &Case, bare: &Value, member: &Value) -> Vec<Check> {
    let has = |value: &Value, list: &str, item: &str| {
        value[list]
            .as_array()
            .is_some_and(|items| items.iter().any(|held| held == item))
    };
    let sees = [
        "user_ext_tool",
        "mcp__user_mcp__user_mcp_tool",
        "project_ext_tool",
    ]
    .iter()
    .all(|tool| has(bare, "tools", tool))
        && has(bare, "markers", case.appended);
    let lost = lost(bare, member);
    vec![
        Check::new(
            case.name,
            "bare Pi has the run's config",
            sees,
            format!("bare: {bare}"),
        ),
        Check::new(
            case.name,
            "the member has all bare Pi has",
            lost.is_empty() && has(member, "markers", case.appended),
            if lost.is_empty() {
                format!("member: {member}")
            } else {
                format!("the member lacks {}", lost.join(", "))
            },
        ),
    ]
}

#[test]
#[ignore = "opt-in: opens real Pi windows, offline and with no message sent; cargo xtask live pi-parity"]
fn a_pi_members_window_has_everything_bare_pi_has_in_the_same_folder() -> Result {
    let log = Log::begin("live pi-parity")?;
    ask::prepare()?;
    // The case is its own home: its windows' HOME holds the Pi folder it made, and none of the
    // owner's is read or written, so the kit's home (`PI_CODING_AGENT_DIR`) is none of its.
    let install = log.found(Harness::Pi)?.on_own_home();
    let users = tempfile::Builder::new()
        .prefix("cf-pi-parity-")
        .tempdir()
        .map_err(|source| Error::File {
            action: "make",
            path: std::env::temp_dir(),
            source,
        })?;
    let home = users.path().join("home");
    user_config(&home)?;
    let config = live::rig(std::slice::from_ref(&install), FAKE_AGENT)
        .var("HOME", home.to_string_lossy())
        .var("USERPROFILE", home.to_string_lossy())
        .var("PI_OFFLINE", "1")
        .var("PI_TELEMETRY", "0")
        .var("OPENROUTER_API_KEY", "not-a-key");
    let rig = Rig::start(config)?;
    let folder = live_folder_in(&home, FOLDER)?;
    let mut checks = Vec::new();
    for (at, case) in CASES.iter().enumerate() {
        project_config(&folder, &home.join("mcp-server.mjs"), case.project_append)?;
        let (bare_out, member_out) = (
            users.path().join(format!("bare-{at}.json")),
            users.path().join(format!("member-{at}.json")),
        );
        bare(&rig, &install, &folder, &format!("bare-{at}"), &bare_out)?;
        let bare_had = written(&bare_out)?;
        let member = Open::new(&rig, &install, FOLDER, &format!("member-{at}"))
            .agent(json!({ "kind": "pi" }))
            .member(Member::new(
                "worker",
                "# ConsensFlow worker\n\nThe parity run's role.",
            ))
            .var("PROBE_OUT", &member_out.to_string_lossy())
            .open(&log)?;
        let member_had = written(&member_out)?;
        member.kill();
        checks.extend(checks_of(case, &bare_had, &member_had));
    }
    rig.close()?;
    report::conclude(&log, "what a Pi member has of bare Pi", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_lacks_what_bare_pi_has_and_it_has_not_and_nothing_it_has_more() {
        let bare = json!({
            "tools": ["read", "user_ext_tool"], "active": ["read"],
            "commands": ["skill:skill:user-skill"], "markers": ["MARK_USER_APPEND"],
            "model": "openrouter/x", "thinking": "low", "trusted": true,
        });
        let mut member = bare.clone();
        member["commands"] = json!(["skill:skill:user-skill", "skill:skill:consensflow-worker"]);
        assert!(lost(&bare, &member).is_empty());
        member["markers"] = json!([]);
        member["thinking"] = json!("high");
        assert_eq!(
            lost(&bare, &member),
            [
                "markers: \"MARK_USER_APPEND\"",
                "thinking: \"low\" -> \"high\""
            ]
        );
    }
}
