//! How to start a program here. A `.cmd` or `.bat` (an npm-installed CLI on
//! Windows is one) is a script for cmd.exe, not a program: an npm-style shim
//! is read for what it runs, and that runs directly, so an argument may hold
//! anything, a newline included; any other script goes through cmd.exe, each
//! argument quoted and escaped the way cmd.exe reads its line and the script
//! then reads it again (the shape npm itself uses, through cross-spawn),
//! which cannot carry a newline. Anything else runs as it is.
//!
//! An npm shim runs on a Node: the one beside it, else the one on the PATH
//! it is started with, as npm's own shim finds it. ConsensFlow bundles none to
//! fall back on, so a shim for which none is to be found is refused, in words
//! that say what to do ([`runnable`], [`pane_argv`]): that is a limit, not a
//! failure to retry.

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

/// Whether `executable` is a script for cmd.exe: a `.cmd` or a `.bat`.
fn is_script(executable: &Path) -> bool {
    let name = executable.to_string_lossy();
    name.len() > 4
        && [".cmd", ".bat"].iter().any(|extension| {
            name.get(name.len() - 4..)
                .is_some_and(|end| end.eq_ignore_ascii_case(extension))
        })
}

/// How to start `executable` with `args` here, or why it cannot be started:
/// an npm shim that runs on a Node, when none is to be found for it.
pub fn runnable(executable: &Path, args: &[OsString], env: &Env) -> Result<Run, String> {
    let name = executable.to_string_lossy();
    if !is_script(executable) {
        return Ok(Run {
            program: executable.to_path_buf(),
            args: args.to_vec(),
            verbatim: false,
        });
    }
    match read_shim(executable, env) {
        Shim::Runs { program, script } => {
            let mut all = vec![script.into_os_string()];
            all.extend(args.iter().cloned());
            return Ok(Run {
                program,
                args: all,
                verbatim: false,
            });
        }
        Shim::NeedsNode => return Err(needs_node(executable)),
        Shim::Opaque => {}
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
    Ok(Run {
        program: shell,
        args: args.collect(),
        verbatim: true,
    })
}

/// A window's program as the pane host starts it. The host starts a file with
/// each argument quoted the way programs read them, which cmd.exe does not, so
/// an npm-installed harness on Windows (a `.cmd` shim) opens as the shim's own
/// node and script; a script of any other shape cannot open a window, and a
/// shim for which no Node is to be found is refused as [`runnable`] refuses it.
pub fn pane_argv(argv: &[String], env: &Env) -> Result<Vec<String>, String> {
    let Some((executable, args)) = argv.split_first() else {
        return Ok(Vec::new());
    };
    if !is_script(Path::new(executable)) {
        return Ok(argv.to_vec());
    }
    let (program, script) = match read_shim(Path::new(executable), env) {
        Shim::Runs { program, script } => (program, script),
        Shim::NeedsNode => return Err(needs_node(Path::new(executable))),
        Shim::Opaque => {
            return Err(format!(
                "{executable} is not an npm shim, and only cmd.exe could run it in a window"
            ));
        }
    };
    let mut opened = vec![
        program.to_string_lossy().into_owned(),
        script.to_string_lossy().into_owned(),
    ];
    opened.extend(args.iter().cloned());
    Ok(opened)
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

/// What a `.cmd` or a `.bat` is, read for how to start it.
enum Shim {
    /// An npm-style shim, read: `program` runs `script`.
    Runs { program: PathBuf, script: PathBuf },
    /// An npm-style shim that runs on a Node, and none is to be found for it.
    NeedsNode,
    /// Not of the shape [`shim_target`] reads: only cmd.exe can run it.
    Opaque,
}

/// The program an npm-style shim runs its script with.
enum Program {
    /// `%_prog%` or `%NODE_EXE%`, which npm's own shim sets to the node beside
    /// it, else to the one on the PATH.
    Node,
    /// A program the shim names outright.
    Named(PathBuf),
}

/// `shim` read for what it runs, and with which Node when it names none.
fn read_shim(shim: &Path, env: &Env) -> Shim {
    match shim_target(shim) {
        None => Shim::Opaque,
        Some((Program::Named(program), script)) => Shim::Runs { program, script },
        Some((Program::Node, script)) => match node_for(shim, env) {
            Some(program) => Shim::Runs { program, script },
            None => Shim::NeedsNode,
        },
    }
}

/// The Node an npm shim runs on, as npm's own shim finds it: the `node.exe`
/// beside it, else the node on `env`'s PATH. There is no other: ConsensFlow
/// bundles none, and an environment variable names none.
fn node_for(shim: &Path, env: &Env) -> Option<PathBuf> {
    let beside = shim.parent()?.join("node.exe");
    if beside.exists() {
        return Some(beside);
    }
    on_path("node", env)
}

/// What is said of an npm shim that runs on a Node when none is to be found,
/// for whoever has to do something about it.
fn needs_node(shim: &Path) -> String {
    format!(
        "{} is an npm shim that runs on Node, and ConsensFlow finds no Node for it: none is \
         beside it, and none is on the PATH ConsensFlow runs with. Make the harness's Node \
         visible to ConsensFlow, or install the harness's own build instead of the npm one.",
        shim.display()
    )
}

/// What an npm-style shim runs, read from the last line that passes its
/// arguments on (`%*`): `"<program>" "<script>" %*`, where the program is
/// `%_prog%` or `%NODE_EXE%` (a Node, found by [`node_for`]) or named outright,
/// and the script may begin with `%dp0%` or `%~dp0`, the shim's own folder.
/// None when the shim is not of that shape, or names a script that is not
/// there.
fn shim_target(shim: &Path) -> Option<(Program, PathBuf)> {
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
        Program::Node
    } else {
        let program = expand(program);
        if program.contains('%') {
            return None;
        }
        Program::Named(PathBuf::from(program))
    };
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
            Ok(Run {
                program: "/usr/local/bin/codex".into(),
                args: args(&["app-server", "a b"]),
                verbatim: false
            })
        );
        assert_eq!(
            runnable(Path::new(r"C:\x\claude.exe"), &[], &Env::default())
                .unwrap()
                .program,
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
        )
        .unwrap();
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
        let run = runnable(Path::new(r"C:\x\tool.BAT"), &[], &Env::default()).unwrap();
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
                Ok(Run {
                    program: elsewhere.join("node.exe"),
                    args: vec![script.into_os_string(), "queue".into(), "a\nb".into()],
                    verbatim: false,
                })
            );
        }

        #[test]
        fn an_npm_shim_takes_the_node_beside_it_first_as_npm_itself_would() {
            let root = tempfile::tempdir().unwrap();
            let (bin, shim, _) = npm(root.path());
            fs::write(bin.join("node.exe"), "").unwrap();
            // Another node is on the PATH, and the one beside the shim is still the one.
            let elsewhere = root.path().join("elsewhere");
            fs::create_dir_all(&elsewhere).unwrap();
            fs::write(elsewhere.join("node.exe"), "").unwrap();
            for path in ["", elsewhere.to_str().unwrap()] {
                let env =
                    Env::from_vars([("OS", "Windows_NT"), ("PATH", path), ("PATHEXT", ".EXE")]);
                assert_eq!(
                    runnable(&shim, &[], &env).unwrap().program,
                    bin.join("node.exe"),
                    "PATH {path:?}"
                );
            }
        }

        /// What a shim that runs on Node is refused with, whichever way it is
        /// asked to start: the shim, and both remedies.
        fn refusal(shim: &Path) -> String {
            let said = shim.display();
            format!(
                "{said} is an npm shim that runs on Node, and ConsensFlow finds no Node for it: none is \
                 beside it, and none is on the PATH ConsensFlow runs with. Make the harness's Node \
                 visible to ConsensFlow, or install the harness's own build instead of the npm one."
            )
        }

        #[test]
        fn an_npm_shim_with_no_node_beside_it_or_on_the_path_is_refused_saying_what_to_do() {
            let root = tempfile::tempdir().unwrap();
            let (_, shim, _) = npm(root.path());
            // A PATH with other programs on it, none of them a node; and a node
            // that an environment variable names, which is nobody's to run.
            let other = root.path().join("other");
            fs::create_dir_all(&other).unwrap();
            fs::write(other.join("git.exe"), "").unwrap();
            let named = root.path().join("named").join("node.exe");
            fs::create_dir_all(named.parent().unwrap()).unwrap();
            fs::write(&named, "").unwrap();
            startable(&named);
            for vars in [vec![], vec![("CONSENSFLOW_NODE", named.to_str().unwrap())]] {
                let mut all = vec![
                    ("OS", "Windows_NT"),
                    ("PATH", other.to_str().unwrap()),
                    ("PATHEXT", ".EXE"),
                ];
                all.extend(vars.clone());
                let env = Env::from_vars(all);
                assert_eq!(
                    runnable(&shim, &args(&["--version"]), &env),
                    Err(refusal(&shim)),
                    "{vars:?}"
                );
                let argv = [shim.to_string_lossy().into_owned()];
                assert_eq!(pane_argv(&argv, &env), Err(refusal(&shim)), "{vars:?}");
            }
        }

        #[test]
        fn the_refusal_is_for_a_shim_that_runs_on_node_and_for_no_other() {
            let root = tempfile::tempdir().unwrap();
            let (_, _, script) = npm(root.path());
            let env = Env::from_vars([("OS", "Windows_NT"), ("PATH", "")]);
            // A shim that names its program outright needs no Node to be found.
            let own = root.path().join("own.cmd");
            fs::write(
                &own,
                format!("@echo off\r\n\"/opt/node\" \"{}\" %*\r\n", script.display()),
            )
            .unwrap();
            assert!(runnable(&own, &[], &env).is_ok());
            // One that cannot be read goes through cmd.exe, which is not asked for a Node.
            let opaque = root.path().join("opaque.cmd");
            fs::write(&opaque, "@echo off\r\nrun.exe %*\r\n").unwrap();
            assert!(runnable(&opaque, &[], &env).unwrap().verbatim);
            // And a program that is no script is never a shim.
            assert!(runnable(Path::new("/usr/bin/pi"), &[], &env).is_ok());
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
                Ok(Run {
                    program: "/opt/node".into(),
                    args: vec![script.into_os_string(), "x".into()],
                    verbatim: false
                })
            );
        }

        #[test]
        fn and_a_window_opens_on_the_shim_as_its_node_and_script_or_not_at_all() {
            let root = tempfile::tempdir().unwrap();
            let (bin, shim, script) = npm(root.path());
            fs::write(bin.join("node.exe"), "").unwrap();
            let env = Env::from_vars([("OS", "Windows_NT"), ("PATH", "")]);
            let text = |path: &Path| path.to_string_lossy().into_owned();
            let words = |words: &[&str]| {
                words
                    .iter()
                    .map(|&word| word.to_owned())
                    .collect::<Vec<_>>()
            };
            let mut argv = vec![text(&shim)];
            argv.extend(words(&["--model", "a b", "a\nb"]));
            let mut opened = vec![text(&bin.join("node.exe")), text(&script)];
            opened.extend(words(&["--model", "a b", "a\nb"]));
            assert_eq!(pane_argv(&argv, &env).unwrap(), opened);
            let opaque = root.path().join("opaque.cmd");
            fs::write(&opaque, "@echo off\r\nrun.exe %*\r\n").unwrap();
            let named = text(&opaque);
            assert_eq!(
                pane_argv(std::slice::from_ref(&named), &env).unwrap_err(),
                format!("{named} is not an npm shim, and only cmd.exe could run it in a window")
            );
            let program = words(&["/usr/local/bin/pi", "x"]);
            assert_eq!(pane_argv(&program, &env).unwrap(), program);
            assert_eq!(pane_argv(&[], &env).unwrap(), Vec::<String>::new());
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
            assert!(runnable(&opaque, &[], &Env::default()).unwrap().verbatim);
        }
    }
}
