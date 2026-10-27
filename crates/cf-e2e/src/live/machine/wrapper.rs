//! The programs a live run puts where the real Claude Code would be started, so
//! that every Claude window of the run starts through one of them: the wrapper
//! that shuts the human's MCP servers, connectors and browser out of it, and the
//! shim that does that and also has the stand-in agent play the windows whose
//! agent's model is `fake` or `fake-chief`. On macOS and Linux both are shell
//! scripts. Windows opens a window only on the shape of an npm shim
//! (`"<program>" "<script>" %*`), so there each is a `.cmd` of that shape that names
//! Node and a script written beside it, which starts the real program (or the stand-in)
//! and ends as it ends ([`windows_wrapper`]).

use std::path::{Path, PathBuf};

use serde_json::json;

use super::path;
use crate::live::agents::{STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL};
use crate::live::ask;
use crate::process::own_var;
use crate::{files, Result};

/// The flags that keep a scripted Claude window from reaching the human's MCP
/// servers, connectors and browser, which it would in full-permission mode on
/// their own setup. The product leaves all of them on for every agent, and
/// test runs are the only place they are shut out.
pub const ISOLATION: [&str; 2] = ["--strict-mcp-config", "--no-chrome"];

/// What a live test's wrapper says of itself: why it is there.
const WHY: &str = "Written by a live test: its Claude windows reach no MCP server and no browser.";

/// A program called `claude-isolated` that runs the real Claude Code at `claude`
/// with [`ISOLATION`] before whatever words it is given, made in `folder`: the
/// stand-in a rig puts first on the `PATH` of its windows is then this one,
/// and every Claude window of the run starts through it. Where it is.
pub fn isolated_claude(folder: &Path, claude: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        return on_windows(folder, "claude-isolated", claude, None, None);
    }
    written(folder, "claude-isolated", isolating_script(claude))
}

/// A program called `claude-shimmed` that runs the stand-in agent at `stand_in`,
/// as it is given the environment (its `CLAUDE_CONFIG_DIR` is the one the rig gives
/// every window, where the daemon looks for the records of a window), for the window
/// whose agent's model is `fake` or `fake-chief`, and the real Claude Code at `claude`
/// as [`isolated_claude`] does for every other: a project whose chief and some
/// members are stand-ins and whose other members are real. Made in `folder`.
/// Where it is.
pub fn shimmed_claude(folder: &Path, claude: &Path, stand_in: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        return on_windows(folder, "claude-shimmed", claude, Some(stand_in), None);
    }
    written(
        folder,
        "claude-shimmed",
        shimming_script(claude, stand_in, None),
    )
}

/// [`shimmed_claude`], whose real Claude Code also loads the MCP servers the file
/// `servers` names, and only those: [`ISOLATION`]'s `--strict-mcp-config` shuts
/// every other out. For the live case that needs a window of the product's own
/// kind, which has a server of its harness's.
pub fn shimmed_claude_with_servers(
    folder: &Path,
    claude: &Path,
    stand_in: &Path,
    servers: &Path,
) -> Result<PathBuf> {
    if cfg!(windows) {
        return on_windows(
            folder,
            "claude-shimmed",
            claude,
            Some(stand_in),
            Some(servers),
        );
    }
    written(
        folder,
        "claude-shimmed",
        shimming_script(claude, stand_in, Some(servers)),
    )
}

/// The program `name` in `folder`, with `script` for its words.
fn written(folder: &Path, name: &str, script: String) -> Result<PathBuf> {
    let program = folder.join(name);
    files::write_executable(&program, script)?;
    Ok(program)
}

/// The words that start the real Claude Code at `claude` the way a live run does: the
/// servers file `servers` names, if it is given one, and [`ISOLATION`], before whatever
/// words a window is given.
fn real_words(claude: &Path, servers: Option<&Path>) -> Vec<String> {
    let mut words = vec![claude.to_string_lossy().into_owned()];
    if let Some(file) = servers {
        words.push("--mcp-config".to_owned());
        words.push(file.to_string_lossy().into_owned());
    }
    words.extend(ISOLATION.map(str::to_owned));
    words
}

