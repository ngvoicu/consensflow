//! The programs every window of a run starts through, first on the daemon's
//! `PATH` (`~/.consensflow-candidate/evals/bin`). The windows run in
//! full-permission mode on the user's own setup, which reaches their browser,
//! screen and accounts: on 2026-09-26 an eval reviewer called the Claude in Chrome
//! tools, and Codex's setup gained browser and computer-use servers the same day.
//! A scripted run must reach none of them. The product leaves them on for every
//! agent, the chief's and the staff's, so these wrappers are the only switch-off,
//! and they cover every window of a run.
//!
//! - Claude's adds `--strict-mcp-config --no-chrome` (and the chief's effort, which
//!   a member's own, later on its command line, wins over).
//! - Codex's adds the chief's model (or the cheap one: a member's own `--model`
//!   still wins), the chief's reasoning effort, and a disabled definition of every
//!   MCP server Codex would start. Codex also opens on an update prompt whenever a
//!   newer release exists (seen 2026-09-26 with 0.157.0 out), and the product skips
//!   that itself now, so the wrapper adds only what is eval-only.
//! - Pi's, for a Pi chief only, gives a window the chief's model and thinking level
//!   where the window names none (Pi's own subcommands take neither, and a member's
//!   roster agent passes its own). It is a shell script, the Mac's alone.
//!
//! On the Mac a wrapper is a shell script. On Windows a window opens on a `.cmd`
//! only in npm's shape, a program and a script (`"<program>" "<script>" %*`), which
//! the daemon reads for the program and the script and starts itself: so the
//! `.cmd` names Node and a script the run writes beside it, which starts the real
//! program with the flags and every argument it was given, in the same console, and
//! ends as it ends. Ctrl+C is the program's to answer, not the script's.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::files;
use cf_e2e::live::{self, ask, codex, ISOLATION};
use serde_json::{json, Value};

use super::machine::Machine;
use super::options::Options;
use crate::plan::{harness, ChiefSetup};
use crate::{Error, Result};

/// What a wrapper says of itself: why it is there.
const WHY: &str = "Written by cf-evals: eval windows reach no MCP server and no browser.";

/// Where each wrapper of a run is: the program a window of that harness is started
/// on (none for a harness not in the run).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wrappers {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
    pub pi: Option<PathBuf>,
}

impl Wrappers {
    /// The wrapper of the harness whose command is `command`, if the run has one.
    pub fn of(&self, command: &str) -> Option<&Path> {
        match command {
            "claude" => self.claude.as_deref(),
            "codex" => self.codex.as_deref(),
            "pi" => self.pi.as_deref(),
            _ => None,
        }
    }
}

/// The flags Claude Code is started with in a run: no MCP server and no browser,
/// and the chief's reasoning level when the chief is Claude.
pub fn claude_flags(options: &Options) -> Vec<String> {
    let mut flags: Vec<String> = ISOLATION.iter().map(|flag| (*flag).to_owned()).collect();
    if options.chief == "claude" {
        flags.extend(["--effort".to_owned(), options.effort.clone()]);
    }
    flags
}

/// The flags Codex is started with in a run: the model, the chief's reasoning
/// level when the chief is Codex, and every MCP server of `servers` (what
/// `codex mcp list --json` says) switched off.
pub fn codex_flags(options: &Options, chief: &ChiefSetup, servers: &Value) -> Result<Vec<String>> {
    let model = if options.chief == "codex" {
        chief.model.as_str()
    } else {
        harness("codex")?.model
    };
    let mut flags = vec!["-c".to_owned(), format!("model={}", json!(model))];
    if options.chief == "codex" {
        flags.extend([
            "-c".to_owned(),
            format!("model_reasoning_effort={}", json!(options.effort)),
        ]);
    }
    flags.extend(codex::isolation(servers)?);
    Ok(flags)
}

