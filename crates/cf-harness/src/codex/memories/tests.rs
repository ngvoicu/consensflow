//! Whether a folder is Codex's memories folder, or inside it: `$CODEX_HOME/memories`, and
//! `~/.codex/memories` where `CODEX_HOME` is unset or empty; its links resolved, and a
//! Windows path in any of its forms.

use super::*;
use std::fs;

fn env_of(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

/// A Codex home in a folder of its own, with its memories folder (the job's) and the
/// folders below it Codex makes (`rollout_summaries`).
struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Resolved: the system's temp folder may be reached through a link (a Mac's `/var`).
        let real = fs::canonicalize(root.path()).unwrap();
        fs::create_dir_all(
            real.join(".codex")
                .join("memories")
                .join("rollout_summaries"),
        )
        .unwrap();
        fs::create_dir_all(real.join(".codex").join("sessions")).unwrap();
        fs::create_dir_all(real.join("project").join("src")).unwrap();
        Self { root }
    }

    fn real(&self) -> PathBuf {
        fs::canonicalize(self.root.path()).unwrap()
    }

    fn at(&self, parts: &[&str]) -> PathBuf {
        parts.iter().fold(self.real(), |path, part| path.join(part))
    }

    fn text(&self, parts: &[&str]) -> String {
        self.at(parts).to_string_lossy().into_owned()
    }

    /// The environment of a window whose Codex home is `CODEX_HOME`: `.codex` of the home.
    fn env_with_codex_home(&self) -> Env {
        env_of(&[("CODEX_HOME", &self.text(&[".codex"]))])
    }

    /// The environment of a window with no `CODEX_HOME`: Codex's home is `.codex` of the user's.
    fn env_without_codex_home(&self) -> Env {
        env_of(&[("HOME", &self.text(&[]))])
    }
}

#[test]
fn the_memories_folder_and_every_folder_below_it_are_inside_it_and_nothing_else_of_codexs_home_is()
{
    let home = Home::new();
    for env in [home.env_with_codex_home(), home.env_without_codex_home()] {
        for inside in [
            home.at(&[".codex", "memories"]),
            home.at(&[".codex", "memories", "rollout_summaries"]),
            // A folder that is not there (yet) below it: the job makes its own.
            home.at(&[".codex", "memories", "extensions", "new"]),
        ] {
            assert!(holds(&env, &inside), "{} is inside", inside.display());
        }
        for outside in [
            home.at(&[".codex"]),
            home.at(&[".codex", "sessions"]),
            // Its name's start is not its folder.
            home.at(&[".codex", "memories-old"]),
            home.at(&["project"]),
            home.at(&["project", "src"]),
            home.at(&[]),
        ] {
            assert!(!holds(&env, &outside), "{} is outside", outside.display());
        }
    }
}

#[test]
fn a_codex_home_that_is_set_moves_the_memories_folder_and_the_default_one_is_then_no_job_s() {
    let home = Home::new();
    // Codex's home is `project` here, so its memories folder is `project/memories`.
    fs::create_dir_all(home.at(&["project", "memories"])).unwrap();
    let env = env_of(&[
        ("CODEX_HOME", &home.text(&["project"])),
        ("HOME", &home.text(&[])),
    ]);
    assert!(holds(&env, &home.at(&["project", "memories"])));
    assert!(holds(&env, &home.at(&["project", "memories", "x"])));
    assert!(!holds(&env, &home.at(&["project"])));
    assert!(!holds(&env, &home.at(&[".codex", "memories"])));
}

#[test]
fn an_empty_codex_home_is_none_and_a_window_with_no_home_at_all_has_no_memories_folder() {
    let home = Home::new();
    let empty = env_of(&[("CODEX_HOME", ""), ("HOME", &home.text(&[]))]);
    assert!(holds(&empty, &home.at(&[".codex", "memories"])));
    assert!(!holds(&empty, &home.at(&["project"])));
    // The user's home is `USERPROFILE` where `HOME` is not set, as Windows has it.
    let profile = env_of(&[("USERPROFILE", &home.text(&[]))]);
    assert!(holds(&profile, &home.at(&[".codex", "memories", "x"])));
    // No `CODEX_HOME` and no home: nowhere is inside a folder that is nowhere.
    let nowhere = env_of(&[]);
    assert!(!holds(&nowhere, &home.at(&[".codex", "memories"])));
    assert!(!holds(&nowhere, Path::new("")));
}

