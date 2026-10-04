//! How to start a program here. A `.cmd` or `.bat` (an npm-installed CLI on
//! Windows is one) is a script for cmd.exe, not a program: an npm-style shim
//! is read for what it runs, and that runs directly, so an argument may hold
//! anything, a newline included; any other script goes through cmd.exe, each
//! argument quoted and escaped the way cmd.exe reads its line and the script
//! then reads it again (the shape npm itself uses, through cross-spawn),
//! which cannot carry a newline. Anything else runs as it is.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};
use std::process::Command;

use cf_base::env::Env;

use crate::search::on_path;

/// cmd.exe's own special characters; each is escaped with a caret.
const CMD_META: [char; 18] = [
    '(', ')', '[', ']', '%', '!', '^', '"', '`', '<', '>', '&', '|', ';', ',', ' ', '*', '?',
];

/// A program and its arguments, as they start here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Whether the arguments go into the command line as they are, not quoted
    /// again: cmd.exe's line is quoted already. Only Windows has a line.
    pub verbatim: bool,
}

impl Run {
    /// The command that starts it.
    #[allow(clippy::disallowed_methods)] // The one place a process starts.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        #[cfg(windows)]
        if self.verbatim {
            use std::os::windows::process::CommandExt;
            for arg in &self.args {
                command.raw_arg(arg);
            }
            return command;
        }
        command.args(&self.args);
        command
    }
}

/// How to start `executable` with `args` here.
pub fn runnable(executable: &Path, args: &[OsString], env: &Env) -> Run {
    let name = executable.to_string_lossy();
    let script = name.len() > 4
        && [".cmd", ".bat"].iter().any(|extension| {
            name.get(name.len() - 4..)
                .is_some_and(|end| end.eq_ignore_ascii_case(extension))
        });
    if !script {
        return Run {
            program: executable.to_path_buf(),
            args: args.to_vec(),
            verbatim: false,
        };
    }
    if let Some((program, script)) = shim_target(executable, env) {
        let mut all = vec![script.into_os_string()];
        all.extend(args.iter().cloned());
        return Run {
            program,
            args: all,
            verbatim: false,
        };
    }
    let line = std::iter::once(caret(&name))
        .chain(args.iter().map(|arg| quoted(&arg.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ");
    // Named absolutely: a launch environment may carry a PATH of its own
    // that has no System32 on it, and cmd.exe must still be found.
    let shell = env.path("ComSpec").map_or_else(
        || PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        Path::to_path_buf,
    );
    let args = ["/d", "/s", "/c"]
        .into_iter()
        .map(OsString::from)
        .chain([format!("\"{line}\"").into()]);
    Run {
        program: shell,
        args: args.collect(),
        verbatim: true,
    }
}

/// `text` with each of cmd.exe's special characters escaped with a caret.
fn caret(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() * 2);
    for character in text.chars() {
        if CMD_META.contains(&character) {
            escaped.push('^');
        }
        escaped.push(character);
    }
    escaped
}

/// One argument on cmd.exe's line: quoted for the program that reads it (an
/// inner quote backslashed, the backslashes before it and at the end doubled),
/// then escaped twice, once for cmd.exe's reading of its line and once for the
/// script's own.
fn quoted(arg: &str) -> String {
    let mut inner = String::with_capacity(arg.len() + 2);
    let mut backslashes = 0;
    for character in arg.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                inner.push_str(&"\\".repeat(backslashes * 2 + 1));
                inner.push('"');
                backslashes = 0;
            }
            other => {
                inner.push_str(&"\\".repeat(backslashes));
                inner.push(other);
                backslashes = 0;
            }
        }
    }
    inner.push_str(&"\\".repeat(backslashes * 2));
    caret(&caret(&format!("\"{inner}\"")))
}