/// The wrapper (`stand_in` given: the shim) `name` of the real Claude Code at `claude`, in
/// `folder`, for a Windows machine: the real program is what its window would open on
/// (an npm-installed Claude Code is a `.cmd`, which is read for its Node and script), and
/// the Node that runs the wrapper's script is the one on the `PATH` of this run, as the
/// rig's windows find it.
fn on_windows(
    folder: &Path,
    name: &str,
    claude: &Path,
    stand_in: Option<&Path>,
    servers: Option<&Path>,
) -> Result<PathBuf> {
    windows_wrapper(
        folder,
        name,
        WHY,
        &real_words(claude, servers),
        &own_var("PATH").unwrap_or_default(),
        stand_in,
    )
}

/// A program called `name` for a Windows machine, in `folder`: the `.cmd` in npm's shape
/// that a window opens on (the pane host starts a file with each argument quoted the way
/// programs read them, which `cmd.exe` does not, so the daemon opens a window on a `.cmd`
/// only as the shim npm writes, and reads it for its program and script). It names Node
/// and a script written beside it that starts `words` (the real program, as a window of
/// it opens, then its flags) with the arguments the window is given after them, in the
/// same console, and ends as it ends; Ctrl+C is the program's to answer, not the script's.
/// Where `stand_in` is given, a window whose arguments name a stand-in's model
/// (`fake`, `fake-chief`) starts the stand-in with them instead. `why` is what both
/// files say of themselves, and `search` the `PATH` a shim among `words` is read on.
/// Where the `.cmd` is.
pub fn windows_wrapper(
    folder: &Path,
    name: &str,
    why: &str,
    words: &[String],
    search: &str,
    stand_in: Option<&Path>,
) -> Result<PathBuf> {
    let env = [("PATH".to_owned(), search.to_owned())];
    let argv = ask::program(words, &env)?;
    let node = path::command("node")?;
    write_windows_wrapper(folder, name, why, &node, &argv, stand_in)
}

/// [`windows_wrapper`], once the program the real one's window opens on (`argv`) and the
/// Node (`node`) that runs the script are known.
fn write_windows_wrapper(
    folder: &Path,
    name: &str,
    why: &str,
    node: &Path,
    argv: &[String],
    stand_in: Option<&Path>,
) -> Result<PathBuf> {
    let script = folder.join(format!("{name}.mjs"));
    files::write(&script, relay(why, argv, stand_in))?;
    let program = folder.join(format!("{name}.cmd"));
    files::write_executable(&program, cmd(why, node, &script))?;
    Ok(program)
}

