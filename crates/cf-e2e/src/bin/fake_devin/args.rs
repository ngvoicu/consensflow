//! Devin's launch arguments, as the daemon gives them to a window: the config it
//! runs on (`--config`), the conversation it goes on with (`--resume`), the model
//! and the permission flags (the mode it is asked to be in), and the file its first
//! message is in (`--prompt-file`). And the question the daemon asks it before it opens a window
//! on it: its version.
//!
//! On Windows a window opens only on an npm-style shim, `"<program>" "<script>"
//! %*`, which the pane host reads for the program and the script and starts as
//! `<program> <script> <arguments>`. The stand-in's shim names the stand-in as
//! both, so the first thing it is given there is its own file.

use std::path::Path;

/// The options Devin takes a value for.
const VALUE_FLAGS: [&str; 6] = [
    "--config",
    "--model",
    "--permission-mode",
    "--respect-workspace-trust",
    "--resume",
    "--prompt-file",
];

/// What the stand-in was asked to be.
#[derive(Debug, PartialEq, Eq)]
pub enum Asked {
    /// Its version, which the daemon asks before it launches a window.
    Version,
    /// A window.
    Window(Launch),
}

/// What a window was launched with.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// The conversation it goes on with: none for a new one.
    pub resume: Option<String>,
    /// The file the first message is in.
    pub prompt_file: Option<String>,
    /// The permission mode it was asked to be in (`dangerous` is full permission).
    pub permission_mode: Option<String>,
}

/// Whether `arg` names the stand-in's own file (`own`), by its file name: the
/// script of the shim that opened it.
fn is_own_file(arg: &str, own: Option<&Path>) -> bool {
    own.and_then(Path::file_name).is_some_and(|mine| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(mine))
    })
}

/// Reads the launch arguments `args` (without the program's name) of the
/// stand-in, whose own file is `own`. An option it does not know is none of its
/// business, as Devin's own are many.
pub fn parse(args: &[String], own: Option<&Path>) -> Asked {
    let args = match args.split_first() {
        Some((first, rest)) if is_own_file(first, own) => rest,
        _ => args,
    };
    // The daemon asks for the version with that word alone.
    if matches!(args, [only] if only == "--version") {
        return Asked::Version;
    }
    let mut launch = Launch::default();
    let mut at = 0;
    while at < args.len() {
        let arg = args[at].as_str();
        if VALUE_FLAGS.contains(&arg) {
            let value = args.get(at + 1).cloned();
            match arg {
                "--resume" => launch.resume = value,
                "--prompt-file" => launch.prompt_file = value,
                "--permission-mode" => launch.permission_mode = value,
                _ => {}
            }
            at += 1;
        }
        at += 1;
    }
    Asked::Window(launch)
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

    fn parse(line: &str) -> Asked {
        super::parse(&words(line), Some(Path::new("/bin/fake-devin")))
    }

    #[test]
    fn the_daemons_question_of_the_version_is_answered_before_any_window() {
        assert_eq!(parse("--version"), Asked::Version);
        assert_eq!(parse("/x/y/fake-devin --version"), Asked::Version);
    }

    #[test]
    fn a_new_window_has_its_prompt_file_and_goes_on_with_no_conversation() {
        let launch = parse(
            "--config /c.json --model swe-2-medium --permission-mode dangerous \
             --respect-workspace-trust false --prompt-file /p.txt",
        );
        assert_eq!(
            launch,
            Asked::Window(Launch {
                resume: None,
                prompt_file: Some("/p.txt".to_owned()),
                permission_mode: Some("dangerous".to_owned()),
            })
        );
    }

    #[test]
    fn a_resumed_window_goes_on_with_its_conversation_and_may_have_no_first_message() {
        let launch = parse("--config /c.json --resume calm-river --model fake");
        assert_eq!(
            launch,
            Asked::Window(Launch {
                resume: Some("calm-river".to_owned()),
                ..Launch::default()
            })
        );
    }

    #[test]
    fn what_the_stand_in_does_not_know_is_none_of_its_business_and_a_value_is_never_a_flag() {
        // A value that looks like an option is the option's value.
        let launch = parse("--model --version --prompt-file /p.txt --unknown thing");
        assert_eq!(
            launch,
            Asked::Window(Launch {
                prompt_file: Some("/p.txt".to_owned()),
                ..Launch::default()
            })
        );
    }

    #[test]
    fn the_stand_in_a_shim_names_as_its_script_is_no_option() {
        // The shim names it twice, program and script: its own file comes first.
        let launch = parse("/x/y/fake-devin --resume abc");
        assert_eq!(
            launch,
            Asked::Window(Launch {
                resume: Some("abc".to_owned()),
                ..Launch::default()
            })
        );
        // However the system writes the name of the file.
        assert_eq!(parse("C:/x/FAKE-DEVIN --version"), Asked::Version);
        // A stand-in that does not know its own file takes nothing for it.
        let args = words("/x/y/fake-devin --resume abc");
        assert_eq!(
            super::parse(&args, None),
            Asked::Window(Launch {
                resume: Some("abc".to_owned()),
                ..Launch::default()
            })
        );
    }
}
