//! What a TUI was given to start with, as Codex's takes it: the options the
//! broker adds (`--remote <url> --remote-auth-token-env <name>`), then Codex's
//! own: a thread to resume (`resume <thread>`) and a message to start with.

/// What a TUI was given to start with, after the options the broker adds.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Launched<'a> {
    /// The thread it is to resume (`resume <thread>`).
    pub resume: Option<&'a str>,
    /// The message it starts with: the word that is no flag and no value of one.
    pub message: Option<&'a str>,
}

/// What the TUI's arguments `args` say of the thread and the message it starts with.
pub fn launched(args: &[String]) -> Launched<'_> {
    let mut launched = Launched::default();
    let mut words = args.get(4..).unwrap_or_default().iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-c" | "--config" | "--enable" | "--disable" | "--model" | "-m" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            "resume" if launched.resume.is_none() => {
                launched.resume = words.next().map(String::as_str);
            }
            message => {
                launched.message = Some(message);
                break;
            }
        }
    }
    launched
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arguments of a TUI the broker added its own options to.
    fn tui_args(rest: &str) -> Vec<String> {
        format!("--remote ws://127.0.0.1:1 --remote-auth-token-env TOKEN {rest}")
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_tui_starts_with_the_message_it_is_given_and_with_none_where_it_is_given_none() {
        let args = tui_args("-c a=b --model fake --enable x HELLO");
        assert_eq!(
            launched(&args),
            Launched {
                resume: None,
                message: Some("HELLO")
            }
        );
        assert_eq!(
            launched(&tui_args("-c a=b --model fake")),
            Launched::default()
        );
        assert_eq!(launched(&[]), Launched::default());
    }

    #[test]
    fn a_tui_told_to_resume_names_the_thread_and_may_have_a_message_after_it() {
        let args = tui_args("-c a=b resume 01a0 --model fake");
        assert_eq!(
            launched(&args),
            Launched {
                resume: Some("01a0"),
                message: None
            }
        );
        let args = tui_args("resume 01a0 --model fake Go on");
        assert_eq!(
            launched(&args),
            Launched {
                resume: Some("01a0"),
                message: Some("Go")
            }
        );
        // A resume that names no thread resumes none.
        assert_eq!(launched(&tui_args("resume")).resume, None);
    }
}