#[test]
fn a_home_and_a_user_profile_that_name_two_folders_both_have_a_memories_folder() {
    let first = Home::new();
    let second = Home::new();
    let env = env_of(&[
        ("HOME", &first.text(&[])),
        ("USERPROFILE", &second.text(&[])),
    ]);
    for home in [&first, &second] {
        assert!(holds(&env, &home.at(&[".codex", "memories"])));
        assert!(holds(&env, &home.at(&[".codex", "memories", "x"])));
        assert!(!holds(&env, &home.at(&[".codex"])));
        assert!(!holds(&env, &home.at(&["project"])));
    }
    // One that is set names the only one, whatever the homes are.
    let moved = env_of(&[
        ("CODEX_HOME", &first.text(&["project"])),
        ("HOME", &first.text(&[])),
        ("USERPROFILE", &second.text(&[])),
    ]);
    assert!(holds(&moved, &first.at(&["project", "memories"])));
    assert!(!holds(&moved, &first.at(&[".codex", "memories"])));
    assert!(!holds(&moved, &second.at(&[".codex", "memories"])));
}

#[cfg(unix)]
#[test]
fn a_link_into_the_memories_folder_is_inside_it_and_so_is_a_folder_below_a_linked_one() {
    use std::os::unix::fs::symlink;

    let home = Home::new();
    let memories = home.at(&[".codex", "memories"]);
    // A folder of the project that is a link to the memories folder: a command run from
    // there runs in the memories folder, whatever the folder is called.
    symlink(&memories, home.at(&["project", "notes"])).unwrap();
    // The memories folder reached through a link to Codex's home, as `CODEX_HOME` may name it.
    symlink(home.at(&[".codex"]), home.at(&["codex-link"])).unwrap();
    let env = home.env_with_codex_home();
    assert!(holds(&env, &home.at(&["project", "notes"])));
    assert!(holds(
        &env,
        &home.at(&["project", "notes", "rollout_summaries"])
    ));
    assert!(holds(&env, &home.at(&["codex-link", "memories"])));
    // And the other way: Codex's home is named through a link, the command runs in the folder itself.
    let through_a_link = env_of(&[("CODEX_HOME", &home.text(&["codex-link"]))]);
    assert!(holds(&through_a_link, &memories));
    assert!(holds(
        &through_a_link,
        &home.at(&["codex-link", "memories", "rollout_summaries"])
    ));
    // A link out of it does not bring the folder it points to in.
    symlink(
        home.at(&["project"]),
        home.at(&[".codex", "memories", "out"]),
    )
    .unwrap();
    assert!(!holds(&env, &home.at(&[".codex", "memories", "out"])));
    // A link whose target is not there is no memories folder, and fails nothing.
    symlink(home.at(&["gone"]), home.at(&["project", "dangling"])).unwrap();
    assert!(!holds(&env, &home.at(&["project", "dangling"])));
}

/// Whether `folder`, as it is written, is inside the memories folder `codex_home` names, by the
/// names of their folders alone: the same on every system, and nothing is looked for on disk.
fn written_inside(vars: &[(&str, &str)], folder: &str, fold: Fold) -> bool {
    let memories = memories_folders(&env_of(vars));
    assert!(!memories.is_empty(), "a memories folder");
    memories
        .iter()
        .any(|memories| inside(Path::new(folder), memories, fold))
}