/// `word` as one word of a shell script, whatever it holds.
fn shell_word(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The shell script that starts `real` with `flags` before the words it is given.
fn script(real: &str, flags: &[String]) -> String {
    let words: Vec<String> = std::iter::once(real)
        .chain(flags.iter().map(String::as_str))
        .map(shell_word)
        .collect();
    format!("#!/bin/sh\n# {WHY}\nexec {} \"$@\"\n", words.join(" "))
}

/// The shell script of a Pi chief's wrapper: its model and its thinking level, for
/// a window only (Pi's own subcommands take neither) and only where the window
/// names none: a member's roster agent passes its own, and it must not depend on
/// which of two flags Pi keeps.
fn pi_script(real: &str, model: &str, thinking: &str) -> String {
    let (real, model, thinking) = (shell_word(real), shell_word(model), shell_word(thinking));
    [
        "#!/bin/sh".to_owned(),
        "# Written by cf-evals: the Pi chief's model and thinking level.".to_owned(),
        "case \"$1\" in".to_owned(),
        "  -*|'') ;;".to_owned(),
        format!("  *) exec {real} \"$@\" ;;"),
        "esac".to_owned(),
        format!("case \" $* \" in *\" --model \"*) ;; *) set -- --model {model} \"$@\" ;; esac"),
        format!(
            "case \" $* \" in *\" --thinking \"*) ;; *) set -- --thinking {thinking} \"$@\" ;; esac"
        ),
        format!("exec {real} \"$@\""),
        String::new(),
    ]
    .join("\n")
}

/// The Node script a Windows wrapper runs: it starts the program and the arguments
/// of `argv` (what the window of the real program opens on, flags and all) with the
/// arguments it was given after them.
fn relay(argv: &[String]) -> String {
    [
        format!("// {WHY}"),
        "import { spawn } from 'node:child_process'".to_owned(),
        format!("const [file, ...args] = {}", json!(argv)),
        "for (const signal of ['SIGINT', 'SIGBREAK']) process.on(signal, () => {})".to_owned(),
        "const child = spawn(file, [...args, ...process.argv.slice(2)], { stdio: 'inherit' })"
            .to_owned(),
        "child.on('error', (error) => { console.error(error.message); process.exit(1) })"
            .to_owned(),
        "child.on('exit', (code) => process.exit(code ?? 1))".to_owned(),
        String::new(),
    ]
    .join("\n")
}

/// The `.cmd` of a Windows wrapper: npm's shape, `"<program>" "<script>" %*`, with
/// the batch file's own line ends.
fn cmd(node: &Path, script: &Path) -> String {
    format!(
        "@echo off\r\nREM {WHY}\r\n\"{}\" \"{}\" %*\r\n",
        node.display(),
        script.display()
    )
}

/// The wrapper `name` that starts `real` with `flags`, in `bin`: where a window
/// finds it.
fn emit(machine: &Machine, name: &str, real: &Path, flags: &[String]) -> Result<PathBuf> {
    let bin = machine.bin();
    if !cfg!(windows) {
        let program = bin.join(name);
        files::write_executable(&program, script(&real.to_string_lossy(), flags))?;
        return Ok(program);
    }
    let words: Vec<String> = std::iter::once(real.to_string_lossy().into_owned())
        .chain(flags.iter().cloned())
        .collect();
    // What a window of the real program opens on: its program and script, which a
    // `.cmd` shim of npm's is read for.
    let env = [("PATH".to_owned(), machine.path())];
    let argv = ask::program(&words, &env)?;
    let node = live::command("node")?;
    let relay_file = bin.join(format!("{name}.mjs"));
    files::write(&relay_file, relay(&argv))?;
    let program = bin.join(format!("{name}.cmd"));
    files::write_executable(&program, cmd(&node, &relay_file))?;
    Ok(program)
}

/// Writes the wrappers a run needs, in a folder of its own, fresh: a wrapper
/// written for another chief must not outlive its run.
pub fn write(machine: &Machine, options: &Options, chief: &ChiefSetup) -> Result<Wrappers> {
    let bin = machine.bin();
    // A folder that is not there is already fresh.
    let _ = fs::remove_dir_all(&bin);
    files::make_dir(&bin)?;
    let mut wrappers = Wrappers::default();
    if options.in_run("claude") {
        let real = machine.install("claude")?.command();
        wrappers.claude = Some(emit(machine, "claude", real, &claude_flags(options))?);
    }
    if options.chief == "pi" {
        if cfg!(windows) {
            return Err(Error::Refused(
                "the Pi chief's wrapper is sh: run Pi chiefs on the Mac".to_owned(),
            ));
        }
        let real = machine.install("pi")?.command();
        let program = bin.join("pi");
        files::write_executable(
            &program,
            pi_script(&real.to_string_lossy(), &chief.model, &options.effort),
        )?;
        wrappers.pi = Some(program);
    }
    if options.in_run("codex") {
        let real = machine.install("codex")?.command();
        let flags = codex_flags(options, chief, &codex::mcp_servers(real)?)?;
        wrappers.codex = Some(emit(machine, "codex", real, &flags)?);
    }
    Ok(wrappers)
}

#[cfg(test)]
mod tests;
