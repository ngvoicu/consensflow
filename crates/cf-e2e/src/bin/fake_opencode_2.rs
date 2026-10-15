//! A stand-in for the `opencode` of a machine whose OpenCode has updated itself
//! to 2, in the integration tests. It says what OpenCode 2.0.29 said to
//! `--version` (`opencode v2.0.29`), and writes down everything it is asked, a
//! list of arguments to a line, in `CF_TEST_OPENCODE_LOG`.
//!
//! Anything else is what OpenCode 2 made of the launch of OpenCode 1: the flags a
//! window is launched with (`serve --port`, `--hostname`, `--model`) are gone, and
//! what the owner's windows did with them was to wait. It takes the arguments and
//! never answers, never listens on the port it is given and never loads a plugin,
//! so a conversation made on its `serve` is never made, and a window opened on it
//! never says which it shows.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::process::own_var;

/// What OpenCode 2.0.29 says to `--version`.
const VERSION: &str = "opencode v2.0.29\n";

/// Whether `arg` names the stand-in's own file (`own`), by its file name: the
/// script of the shim that opened it, where the system wraps a program in one.
fn is_own_file(arg: &str, own: Option<&Path>) -> bool {
    own.and_then(Path::file_name).is_some_and(|mine| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(mine))
    })
}

/// What the stand-in was asked, without its own file where a shim put it first:
/// on Windows a program is started as `<program> <script> <arguments>`, and the
/// shim names the stand-in as both.
fn asked<'a>(args: &'a [String], own: Option<&Path>) -> &'a [String] {
    match args.split_first() {
        Some((first, rest)) if is_own_file(first, own) => rest,
        _ => args,
    }
}

/// Whether the question is the version, which the daemon asks with that word alone.
fn is_version(args: &[String]) -> bool {
    matches!(args, [only] if only == "--version")
}

/// Writes down what the stand-in was asked: the arguments, as a JSON list.
fn write_down(log: &str, args: &[String]) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{}", serde_json::json!(args))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let own = std::env::current_exe().ok();
    let args = asked(&args, own.as_deref());
    if let Some(log) = own_var("CF_TEST_OPENCODE_LOG") {
        if let Err(failed) = write_down(&log, args) {
            let _ = writeln!(io::stderr(), "fake-opencode-2: {log}: {failed}");
            return ExitCode::FAILURE;
        }
    }
    if is_version(args) {
        let said = io::stdout().lock().write_all(VERSION.as_bytes());
        return if said.is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    loop {
        sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn the_daemons_question_of_the_version_is_the_word_alone() {
        assert!(is_version(&words("--version")));
        assert!(!is_version(&words("")));
        assert!(!is_version(&words("--version --port 41001")));
        assert!(!is_version(&words(
            "serve --port 41001 --hostname 127.0.0.1"
        )));
    }

    #[test]
    fn the_stand_in_a_shim_names_as_its_script_is_no_argument() {
        let own = Some(Path::new("/bin/fake-opencode-2"));
        let args = words("/x/y/fake-opencode-2 --version");
        assert_eq!(asked(&args, own), ["--version"]);
        // However the system writes the name of the file.
        let args = words("C:/x/FAKE-OPENCODE-2 --version");
        assert_eq!(asked(&args, own), ["--version"]);
        // A stand-in that does not know its own file takes nothing for it.
        let args = words("/x/y/fake-opencode-2 --version");
        assert_eq!(asked(&args, None), args.as_slice());
        // What is asked of it with no shim is asked as it is.
        let args = words("serve --port 41001");
        assert_eq!(asked(&args, own), args.as_slice());
    }

    #[test]
    fn what_it_is_asked_is_written_down_a_list_to_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir
            .path()
            .join("opencode.jsonl")
            .to_string_lossy()
            .into_owned();
        write_down(&log, &words("--version")).unwrap();
        write_down(&log, &words("serve --port 41001")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "[\"--version\"]\n[\"serve\",\"--port\",\"41001\"]\n"
        );
    }
}