#[test]
fn a_windows_path_is_inside_in_any_form_it_is_written_in() {
    let on_windows = |codex_home: &str, folder: &str| {
        written_inside(&[("CODEX_HOME", codex_home)], folder, Fold::WINDOWS)
    };
    let home = r"C:\Users\Gabriel\.codex";
    for folder in [
        r"C:\Users\Gabriel\.codex\memories",
        r"C:\Users\Gabriel\.codex\memories\rollout_summaries",
        // The separators the shell wrote.
        "C:/Users/Gabriel/.codex/memories",
        r"C:\Users\Gabriel/.codex\memories/rollout_summaries",
        // A trailing one, a `.`, and a `..` that stays inside.
        r"C:\Users\Gabriel\.codex\memories\",
        r"C:\Users\Gabriel\.codex\memories\.\rollout_summaries",
        r"C:\Users\Gabriel\.codex\memories\a\..\rollout_summaries",
        // Windows' own way to write a path whose links are resolved.
        r"\\?\C:\Users\Gabriel\.codex\memories",
        r"\\?\C:\Users\Gabriel\.codex\memories\rollout_summaries",
        // The drive and the folders in any case: NTFS does not tell them apart.
        r"c:\users\gabriel\.CODEX\Memories",
    ] {
        assert!(on_windows(home, folder), "{folder} is inside");
    }
    for folder in [
        r"C:\Users\Gabriel\.codex",
        r"C:\Users\Gabriel\.codex\sessions",
        r"C:\Users\Gabriel\.codex\memories-old",
        r"C:\Users\Gabriel\.codex\memories\..\sessions",
        r"C:\Users\Gabriel\project",
        // Another drive, and a folder of the same name elsewhere.
        r"D:\Users\Gabriel\.codex\memories",
        r"C:\Other\Users\Gabriel\.codex\memories",
        r"\\?\D:\Users\Gabriel\.codex\memories",
        r"\\server\share\Users\Gabriel\.codex\memories",
    ] {
        assert!(!on_windows(home, folder), "{folder} is outside");
    }
    // Codex's home written in any of the forms too, and a network one.
    for codex_home in [
        "C:/Users/Gabriel/.codex",
        r"\\?\C:\Users\Gabriel\.codex",
        r"c:\USERS\gabriel\.codex\",
    ] {
        assert!(
            on_windows(codex_home, r"C:\Users\Gabriel\.codex\memories\x"),
            "{codex_home}"
        );
    }
    assert!(on_windows(
        r"\\server\share\codex",
        r"\\?\UNC\server\share\codex\memories\x"
    ));
    assert!(!on_windows(
        r"\\server\share\codex",
        r"\\?\UNC\server\other\codex\memories\x"
    ));
    // Windows' default `.codex` is of the user's profile.
    assert!(written_inside(
        &[("USERPROFILE", r"C:\Users\Gabriel")],
        r"c:\users\gabriel\.codex\memories",
        Fold::WINDOWS
    ));
}

#[test]
fn a_mac_compares_the_names_of_folders_in_any_case_and_a_system_that_does_not_ignore_it_does_not() {
    // Written out, not joined: this holds on every system, and a Windows host joins with `\`.
    let memories = Path::new("/Users/gabriel/.codex/memories");
    let inside_on = |folder: &str, fold| inside(Path::new(folder), memories, fold);
    let folder = "/users/Gabriel/.CODEX/Memories/x";
    assert!(inside_on(folder, Fold::MAC));
    assert!(!inside_on(folder, Fold::LINUX));
    assert!(inside_on("/Users/gabriel/.codex/memories/x", Fold::LINUX));
    // `..` takes the folder before it and the root has none.
    assert!(!inside_on(
        "/Users/gabriel/.codex/memories/../sessions",
        Fold::MAC
    ));
    assert!(!inside_on("/../..", Fold::MAC));
    // The memories folder as `memories_folders` names it, for a Codex home written the same way.
    let named = memories_folders(&env_of(&[("CODEX_HOME", "/Users/gabriel/.codex")]));
    assert_eq!(named.len(), 1);
    assert!(named[0].ends_with("memories"));
}