/// What an npm-style shim runs, read from the last line that passes its
/// arguments on (`%*`): `"<program>" "<script>" %*`, where the program is
/// `%_prog%` or `%NODE_EXE%` (the node beside the shim, else the node on
/// PATH, else the app's) and the script may begin with `%dp0%` or `%~dp0`,
/// the shim's own folder. None when the shim is not of that shape, or names
/// a script that is not there.
fn shim_target(shim: &Path, env: &Env) -> Option<(PathBuf, PathBuf)> {
    let text = String::from_utf8_lossy(&fs::read(shim).ok()?).into_owned();
    let line = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .rev()
        .find(|line| line.contains("%*"))?;
    let parts = quoted_parts(line);
    let [.., program, script] = parts.as_slice() else {
        return None;
    };
    let folder = shim.parent()?.to_string_lossy().into_owned();
    let expand = |value: &str| -> String {
        let head = value.get(..5).filter(|head| {
            head.eq_ignore_ascii_case("%~dp0") || head.eq_ignore_ascii_case("%dp0%")
        });
        let value = match head {
            Some(_) => {
                let rest = &value[5..];
                format!(
                    "{folder}{MAIN_SEPARATOR}{}",
                    rest.strip_prefix('\\').unwrap_or(rest)
                )
            }
            None => value.to_string(),
        };
        value.replace('\\', MAIN_SEPARATOR_STR)
    };
    let script = expand(script);
    if script.contains('%') || !Path::new(&script).exists() {
        return None;
    }
    let names_node = ["%_prog%", "%NODE_EXE%"]
        .iter()
        .any(|name| program.eq_ignore_ascii_case(name));
    let program = if names_node {
        let beside = Path::new(&folder).join("node.exe");
        if beside.exists() {
            beside
        } else {
            on_path("node", env).or_else(|| env.path("CONSENSFLOW_NODE").map(Path::to_path_buf))?
        }
    } else {
        PathBuf::from(expand(program))
    };
    if program.to_string_lossy().contains('%') {
        return None;
    }
    Some((program, PathBuf::from(script)))
}

