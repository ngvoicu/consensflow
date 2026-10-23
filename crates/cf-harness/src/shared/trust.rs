//! The question a harness asks of a folder before it starts there: whether to
//! trust it. Claude Code asks it in a git repository it was never told to trust, and
//! waits on it with nothing else on the screen; Pi, OpenCode and Devin (opened with
//! `--respect-workspace-trust false`) never do, and nor does Codex as ConsensFlow
//! opens it, its window on the broker's server, though the same Codex opened alone
//! asks as Claude Code does (`cargo xtask live folder-trust`, 2026-10-21: Claude Code
//! 2.1.305, Codex 0.163.0, Pi 1.3.0, OpenCode 1.19.3, Devin 3000.11.92). A person
//! answers it with the arrow keys and Enter, and so does the engine
//! ([`crate::contract::Window::toward_trust`]); what a yes trusts is what this module
//! says ([`trusts`]).

use std::fs;
use std::path::{Path, PathBuf};

use super::directory::real_path;
pub(crate) use crate::contract::Trusts;

/// The keys a person answers a harness's question with.
pub(crate) const DOWN: &[u8] = b"\x1b[B";
pub(crate) const UP: &[u8] = b"\x1b[A";
pub(crate) const ENTER: &[u8] = b"\r";

/// What a yes to the question trusts, for a window opened in the project's folder
/// `directory`: the root of the git repository the folder is in, found from the
/// folder as the system names it (Claude Code walks its physical working folder: a
/// link in the project's path may lead into a repository no folder of the path is
/// in), and the folder itself where it is in none. Claude Code 2.1.305 keeps a yes
/// for the repository's root, whichever folder of it the window opened in, though
/// its question names the window's own folder (Codex 0.163.0 opened alone does the
/// same, and says so: "Trusting will apply to the repository root"); and for a
/// linked worktree, whose `.git` is a file, the root of the checkout it belongs to
/// ([`root_of`]). Out of a repository, it asks only where no folder above is
/// trusted, and a yes trusts the folder itself. The yes is the project's where that
/// folder is the project's folder or one below it, both as the system names them.
/// Claude Code 2.1.309 reads the `.mcp.json` of the window's folder and of every
/// folder above it, up to the root, as the project's servers, and asks of them in
/// one dialog (T-383): the nearest folder above the project's that has one is named.
pub(crate) fn trusts(directory: &str) -> Trusts {
    let project = real_path(directory).unwrap_or_else(|_| directory.to_owned());
    let folder = Path::new(&project)
        .ancestors()
        .find_map(root_of)
        .map_or_else(|| project.clone(), |root| plain(&root));
    let inside = Path::new(&folder).starts_with(&project);
    let servers_above = Path::new(&project)
        .ancestors()
        .skip(1)
        .find(|above| above.join(".mcp.json").is_file())
        .map(|above| above.to_string_lossy().into_owned());
    Trusts {
        folder,
        inside,
        servers_above,
    }
}

/// The root of the repository whose `.git` is in `folder`, where it has one: the
/// folder itself, or, where its `.git` is a file that names a folder of git's own
/// (`gitdir:`) that shares another's (`commondir`: a linked worktree's, in
/// `<checkout>/.git/worktrees/<name>`), the checkout that common folder is the
/// `.git` of, and the common folder itself where it is no `.git` (a bare
/// repository's). A file that says nothing of the kind (a submodule's, in
/// `<superproject>/.git/modules/<name>`) leaves the folder its root.
fn root_of(folder: &Path) -> Option<PathBuf> {
    let git = folder.join(".git");
    if !git.exists() {
        return None;
    }
    let own = || Some(folder.to_path_buf());
    let Ok(text) = fs::read_to_string(&git) else {
        return own();
    };
    let Some(named) = text.lines().find_map(|line| line.strip_prefix("gitdir:")) else {
        return own();
    };
    let gitdir = folder.join(named.trim());
    let Ok(common) = fs::read_to_string(gitdir.join("commondir")) else {
        return own();
    };
    let common = gitdir.join(common.trim());
    let common = fs::canonicalize(&common).unwrap_or(common);
    Some(if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().map_or(common.clone(), Path::to_path_buf)
    } else {
        common
    })
}

