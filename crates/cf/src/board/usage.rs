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

/// How far the lines that go on from a command's line are indented.
const CONTINUED: usize = 36;

/// The task commands alone: what `cf task --help` prints.
pub fn task_usage() -> String {
    usage()
        .lines()
        .filter(|line| {
            let indented = line.starts_with(char::is_whitespace);
            (indented && line.trim_start().starts_with("cf task "))
                || line.starts_with(&" ".repeat(CONTINUED))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The usage of the one command `cf <path…>` (`["note"]`, `["task", "get"]`):
/// its line in the list of commands, with the lines that go on from it. What
/// `cf <path…> --help` prints. A line names the commands it covers by their
/// words, `accept|cancel` for two; the `… --needs` lines say what `cf task add`
/// takes besides. What the usage says after the list (how to give a text on
/// the standard input) is for none of them.
pub fn of(path: &[&str]) -> String {
    let mut kept = false;
    let mut lines = Vec::new();
    let commands = usage()
        .lines()
        .skip_while(|line| !line.starts_with("  cf "))
        .take_while(|line| !line.is_empty());
    for line in commands {
        if !line.starts_with(&" ".repeat(CONTINUED)) {
            kept = covers(line, path);
        }
        if kept {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Whether the usage `line` is one of the command `cf <path…>`'s.
fn covers(line: &str, path: &[&str]) -> bool {
    let line = line.trim_start();
    if line.starts_with('…') {
        return path == ["task", "add"];
    }
    let Some(command) = line.strip_prefix("cf ") else {
        return false;
    };
    let mut words = command.split_whitespace();
    path.iter().all(|wanted| {
        words
            .next()
            .is_some_and(|word| word.split('|').any(|each| each == *wanted))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_its_line_of_the_list_with_the_lines_that_go_on_from_it() {
        assert_eq!(
            of(&["note"]),
            "  cf note \"…\" [--human]             something they should know; nothing waits on it"
        );
        let tell = of(&["tell"]);
        assert_eq!(tell.lines().count(), 2);
        assert!(tell
            .lines()
            .nth(1)
            .is_some_and(|line| line.starts_with(&" ".repeat(CONTINUED))));
        assert!(tell.contains("then cf task resume T-3"));
    }

    #[test]
    fn a_line_that_names_two_commands_is_the_usage_of_each() {
        let accept = of(&["task", "accept"]);
        assert_eq!(accept, of(&["task", "cancel"]));
        assert!(accept.starts_with("  cf task accept|cancel T-3"));
    }

    #[test]
    fn task_add_takes_the_lines_that_say_what_it_takes_besides_and_none_of_the_other_commands() {
        let add = of(&["task", "add"]);
        assert!(add.contains("… --needs T-3,T-4"), "{add}");
        assert!(add.contains("… --before T-9,T-10"), "{add}");
        assert!(
            !add.contains("cf task list") && !add.contains("cf task get"),
            "{add}"
        );
        assert!(!of(&["task", "get"]).contains("… --needs"));
    }

    #[test]
    fn what_the_usage_says_after_its_commands_is_no_commands() {
        assert!(!of(&["task", "add"]).contains("BRIEF"));
        assert_eq!(of(&["frobnicate"]), "");
        assert_eq!(of(&["task", "frobnicate"]), "");
    }
}