/// Each `"…"` of a line that holds something, found as JavaScript's
/// `/"([^"]+)"/g` finds them: an empty `""` matches nothing, and its second
/// quote may open the next.
fn quoted_parts(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut at = 0;
    while let Some(open) = line[at..].find('"').map(|found| at + found) {
        match line[open + 1..].find('"') {
            Some(0) => at = open + 1,
            Some(length) => {
                parts.push(&line[open + 1..open + 1 + length]);
                at = open + 2 + length;
            }
            None => break,
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn runs_a_program_directly_with_its_arguments_untouched() {
        let run = runnable(
            Path::new("/usr/local/bin/codex"),
            &args(&["app-server", "a b"]),
            &Env::default(),
        );
        assert_eq!(
            run,
            Run {
                program: "/usr/local/bin/codex".into(),
                args: args(&["app-server", "a b"]),
                verbatim: false
            }
        );
        assert_eq!(
            runnable(Path::new(r"C:\x\claude.exe"), &[], &Env::default()).program,
            Path::new(r"C:\x\claude.exe")
        );
    }

    #[test]
    fn runs_a_cmd_through_cmd_exe_each_argument_quoted_the_way_cmd_exe_reads_it() {
        let env = Env::from_vars([("ComSpec", r"C:\Windows\System32\cmd.exe")]);
        let run = runnable(
            Path::new(r"C:\Program Files\nodejs\codex.cmd"),
            &args(&["-c", r#"developer_instructions="hi" & more"#, "trailing\\"]),
            &env,
        );
        assert_eq!(run.program, Path::new(r"C:\Windows\System32\cmd.exe"));
        assert!(run.verbatim);
        assert_eq!(&run.args[..3], &args(&["/d", "/s", "/c"])[..]);
        // The exact line, as cross-spawn (npm's own runner) would write it: every
        // special character carries one caret for cmd.exe's reading of the line
        // and two more for the script's own reading; an inner quote is backslashed
        // for the program and a trailing backslash doubled.
        assert_eq!(
            run.args[3],
            OsString::from(
                r#""C:\Program^ Files\nodejs\codex.cmd ^^^"-c^^^" ^^^"developer_instructions=\^^^"hi\^^^"^^^ ^^^&^^^ more^^^" ^^^"trailing\\^^^"""#
            )
        );
    }

    #[test]
    fn treats_a_bat_as_a_script_too() {
        let run = runnable(Path::new(r"C:\x\tool.BAT"), &[], &Env::default());
        assert!(run
            .program
            .to_string_lossy()
            .to_lowercase()
            .ends_with(r"\cmd.exe"));
    }

    #[test]
    fn reads_a_shims_quoted_parts_as_the_javascript_regex_did() {
        assert_eq!(
            quoted_parts(r#""%_prog%"  "%dp0%\x.js" %*"#),
            [r"%_prog%", r"%dp0%\x.js"]
        );
        assert_eq!(quoted_parts(r#""" "x" %*"#), [" "]);
        assert_eq!(quoted_parts(r#"no quotes %*"#), Vec::<&str>::new());
    }

    mod shims {
        use super::*;

        /// The last line of the shim npm writes for a global package, with npm's variables.
        const NPM_SHIM: &str = "@ECHO off\r\nSETLOCAL\r\nCALL :find_dp0\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@x\\cli\\bin\\cli.js\" %*\r\n";

        /// `file` made startable, where that takes a mode.
        fn startable(file: &Path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
            }
            #[cfg(not(unix))]
            let _ = file;
        }

        /// A folder of shims: npm's for a package whose script is there.
        fn npm(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
            let bin = root.join("bin");
            // Built of its parts: the shim's path is expanded with the platform's separators.
            let script = ["node_modules", "@x", "cli", "bin", "cli.js"]
                .iter()
                .fold(bin.clone(), |path, part| path.join(part));
            fs::create_dir_all(script.parent().unwrap()).unwrap();
            fs::write(&script, "").unwrap();
            let shim = bin.join("cli.cmd");
            fs::write(&shim, NPM_SHIM).unwrap();
            (bin, shim, script)
        }

        #[test]
        fn an_npm_shim_runs_its_script_with_the_node_on_path_when_none_sits_beside_it() {
            let root = tempfile::tempdir().unwrap();
            let (_, shim, script) = npm(root.path());
            let elsewhere = root.path().join("elsewhere");
            fs::create_dir_all(&elsewhere).unwrap();
            fs::write(elsewhere.join("node.exe"), "").unwrap();
            startable(&elsewhere.join("node.exe"));
            let env = Env::from_vars([
                ("OS", "Windows_NT"),
                ("PATH", elsewhere.to_str().unwrap()),
                ("PATHEXT", ".EXE"),
            ]);
            assert_eq!(
                runnable(&shim, &args(&["queue", "a\nb"]), &env),
                Run {
                    program: elsewhere.join("node.exe"),
                    args: vec![script.into_os_string(), "queue".into(), "a\nb".into()],
                    verbatim: false,
                }
            );
        }

        #[test]
        fn an_npm_shim_takes_the_node_beside_it_first_as_npm_itself_would() {
            let root = tempfile::tempdir().unwrap();
            let (bin, shim, _) = npm(root.path());
            fs::write(bin.join("node.exe"), "").unwrap();
            let env = Env::from_vars([("OS", "Windows_NT"), ("PATH", "")]);
            assert_eq!(runnable(&shim, &[], &env).program, bin.join("node.exe"));
        }

        #[test]
        fn a_shim_naming_its_program_and_script_outright_runs_them() {
            let root = tempfile::tempdir().unwrap();
            let (_, _, script) = npm(root.path());
            let own = root.path().join("own.cmd");
            fs::write(
                &own,
                format!("@echo off\r\n\"/opt/node\" \"{}\" %*\r\n", script.display()),
            )
            .unwrap();
            assert_eq!(
                runnable(&own, &args(&["x"]), &Env::default()),
                Run {
                    program: "/opt/node".into(),
                    args: vec![script.into_os_string(), "x".into()],
                    verbatim: false
                }
            );
        }

        #[test]
        fn a_shim_it_cannot_read_goes_through_cmd_exe() {
            let root = tempfile::tempdir().unwrap();
            let opaque = root.path().join("opaque.cmd");
            fs::write(
                &opaque,
                "@echo off\r\n\"%NODE_EXE%\" \"%NPM_CLI_JS%\" %*\r\n",
            )
            .unwrap();
            assert!(runnable(&opaque, &[], &Env::default()).verbatim);
        }
    }
}
