//! What `cf` says it does inside a window: the board's commands, as an
//! agent reads them in `cf help` and when it got a command wrong.

/// The board's commands, as `cf help` prints them (usage.txt, which the
/// test that checks every `cf …` the repo names reads too).
pub fn usage() -> &'static str {
    let text = include_str!("usage.txt");
    text.strip_suffix('\n').unwrap_or(text)
}

/// What `cf task add` takes, said when it was given too little.
pub const ADD_USAGE: &str = r#"cf task add --tier <critical|complex|standard|light> "what to do" (with --advice for an advisor or --review for a reviewer; or --design, --after T-3, or --self; --needs T-3,T-4 and --before T-9,T-10 order the board)"#;

/// The task commands alone: what `cf task --help` prints.
pub fn task_usage() -> String {
    usage()
        .lines()
        .filter(|line| {
            let indented = line.starts_with(char::is_whitespace);
            (indented && line.trim_start().starts_with("cf task "))
                || line.starts_with(&" ".repeat(36))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