/// A path as `real_path` writes one.
fn plain(path: &Path) -> String {
    real_path(&path.to_string_lossy()).unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// The choice `cursor` is on in `screen`, and the line it is on: the words after it
/// on the last line that begins with it, a box's edge before it aside.
pub(crate) fn chosen_at(screen: &[String], cursor: char) -> Option<(usize, &str)> {
    screen.iter().enumerate().rev().find_map(|(at, line)| {
        line.trim_start_matches(blank_or_edge)
            .strip_prefix(cursor)
            .map(|choice| (at, choice.trim()))
    })
}

/// Whether some line of `screen` holds `words`.
pub(crate) fn shows(screen: &[String], words: &str) -> bool {
    screen.iter().any(|line| line.contains(words))
}

/// Whether `screen` offers the choice `words`: a line that names it ([`names`]).
pub(crate) fn offers(screen: &[String], words: &str) -> bool {
    screen.iter().any(|line| names(line, words))
}

/// Whether `line` is the choice `words` as a list of choices draws one: the words
/// with nothing but a cursor or a number before them, and nothing but blanks and a
/// box's edge (`│`, `┃`, `║`) after them. A line that quotes them among other words,
/// or that other words follow, names nothing.
pub(crate) fn names(line: &str, words: &str) -> bool {
    line.trim_end_matches(blank_or_edge)
        .strip_suffix(words)
        .is_some_and(|before| !before.chars().any(char::is_alphabetic))
}

/// A blank, or a box's edge as a terminal draws one.
fn blank_or_edge(at: char) -> bool {
    at.is_whitespace() || matches!(at, '│' | '┃' | '║')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder made in `home`, as the system names it.
    fn made(home: &Path, name: &str) -> String {
        let folder = home.join(name);
        fs::create_dir_all(&folder).unwrap();
        real_path(&folder.to_string_lossy()).unwrap()
    }

    #[test]
    fn a_yes_trusts_the_root_of_the_repository_the_folder_is_in_or_the_folder_out_of_one() {
        let home = tempfile::tempdir().unwrap();
        let repository = made(home.path(), "repository");
        fs::create_dir(Path::new(&repository).join(".git")).unwrap();
        let deep = made(home.path(), "repository/a/b");
        let plain = made(home.path(), "plain");
        let trusted = |folder: &str, inside: bool| Trusts {
            folder: folder.to_owned(),
            inside,
            servers_above: None,
        };
        assert_eq!(trusts(&deep), trusted(&repository, false));
        assert_eq!(trusts(&repository), trusted(&repository, true));
        assert_eq!(trusts(&plain), trusted(&plain, true));
        // A folder that cannot be named as the system names it is taken as given.
        let gone = home.path().join("gone").to_string_lossy().into_owned();
        assert_eq!(trusts(&gone), trusted(&gone, true));
    }

    #[test]
    fn a_linked_worktree_trusts_the_checkout_it_belongs_to_and_a_submodule_itself() {
        let home = tempfile::tempdir().unwrap();
        let main = made(home.path(), "main");
        let worktree_git = made(home.path(), "main/.git/worktrees/tree");
        fs::write(Path::new(&worktree_git).join("commondir"), "../..\n").unwrap();
        let tree = made(home.path(), "trees/tree");
        fs::write(
            Path::new(&tree).join(".git"),
            format!("gitdir: {worktree_git}\n"),
        )
        .unwrap();
        let below = made(home.path(), "trees/tree/app");
        for folder in [&tree, &below] {
            assert_eq!(
                trusts(folder),
                Trusts {
                    folder: main.clone(),
                    inside: false,
                    servers_above: None,
                },
                "{folder}"
            );
        }
        // A submodule's `.git` names a folder of the superproject's, which shares none.
        let modules = made(home.path(), "main/.git/modules/sub");
        let sub = made(home.path(), "main/sub");
        fs::write(
            Path::new(&sub).join(".git"),
            "gitdir: ../.git/modules/sub\n",
        )
        .unwrap();
        assert!(Path::new(&modules).is_dir());
        assert_eq!(
            trusts(&sub),
            Trusts {
                folder: sub.clone(),
                inside: true,
                servers_above: None,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_projects_path_leads_to_the_repository_the_system_finds() {
        let home = tempfile::tempdir().unwrap();
        let repository = made(home.path(), "B/repo");
        fs::create_dir(Path::new(&repository).join(".git")).unwrap();
        made(home.path(), "B/repo/x/sub");
        made(home.path(), "A");
        std::os::unix::fs::symlink(
            Path::new(&repository).join("x"),
            home.path().join("A").join("link"),
        )
        .unwrap();
        let through = home
            .path()
            .join("A/link/sub")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            trusts(&through),
            Trusts {
                folder: repository,
                inside: false,
                servers_above: None,
            }
        );
    }

    #[test]
    fn the_nearest_folder_above_the_project_whose_mcp_json_claude_reads_is_named() {
        let home = tempfile::tempdir().unwrap();
        let project = made(home.path(), "a/b/project");
        fs::write(Path::new(&project).join(".mcp.json"), "{}").unwrap();
        // The project's own file is the project's.
        assert_eq!(trusts(&project).servers_above, None);
        let a = made(home.path(), "a");
        fs::write(Path::new(&a).join(".mcp.json"), "{}").unwrap();
        assert_eq!(trusts(&project).servers_above, Some(a.clone()));
        let b = made(home.path(), "a/b");
        fs::write(Path::new(&b).join(".mcp.json"), "{}").unwrap();
        assert_eq!(trusts(&project).servers_above, Some(b));
        // A folder called so is no file of servers.
        let c = made(home.path(), "c/project");
        made(home.path(), "c/.mcp.json");
        assert_eq!(trusts(&c).servers_above, None);
    }

    #[test]
    fn the_choice_is_the_last_line_the_cursor_begins() {
        let screen: Vec<String> = ["  No, exit", " ❯ Yes, I trust this folder", "❯ typed"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(chosen_at(&screen, '❯'), Some((2, "typed")));
        assert_eq!(
            chosen_at(&screen[..2], '❯'),
            Some((1, "Yes, I trust this folder"))
        );
        assert_eq!(chosen_at(&screen[..1], '❯'), None);
        assert!(shows(&screen, "No, exit") && !shows(&screen, "Trust and continue"));
    }

    #[test]
    fn a_choice_is_offered_on_a_line_of_its_own_with_a_cursor_or_a_number_before_it() {
        let screen: Vec<String> = [" ❯ 1. Yes", "   > No ", "  it said \"Maybe\" twice: Maybe"]
            .map(str::to_owned)
            .to_vec();
        assert!(offers(&screen, "Yes") && offers(&screen, "No"));
        assert!(!offers(&screen, "Maybe") && !offers(&screen, "Later"));
    }

    #[test]
    fn a_choice_in_a_box_is_offered_and_one_followed_by_other_words_is_not() {
        let boxed: Vec<String> = [
            "│ ❯ No, exit                 │",
            "┃   Yes, I trust this folder ┃",
            "║ 2. Later ║",
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(offers(&boxed, "No, exit") && offers(&boxed, "Yes, I trust this folder"));
        assert!(offers(&boxed, "Later"));
        // Ordinary prose that ends in other words, or in a stop, offers nothing.
        let prose: Vec<String> = [
            "  No, exit was what it chose",
            "  - Yes, I trust this folder.",
            "  No, exit │ then more",
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(!offers(&prose, "No, exit") && !offers(&prose, "Yes, I trust this folder"));
        // The cursor inside a box's edge is the choice it is on.
        assert_eq!(
            chosen_at(&boxed, '❯').map(|(_, choice)| choice),
            Some("No, exit                 │")
        );
        assert!(names("No, exit                 │", "No, exit"));
    }
}
