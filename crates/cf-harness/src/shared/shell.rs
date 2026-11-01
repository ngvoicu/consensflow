//! What a harness's config says to a shell: one word of a command line, and the
//! command of a `cf hook`.

/// `value` as one word of a shell's line: single-quoted, each quote of its own closed
/// around.
pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// The command a harness's config runs for `cf hook <hook> <args>`: the window's own `cf`
/// by its whole path (`pane_cf`, quoted) and never the word `cf`, which the shell a hook
/// runs in can find another program for (a profile puts its own folders first on the
/// `PATH`, as a Devin member's login zsh and a Claude Code member's login Git Bash do,
/// 2026-10-31). Each argument is quoted.
pub(crate) fn cf_hook(pane_cf: &str, hook: &str, args: &[&str]) -> String {
    let mut command = format!("{} hook {hook}", quote(pane_cf));
    for arg in args {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_word_is_quoted_with_each_quote_of_its_own_closed_around() {
        assert_eq!(quote("/a b/cf"), "'/a b/cf'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("''"), r"''\'''\'''");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn a_hook_is_run_by_the_whole_path_of_the_cf_and_each_argument_is_one_word() {
        assert_eq!(
            cf_hook(
                "/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf",
                "claude",
                &[]
            ),
            "'/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf' hook claude"
        );
        assert_eq!(
            cf_hook("/a b/cf", "muse-session", &["/r o/le.md", "it's"]),
            r"'/a b/cf' hook muse-session '/r o/le.md' 'it'\''s'"
        );
    }
}
