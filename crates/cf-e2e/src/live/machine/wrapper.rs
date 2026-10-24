//! The programs a live run puts where the real Claude Code would be started, so
//! that every Claude window of the run starts through one of them: the wrapper
//! that shuts the human's MCP servers, connectors and browser out of it, and the
//! shim that does that and also has the stand-in agent play the windows whose
//! agent's model is `fake` or `fake-chief`. Both are shell scripts, for macOS
//! and Linux: Windows opens a window only on the shape of an npm shim.

use std::io;
use std::path::{Path, PathBuf};

use crate::{files, Error, Result};

/// The flags that keep a scripted Claude window from reaching the human's MCP
/// servers, connectors and browser, which it would in full-permission mode on
/// their own setup. The product leaves all of them on for every agent, and
/// test runs are the only place they are shut out.
pub const ISOLATION: [&str; 2] = ["--strict-mcp-config", "--no-chrome"];

/// A program called `claude-isolated` that runs the real Claude Code at `claude`
/// with [`ISOLATION`] before whatever words it is given, made in `folder`: the
/// stand-in a rig puts first on the `PATH` of its windows is then this one,
/// and every Claude window of the run starts through it. Where it is.
pub fn isolated_claude(folder: &Path, claude: &Path) -> Result<PathBuf> {
    written(folder, "claude-isolated", claude, isolating_script(claude))
}

/// A program called `claude-shimmed` that runs the stand-in agent at `stand_in`,
/// with the Claude config folder of the machine (`$HOME/.claude`, where the
/// daemon looks for the records of a window), for the window whose agent's model
/// is `fake` or `fake-chief`, and the real Claude Code at `claude` as
/// [`isolated_claude`] does for every other: a project whose chief and some
/// members are stand-ins and whose other members are real. Made in `folder`.
/// Where it is.
pub fn shimmed_claude(folder: &Path, claude: &Path, stand_in: &Path) -> Result<PathBuf> {
    written(
        folder,
        "claude-shimmed",
        claude,
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
    written(
        folder,
        "claude-shimmed",
        claude,
        shimming_script(claude, stand_in, Some(servers)),
    )
}

/// The program `name` in `folder`, with `script` for its words; a platform that
/// cannot run a script as a window's program is an error that says so.
fn written(folder: &Path, name: &str, claude: &Path, script: String) -> Result<PathBuf> {
    if cfg!(windows) {
        return Err(Error::Program {
            action: "isolate",
            program: claude.to_string_lossy().into_owned(),
            source: io::Error::new(
                io::ErrorKind::Unsupported,
                "a live run that starts Claude Code through a wrapper is for macOS and Linux",
            ),
        });
    }
    let program = folder.join(name);
    files::write_executable(&program, script)?;
    Ok(program)
}

/// The shell script that is [`isolated_claude`].
fn isolating_script(claude: &Path) -> String {
    format!(
        "#!/bin/sh\n# Written by a live test: its Claude windows reach no MCP server and no browser.\nexec {} {} \"$@\"\n",
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
         # stand-in's, which writes its records where the daemon reads Claude's (the run's own\n\
         # config where it was given one); every other\n\
         # Claude window reaches no MCP server and no browser.\n\
         for arg in \"$@\"; do\n\
         \x20 case \"$arg\" in\n\
         \x20   fake | fake-chief) CLAUDE_CONFIG_DIR=\"${{CLAUDE_CONFIG_DIR:-$HOME/.claude}}\" exec {} \"$@\" ;;\n\
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
            script.contains(
                "fake | fake-chief) CLAUDE_CONFIG_DIR=\"${CLAUDE_CONFIG_DIR:-$HOME/.claude}\" exec '/the/fake-agent' \"$@\" ;;"
            ),
            "{script}"
        );
        assert!(
            !shimming_script(Path::new("/c"), Path::new("/s"), None).contains("--mcp-config"),
            "a shim with no servers names none"
        );
    }

    #[test]
    fn the_shim_hands_the_stand_in_the_machines_claude_folder_and_the_real_claude_the_flags() {
        let script = shimming_script(
            Path::new("/real/claude"),
            Path::new("/the/fake-agent"),
            None,
        );
        assert!(
            script.contains(
                "fake | fake-chief) CLAUDE_CONFIG_DIR=\"${CLAUDE_CONFIG_DIR:-$HOME/.claude}\" exec '/the/fake-agent' \"$@\" ;;"
            ),
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
    /// says to the words `words`, with `HOME` the folder `/h`: the stand-in is a
    /// shell that prints the Claude folder it was given.
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
            .run()
            .unwrap()
            .stdout
    }

    #[cfg(unix)]
    #[test]
    fn a_window_of_a_fake_model_is_the_stand_ins_with_the_machines_claude_folder() {
        for model in ["fake", "fake-chief"] {
            assert_eq!(
                shimmed(&["-c", "echo $CLAUDE_CONFIG_DIR", model]),
                "/h/.claude\n",
                "{model}"
            );
        }
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
            "/h/.claude\n"
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

    #[cfg(windows)]
    #[test]
    fn on_windows_a_wrapper_is_refused_and_says_why() {
        let folder = tempfile::tempdir().unwrap();
        let failed = isolated_claude(folder.path(), Path::new("claude.cmd")).unwrap_err();
        assert!(failed.to_string().contains("macOS and Linux"), "{failed}");
        let failed = shimmed_claude(
            folder.path(),
            Path::new("claude.cmd"),
            Path::new("fake-agent.exe"),
        )
        .unwrap_err();
        assert!(failed.to_string().contains("macOS and Linux"), "{failed}");
    }
}