/// The Node script a Windows wrapper runs: it starts the program and the arguments of
/// `argv` with the arguments it was given after them, or, where a `stand_in` is given and
/// one of the arguments it was given is the model of a stand-in agent, the stand-in.
fn relay(why: &str, argv: &[String], stand_in: Option<&Path>) -> String {
    let route = match stand_in {
        None => format!("const [file, ...args] = {}", json!(argv)),
        Some(agent) => [
            format!("const real = {}", json!(argv)),
            format!("const stand_in = {}", json!([agent.to_string_lossy()])),
            format!(
                "const fake = process.argv.slice(2).some((word) => {})",
                [STAND_IN_WORKER_MODEL, STAND_IN_CHIEF_MODEL]
                    .map(|model| format!("word === '{model}'"))
                    .join(" || ")
            ),
            "const [file, ...args] = fake ? stand_in : real".to_owned(),
        ]
        .join("\n"),
    };
    [
        format!("// {why}"),
        "import { spawn } from 'node:child_process'".to_owned(),
        route,
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

/// The `.cmd` of a Windows wrapper: npm's shape, `"<program>" "<script>" %*`, with the
/// batch file's own line ends.
fn cmd(why: &str, node: &Path, script: &Path) -> String {
    format!(
        "@echo off\r\nREM {why}\r\n\"{}\" \"{}\" %*\r\n",
        node.display(),
        script.display()
    )
}

/// The shell script that is [`isolated_claude`].
fn isolating_script(claude: &Path) -> String {
    format!(
        "#!/bin/sh\n# {WHY}\nexec {} {} \"$@\"\n",
        shell_word(&claude.to_string_lossy()),
        ISOLATION.join(" ")
    )
}

/// The shell script that is [`shimmed_claude`]. A window is the stand-in's when
/// one of its words is its agent's model, `fake` or `fake-chief`. `servers` is
/// the file of the MCP servers the real Claude Code loads (alone: the isolation
/// flags shut the rest out); it comes first, before a flag that ends its list.
fn shimming_script(claude: &Path, stand_in: &Path, servers: Option<&Path>) -> String {
    let servers = servers.map_or_else(String::new, |file| {
        format!("--mcp-config {} ", shell_word(&file.to_string_lossy()))
    });
    format!(
        "#!/bin/sh\n\
         # Written by a live test: a window whose agent's model is fake or fake-chief is the\n\
         # stand-in's, which writes its records where the daemon reads Claude's (the folder\n\
         # CLAUDE_CONFIG_DIR names, as it does for the real Claude); every other Claude window\n\
         # reaches no MCP server and no browser.\n\
         for arg in \"$@\"; do\n\
         \x20 case \"$arg\" in\n\
         \x20   fake | fake-chief) exec {} \"$@\" ;;\n\
         \x20 esac\n\
         done\n\
         exec {} {servers}{} \"$@\"\n",
        shell_word(&stand_in.to_string_lossy()),
        shell_word(&claude.to_string_lossy()),
        ISOLATION.join(" ")
    )
}

/// `word` as one word of a shell script, whatever it holds.
fn shell_word(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn claude_is_started_through_a_wrapper_that_adds_the_flags_and_keeps_every_word_given() {
        let script = isolating_script(Path::new("/home/me/.local/bin/claude"));
        assert_eq!(
            script,
            "#!/bin/sh\n# Written by a live test: its Claude windows reach no MCP server and no browser.\n\
             exec '/home/me/.local/bin/claude' --strict-mcp-config --no-chrome \"$@\"\n"
        );
        // A path with a quote in it is still one word.
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word("a b"), "'a b'");
    }

    #[test]
    fn a_shim_made_with_servers_gives_the_real_claude_their_file_before_the_flags_that_shut_the_rest_out(
    ) {
        let script = shimming_script(
            Path::new("/real/claude"),
            Path::new("/the/fake-agent"),
            Some(Path::new("/run/it's/mcp.json")),
        );
        // The list of a flag that takes several files ends at the flag after it.
        assert!(
            script.ends_with(
                "exec '/real/claude' --mcp-config '/run/it'\\''s/mcp.json' --strict-mcp-config --no-chrome \"$@\"\n"
            ),
            "{script}"
        );
        // The stand-in's window is as it was: it has no server of the harness's.
        assert!(
            script.contains("fake | fake-chief) exec '/the/fake-agent' \"$@\" ;;"),
            "{script}"
        );
        assert!(
            !shimming_script(Path::new("/c"), Path::new("/s"), None).contains("--mcp-config"),
            "a shim with no servers names none"
        );
    }

    #[test]
    fn the_shim_hands_the_stand_in_the_environment_as_it_is_and_the_real_claude_the_flags() {
        let script = shimming_script(
            Path::new("/real/claude"),
            Path::new("/the/fake-agent"),
            None,
        );
        assert!(
            script.contains("fake | fake-chief) exec '/the/fake-agent' \"$@\" ;;"),
            "{script}"
        );
        assert!(
            script.ends_with("exec '/real/claude' --strict-mcp-config --no-chrome \"$@\"\n"),
            "{script}"
        );
        // The stand-in comes first: a window that names a fake model is never the real one's.
        assert!(
            script.find("fake-agent").unwrap() < script.find("/real/claude").unwrap(),
            "{script}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_wrapper_is_a_program_that_runs_the_claude_it_was_made_for_with_the_flags_first() {
        let folder = tempfile::tempdir().unwrap();
        let program = isolated_claude(folder.path(), Path::new("/bin/echo")).unwrap();
        assert_eq!(program, folder.path().join("claude-isolated"));
        // Run by the shell: a script just written and started as a program may be
        // refused as a file still open for writing, when another test starts one.
        let ran = crate::process::Run::new("/bin/sh")
            .arg(&program)
            .args(["--settings", "a b"])
            .run()
            .unwrap();
        assert_eq!(
            ran.stdout,
            "--strict-mcp-config --no-chrome --settings a b\n"
        );
    }

    /// What the shim made for `/bin/sh` as the stand-in and `/bin/echo` as Claude
    /// says to the words `words`, with `HOME` the folder `/h` and `CLAUDE_CONFIG_DIR`
    /// the folder `/rig/claude`: the stand-in is a shell that prints the Claude folder
    /// it was given.
    #[cfg(unix)]
    fn shimmed(words: &[&str]) -> String {
        let folder = tempfile::tempdir().unwrap();
        let shim =
            shimmed_claude(folder.path(), Path::new("/bin/echo"), Path::new("/bin/sh")).unwrap();
        assert_eq!(shim, folder.path().join("claude-shimmed"));
        crate::process::Run::new("/bin/sh")
            .arg(&shim)
            .args(words)
            .var("HOME", "/h")
            .var("CLAUDE_CONFIG_DIR", "/rig/claude")
            .run()
            .unwrap()
            .stdout
    }

    #[cfg(unix)]
    #[test]
    fn a_window_of_a_fake_model_is_the_stand_ins_with_the_claude_folder_the_rig_gave_it() {
        for model in ["fake", "fake-chief"] {
            assert_eq!(
                shimmed(&["-c", "echo $CLAUDE_CONFIG_DIR", model]),
                "/rig/claude\n",
                "{model}"
            );
        }
    }

    /// A run's Claude config folder, which the rig gives every window, is where the daemon
    /// reads Claude's records: the stand-in chief must write its own there too, or its
    /// window never reads as idle (T-540's live case met it). The shim forces no folder
    /// of its own (T-302: never the machine's `$HOME/.claude`).
    #[cfg(unix)]
    #[test]
    fn a_stand_in_keeps_the_claude_folder_a_run_was_given() {
        let folder = tempfile::tempdir().unwrap();
        let shim =
            shimmed_claude(folder.path(), Path::new("/bin/echo"), Path::new("/bin/sh")).unwrap();
        let run = |config: Option<&str>| {
            let run = crate::process::Run::new("/bin/sh")
                .arg(&shim)
                .args(["-c", "echo $CLAUDE_CONFIG_DIR", "fake-chief"])
                .var("HOME", "/h");
            match config {
                Some(config) => run.var("CLAUDE_CONFIG_DIR", config),
                None => run,
            }
            .run()
            .unwrap()
            .stdout
        };
        assert_eq!(run(Some("/run/own config")), "/run/own config\n");
        assert_eq!(run(None), "\n", "no folder of the shim's own");
    }

    #[cfg(unix)]
    #[test]
    fn every_other_window_is_the_real_claude_with_the_flags_first_and_its_words_kept() {
        assert_eq!(
            shimmed(&["-c", "echo $CLAUDE_CONFIG_DIR", "claude-sonnet-5-5"]),
            "--strict-mcp-config --no-chrome -c echo $CLAUDE_CONFIG_DIR claude-sonnet-5-5\n"
        );
        // A word that only holds the name of a fake model is not the model.
        assert_eq!(
            shimmed(&["--append", "fakes"]),
            "--strict-mcp-config --no-chrome --append fakes\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_window_of_the_real_claude_is_given_the_servers_file_and_a_fake_one_is_not() {
        let folder = tempfile::tempdir().unwrap();
        let shim = shimmed_claude_with_servers(
            folder.path(),
            Path::new("/bin/echo"),
            Path::new("/bin/sh"),
            Path::new("/run/mcp.json"),
        )
        .unwrap();
        assert_eq!(shim, folder.path().join("claude-shimmed"));
        let run = |words: &[&str]| {
            crate::process::Run::new("/bin/sh")
                .arg(&shim)
                .args(words)
                .var("HOME", "/h")
                .var("CLAUDE_CONFIG_DIR", "/rig/claude")
                .run()
                .unwrap()
                .stdout
        };
        assert_eq!(
            run(&["--model", "claude-sonnet-5-5"]),
            "--mcp-config /run/mcp.json --strict-mcp-config --no-chrome --model claude-sonnet-5-5\n"
        );
        // A window the stand-in plays is started as it was, with no servers file (the
        // stand-in is a shell here, which prints the Claude folder it was given).
        assert_eq!(
            run(&["-c", "echo $CLAUDE_CONFIG_DIR", "fake"]),
            "/rig/claude\n"
        );
        // A run given a Claude config of its own has the stand-in write there, where the
        // daemon reads Claude's records: never in the owner's own (T-432).
        let own = crate::process::Run::new("/bin/sh")
            .arg(&shim)
            .args(["-c", "echo $CLAUDE_CONFIG_DIR", "fake"])
            .var("HOME", "/h")
            .var("CLAUDE_CONFIG_DIR", "/run/own")
            .run()
            .unwrap()
            .stdout;
        assert_eq!(own, "/run/own\n");
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_real_claude_is_started_with_its_servers_file_first_and_the_flags_that_shut_the_rest_out()
    {
        assert_eq!(
            real_words(Path::new("C:/bin/claude.exe"), None),
            strings(&["C:/bin/claude.exe", "--strict-mcp-config", "--no-chrome"])
        );
        // The order the shell scripts have: a flag that takes several files ends its list at the
        // flag after it.
        assert_eq!(
            real_words(Path::new("claude"), Some(Path::new("/run/mcp.json"))),
            strings(&[
                "claude",
                "--mcp-config",
                "/run/mcp.json",
                "--strict-mcp-config",
                "--no-chrome"
            ])
        );
    }

    #[test]
    fn a_windows_wrapper_is_a_cmd_in_the_shape_of_an_npm_shim_that_names_node_and_the_script_it_runs(
    ) {
        let shim = cmd(
            "Written by a test.",
            Path::new(r"C:\Program Files\nodejs\node.exe"),
            Path::new(r"C:\Users\u\bin\claude-shimmed.mjs"),
        );
        assert_eq!(
            shim,
            "@echo off\r\nREM Written by a test.\r\n\
             \"C:\\Program Files\\nodejs\\node.exe\" \"C:\\Users\\u\\bin\\claude-shimmed.mjs\" %*\r\n"
        );
        // The daemon reads the last line that passes the arguments on, and takes the last
        // two words it quotes there for the program and the script: the comment is no such line.
        let line = shim.lines().rev().find(|line| line.contains("%*")).unwrap();
        assert_eq!(line.matches('"').count(), 4);
        assert!(!shim.lines().next().unwrap_or_default().contains("%*"));
    }

    #[test]
    fn the_script_a_windows_wrapper_runs_starts_the_real_program_and_ends_as_it_ends() {
        let text = relay(
            "Why.",
            &strings(&[
                "C:\\node.exe",
                "C:\\cli.js",
                "--no-chrome",
                "-c",
                "model=\"m\"",
            ]),
            None,
        );
        assert!(
            text.contains(
                "const [file, ...args] = [\"C:\\\\node.exe\",\"C:\\\\cli.js\",\"--no-chrome\",\"-c\",\"model=\\\"m\\\"\"]"
            ),
            "{text}"
        );
        assert!(text.starts_with("// Why.\n"), "{text}");
        for part in [
            "import { spawn } from 'node:child_process'",
            "for (const signal of ['SIGINT', 'SIGBREAK']) process.on(signal, () => {})",
            "spawn(file, [...args, ...process.argv.slice(2)], { stdio: 'inherit' })",
            "child.on('exit', (code) => process.exit(code ?? 1))",
        ] {
            assert!(text.contains(part), "{part}");
        }
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_script_of_a_shim_starts_the_stand_in_for_the_model_of_one_and_the_real_program_for_every_other(
    ) {
        let text = relay(
            "Why.",
            &strings(&["C:\\real.exe", "--no-chrome"]),
            Some(Path::new(r"C:\bin\fake-agent.exe")),
        );
        assert!(
            text.contains("const real = [\"C:\\\\real.exe\",\"--no-chrome\"]"),
            "{text}"
        );
        assert!(
            text.contains("const stand_in = [\"C:\\\\bin\\\\fake-agent.exe\"]"),
            "{text}"
        );
        // A word that is the model of a stand-in, as the shell script reads it.
        assert!(
            text.contains(
                "const fake = process.argv.slice(2).some((word) => word === 'fake' || word === 'fake-chief')"
            ),
            "{text}"
        );
        assert!(
            text.contains("const [file, ...args] = fake ? stand_in : real"),
            "{text}"
        );
        // The stand-in is started with the window's arguments as they are given: it takes
        // its own file for none of them, whether it is given it first or not.
        assert!(
            text.contains("spawn(file, [...args, ...process.argv.slice(2)]"),
            "{text}"
        );
    }

    #[test]
    fn a_windows_wrapper_is_the_cmd_of_a_script_written_beside_it_under_the_name_it_was_given() {
        let folder = tempfile::tempdir().unwrap();
        let program = write_windows_wrapper(
            folder.path(),
            "claude-shimmed",
            "Why.",
            Path::new(r"C:\nodejs\node.exe"),
            &strings(&[r"C:\real.exe"]),
            Some(Path::new(r"C:\fake-agent.exe")),
        )
        .unwrap();
        assert_eq!(program, folder.path().join("claude-shimmed.cmd"));
        let shim = fs::read_to_string(&program).unwrap();
        let script = folder.path().join("claude-shimmed.mjs");
        assert!(
            shim.ends_with(&format!(
                "\"C:\\nodejs\\node.exe\" \"{}\" %*\r\n",
                script.display()
            )),
            "{shim}"
        );
        assert!(fs::read_to_string(&script).unwrap().contains("real.exe"));
    }

    /// What the script of a Windows wrapper does, run by the Node of this machine (the one a
    /// window would run it on), with `real` as the program the real Claude's window opens on and
    /// the Node as the stand-in: the words a window was given, `given`, and what comes of them.
    fn relayed(real: &[&str], given: &[&str]) -> crate::process::Ran {
        let node = path::command("node").expect("a test of the wrapper's script needs Node");
        let folder = tempfile::tempdir().unwrap();
        let mut argv = vec![node.to_string_lossy().into_owned()];
        argv.extend(real.iter().map(|word| (*word).to_owned()));
        let program =
            write_windows_wrapper(folder.path(), "w", "Why.", &node, &argv, Some(&node)).unwrap();
        crate::process::Run::new(&node)
            .arg(program.with_extension("mjs"))
            .args(given)
            .run()
            .unwrap()
    }

    #[test]
    fn the_script_starts_the_real_program_with_its_flags_first_and_every_word_the_window_gave_it() {
        let say = "console.log(JSON.stringify(process.argv.slice(1)))";
        let ran = relayed(
            &["-e", say, "--", "--strict-mcp-config", "--no-chrome"],
            &["--model", "claude-sonnet-5-5", "a b", "it's"],
        );
        assert_eq!(
            ran.stdout.trim(),
            r#"["--strict-mcp-config","--no-chrome","--model","claude-sonnet-5-5","a b","it's"]"#
        );
        assert_eq!(ran.code, Some(0));
    }

    #[test]
    fn the_script_starts_the_stand_in_for_a_window_of_a_fake_model_and_ends_as_the_program_does() {
        let real = ["-e", "console.log('real')"];
        // The Node is the stand-in here: it is given the window's words, and runs them.
        let ran = relayed(
            &real,
            &[
                "-e",
                "console.log('stand-in', process.argv.slice(1))",
                "fake",
            ],
        );
        assert_eq!(ran.stdout.trim(), "stand-in [ 'fake' ]");
        let ran = relayed(&real, &["-e", "process.exit(0)", "fake-chief"]);
        assert_eq!(ran.code, Some(0));
        // A word that only holds the name of a fake model is not the model.
        assert_eq!(relayed(&real, &["fakes"]).stdout.trim(), "real");
        // The program's exit code is the wrapper's.
        let ran = relayed(&["-e", "process.exit(7)"], &[]);
        assert_eq!(ran.code, Some(7));
    }
}
