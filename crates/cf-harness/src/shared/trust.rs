//! The question a harness asks of a folder before it starts there: whether to
//! trust it. Claude Code asks it in a git repository it was never told to trust, and
//! waits on it with nothing else on the screen; Pi, OpenCode and Devin (opened with
//! `--respect-workspace-trust false`) never do, and nor does Codex as ConsensFlow
//! opens it, its window on the broker's server, though the same Codex opened alone
//! asks as Claude Code does (`cargo xtask live folder-trust`, 2026-10-21: Claude Code
//! 2.1.305, Codex 0.163.0, Pi 1.3.0, OpenCode 1.19.3, Devin 3000.11.92). A person
//! answers it with the arrow keys and Enter, and so does the engine
//! ([`crate::contract::Window::toward_trust`]); what a yes trusts is what this module
//! says ([`trusted_by_a_yes`]).

use std::path::Path;

/// The keys a person answers a harness's question with.
pub(crate) const DOWN: &[u8] = b"\x1b[B";
pub(crate) const ENTER: &[u8] = b"\r";

/// The folder a yes to the question trusts, for a window opened in `directory`:
/// the root of the git repository the folder is in (the nearest folder at or above
/// it that holds a `.git`, a worktree's file as a repository's folder), and the
/// folder itself where it is in none. Claude Code 2.1.305 keeps a yes for the
/// repository's root, whichever folder of it the window opened in, though its
/// question names the window's own folder (Codex 0.163.0 opened alone does the same,
/// and says so: "Trusting will apply to the repository root"). Out of a repository, it asks only
/// where no folder above is trusted, and a yes trusts the folder itself.
pub(crate) fn trusted_by_a_yes(directory: &str) -> String {
    Path::new(directory)
        .ancestors()
        .find(|folder| folder.join(".git").exists())
        .map_or_else(
            || directory.to_owned(),
            |root| root.to_string_lossy().into_owned(),
        )
}

/// The choice `cursor` is on in `screen`: the words after it on the last line
/// that begins with it.
pub(crate) fn chosen(screen: &[String], cursor: char) -> Option<&str> {
    screen
        .iter()
        .rev()
        .find_map(|line| line.trim_start().strip_prefix(cursor))
        .map(str::trim)
}

/// Whether some line of `screen` holds `words`.
pub(crate) fn shows(screen: &[String], words: &str) -> bool {
    screen.iter().any(|line| line.contains(words))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yes_trusts_the_root_of_the_repository_the_folder_is_in_or_the_folder_out_of_one() {
        let home = tempfile::tempdir().unwrap();
        let repository = home.path().join("repository");
        let deep = repository.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir(repository.join(".git")).unwrap();
        let plain = home.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let text = |path: &Path| path.to_string_lossy().into_owned();
        assert_eq!(trusted_by_a_yes(&text(&deep)), text(&repository));
        assert_eq!(trusted_by_a_yes(&text(&repository)), text(&repository));
        assert_eq!(trusted_by_a_yes(&text(&plain)), text(&plain));
        // A worktree's `.git` is a file.
        let worktree = repository.join("a").join("tree");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: elsewhere\n").unwrap();
        assert_eq!(trusted_by_a_yes(&text(&worktree)), text(&worktree));
    }

    #[test]
    fn the_choice_is_the_last_line_the_cursor_begins() {
        let screen: Vec<String> = ["  No, exit", " ❯ Yes, I trust this folder", "❯ typed"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(chosen(&screen, '❯'), Some("typed"));
        assert_eq!(chosen(&screen[..2], '❯'), Some("Yes, I trust this folder"));
        assert_eq!(chosen(&screen[..1], '❯'), None);
        assert!(shows(&screen, "No, exit") && !shows(&screen, "Trust and continue"));
    }
}
