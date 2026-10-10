//! Claude's launch arguments, as the daemon gives them to a window: the
//! options that take a value, the session the window runs (a new one, or one it
//! resumes), the settings file whose hooks a question goes through, and the
//! first message the window is launched with.
//!
//! On Windows a window opens only on an npm-style shim, `"<program>" "<script>"
//! %*`, which the pane host reads for the program and the script and starts as
//! `<program> <script> <arguments>`. The stand-in's shim names the stand-in as
//! both, so the first thing it is given there is its own file, which is no
//! message.

use std::path::Path;

/// The options Claude takes a value for, which are no message.
const VALUE_FLAGS: [&str; 7] = [
    "--settings",
    "--add-dir",
    "--append-system-prompt-file",
    "--system-prompt-snapshot",
    "--permission-mode",
    "--model",
    "--effort",
];

/// What a window was launched with.
#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    /// The conversation: the `--session-id` of a new one, the `--resume` of one
    /// that goes on.
    pub session: String,
    pub resuming: bool,
    /// The settings file the window was launched with.
    pub settings: Option<String>,
    /// The first message: the last word that is neither an option nor a value.
    pub seed: Option<String>,
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
/// stand-in, whose own file is `own`.
pub fn parse(args: &[String], own: Option<&Path>) -> Result<Launch, String> {
    let args = match args.split_first() {
        Some((first, rest)) if is_own_file(first, own) => rest,
        _ => args,
    };
    let mut session = None;
    let mut resuming = false;
    let mut settings = None;
    let mut seed = None;
    let mut at = 0;
    while at < args.len() {
        let arg = args[at].as_str();
        let value = args.get(at + 1).cloned();
        if arg == "--settings" {
            settings = value.clone();
        }
        if VALUE_FLAGS.contains(&arg) {
            at += 1;
        } else if arg == "--session-id" || arg == "--resume" {
            session = value;
            resuming = arg == "--resume";
            at += 1;
        } else {
            seed = Some(arg.to_owned());
        }
        at += 1;
    }
    match session {
        Some(session) if !session.is_empty() => Ok(Launch {
            session,
            resuming,
            settings,
            seed,
        }),
        _ => Err("the fake agent needs --session-id or --resume".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    /// The launch `line` gives a stand-in whose file is `/bin/fake-agent`.
    fn parse(line: &[String]) -> Result<Launch, String> {
        super::parse(line, Some(Path::new("/bin/fake-agent")))
    }

    #[test]
    fn a_new_session_a_settings_file_and_the_message_it_was_launched_with_are_read() {
        let launch = parse(&words(
            "--session-id abc --settings /s.json --add-dir /w --append-system-prompt-file /r.md \
             --permission-mode bypassPermissions --model fake --effort high HELLO",
        ))
        .unwrap();
        assert_eq!(
            launch,
            Launch {
                session: "abc".into(),
                resuming: false,
                settings: Some("/s.json".into()),
                seed: Some("HELLO".into()),
            }
        );
    }

    #[test]
    fn a_resumed_session_is_one_that_goes_on_and_a_launch_with_no_message_has_no_seed() {
        let launch = parse(&words("--resume abc --model fake")).unwrap();
        assert_eq!((launch.session.as_str(), launch.resuming), ("abc", true));
        assert_eq!(launch.seed, None);
        assert_eq!(launch.settings, None);
    }

    #[test]
    fn the_last_word_that_is_no_option_is_the_message_and_a_value_is_never_one() {
        let launch = parse(&words("--session-id abc --model m first second")).unwrap();
        assert_eq!(launch.seed.as_deref(), Some("second"));
        // A flag the stand-in does not know is read as a message, as it always was.
        let launch = parse(&words("--session-id abc --dangerously-skip-permissions")).unwrap();
        assert_eq!(
            launch.seed.as_deref(),
            Some("--dangerously-skip-permissions")
        );
    }

    #[test]
    fn the_stand_in_a_shim_names_as_its_script_is_no_message() {
        // The shim names it twice, program and script: its own file comes first.
        let launch = parse(&words("/x/y/fake-agent --session-id abc --model fake")).unwrap();
        assert_eq!(launch.session, "abc");
        assert_eq!(launch.seed, None);
        // However the system writes the name of the file.
        let launch = parse(&words("C:/x/FAKE-AGENT --resume abc")).unwrap();
        assert_eq!((launch.session.as_str(), launch.seed), ("abc", None));
        // Only the first word, and only the stand-in's own file: a message is a message.
        let launch = parse(&words("--session-id abc /x/y/fake-agent")).unwrap();
        assert_eq!(launch.seed.as_deref(), Some("/x/y/fake-agent"));
        let launch = parse(&words("/x/y/other --session-id abc")).unwrap();
        assert_eq!(launch.seed.as_deref(), Some("/x/y/other"));
        // A stand-in that does not know its own file takes the word for a message.
        let launch = super::parse(&words("/x/y/fake-agent --session-id abc"), None).unwrap();
        assert_eq!(launch.seed.as_deref(), Some("/x/y/fake-agent"));
    }

    #[test]
    fn a_launch_with_no_session_is_refused_in_words() {
        for line in ["--model fake", "--session-id", "HELLO", ""] {
            let args: Vec<String> = line
                .split(' ')
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect();
            assert_eq!(
                parse(&args),
                Err("the fake agent needs --session-id or --resume".to_owned()),
                "{line:?}"
            );
        }
    }
}
