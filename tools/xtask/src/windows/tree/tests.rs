//! The copy of the tree, tried on a git repository made for each test, with the
//! `git` and the `tar` of the machine the tests run on: what goes wrong with a
//! file that was deleted is what those two do with its name.

use std::fs;
use std::path::Path;

use cf_base::env::Env;

use super::*;

/// The environment of this process, less what a hook that runs the tests would
/// leave in it for git to read (the repository of the hook's own).
fn environment() -> Env {
    Env::from_vars(
        Env::from_process()
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().starts_with("GIT_"))
            .map(|(name, value)| (name.to_owned(), value.to_owned())),
    )
}

fn git(context: &Context, words: &[&str]) -> String {
    let asked = Invocation::new("git", &context.root).args(words);
    let answered = process::capture(&asked, &context.env).unwrap();
    assert_eq!(answered.code, 0, "{}: {}", asked.display(), answered.stderr);
    answered.stdout
}

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A checkout: a repository with these files in it, added to the index (not
/// committed: no one's name is needed for that).
fn checkout(files: &[(&str, &str)]) -> (tempfile::TempDir, Context) {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        root: dir.path().to_path_buf(),
        env: environment(),
    };
    git(&context, &["init", "-q"]);
    for (name, text) in files {
        write(dir.path(), name, text);
    }
    git(&context, &["add", "-A"]);
    (dir, context)
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

/// The files `tarball` holds, unpacked into a folder of their own. Unpacked there
/// from a copy named bare: a GNU tar (Git for Windows's, first on the PATH of
/// GitHub's runners) takes `C:\…` for a host, as an archive and as a folder.
fn unpacked(context: &Context, tarball: &Path) -> tempfile::TempDir {
    let into = tempfile::tempdir().unwrap();
    let copy = into.path().join("unpacking.tgz");
    fs::copy(tarball, &copy).unwrap();
    let untar = Invocation::new("tar", into.path()).args(["xzf", "unpacking.tgz"]);
    let answered = process::capture(&untar, &context.env).unwrap();
    assert_eq!(answered.code, 0, "{}", answered.stderr);
    fs::remove_file(&copy).unwrap();
    into
}

#[test]
fn the_tree_is_what_git_lists_of_it_that_is_on_disk_tracked_and_new_but_not_ignored() {
    let (dir, context) = checkout(&[
        (".gitignore", "ignored.out\n"),
        ("src/lib.rs", "fn main() {}\n"),
        ("notes.md", "notes\n"),
    ]);
    write(dir.path(), "new/file.txt", "not added yet");
    write(dir.path(), "ignored.out", "build output");
    assert_eq!(
        sorted(files(&context).unwrap()),
        [".gitignore", "new/file.txt", "notes.md", "src/lib.rs"]
    );
}

#[test]
fn a_tracked_file_deleted_from_the_tree_is_left_out_though_git_still_lists_it() {
    let (dir, context) = checkout(&[("keep.txt", "kept"), ("gone.txt", "to be deleted")]);
    fs::remove_file(dir.path().join("gone.txt")).unwrap();
    // The scenario: the index has it, the disk has not.
    assert_eq!(
        sorted(
            git(&context, &["ls-files", "-co", "--exclude-standard"])
                .lines()
                .map(str::to_owned)
                .collect()
        ),
        ["gone.txt", "keep.txt"]
    );
    assert_eq!(files(&context).unwrap(), ["keep.txt"]);
}

#[test]
fn what_goes_over_is_packed_with_the_list_of_it_which_leaves_out_what_was_deleted() {
    let (dir, context) = checkout(&[
        ("keep.txt", "kept"),
        ("src/deep/lib.rs", "fn lib() {}\n"),
        ("gone.txt", "to be deleted"),
    ]);
    fs::remove_file(dir.path().join("gone.txt")).unwrap();
    write(dir.path(), "new.txt", "new");
    let scratch = tempfile::tempdir().unwrap();

    let listed = files(&context).unwrap();
    let tarball = pack(&context, &listed, scratch.path()).unwrap();

    let there = unpacked(&context, &tarball);
    let held = |name: &str| fs::read_to_string(there.path().join(name)).ok();
    assert_eq!(held("keep.txt").as_deref(), Some("kept"));
    assert_eq!(held("src/deep/lib.rs").as_deref(), Some("fn lib() {}\n"));
    assert_eq!(held("new.txt").as_deref(), Some("new"));
    assert_eq!(held("gone.txt"), None);
    // The list the machine takes away what has left the tree by: only what went over.
    let list = held(SYNCED).unwrap();
    assert_eq!(
        sorted(list.lines().map(str::to_owned).collect()),
        ["keep.txt", "new.txt", "src/deep/lib.rs"]
    );
    assert!(list.ends_with('\n'));
    // Beside it, the sweep that removes from the copy what the list does not name.
    assert_eq!(held(SWEEP), Some(sweep_script()));
    // And neither is left in the checkout.
    assert!(!dir.path().join(SYNCED).exists());
    assert!(!dir.path().join(SWEEP).exists());
    assert!(git(&context, &["status", "--short"]).contains("new.txt"));
}

#[test]
fn a_tarball_that_cannot_be_made_is_said_with_what_tar_said_and_leaves_no_list_behind() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    let scratch = tempfile::tempdir().unwrap();
    // A name with nothing behind it is what the copy broke on before the tree was filtered.
    let said = pack(&context, &["gone.txt".to_owned()], scratch.path()).unwrap_err();
    let said = said.to_string();
    assert!(said.starts_with("`tar czf "), "{said}");
    // What tar says of it is its own, which differs by system (`gone.txt: Cannot
    // stat` where one has the name, `Couldn't visit directory` where another has not).
    let (_, after) = said.split_once("` ended with status ").unwrap();
    let words = after.trim_start_matches(|c: char| c.is_ascii_digit());
    assert!(words.starts_with(": ") && words.len() > 2, "{said}");
    assert!(!dir.path().join(SYNCED).exists());
    assert!(!dir.path().join(SWEEP).exists());
}

#[test]
fn a_directory_that_is_not_a_repository_is_said_with_what_git_said() {
    let dir = tempfile::tempdir().unwrap();
    // No repository above the folder is looked for, whatever the temporary folder is in.
    let above = dir.path().parent().unwrap();
    let env = Env::from_vars(
        environment()
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([(
                "GIT_CEILING_DIRECTORIES".into(),
                above.as_os_str().to_owned(),
            )]),
    );
    let context = Context {
        root: dir.path().to_path_buf(),
        env,
    };
    let said = files(&context).unwrap_err().to_string();
    assert!(
        said.starts_with("`git ls-files -co --exclude-standard -z` ended with status"),
        "{said}"
    );
    assert!(said.contains("not a git repository"), "{said}");
}

#[test]
fn the_tarball_is_made_by_tar_in_the_scratch_from_the_checkout_by_a_list_of_names_and_without_the_resource_files_of_macos(
) {
    let context = Context {
        root: PathBuf::from("checkout"),
        env: Env::default(),
    };
    let made = tar(&context, Path::new("scratch"), Path::new("scratch/files"));
    assert_eq!(made.program, "tar");
    // The archive is named bare, in the folder tar runs in: a GNU tar takes `C:\…`
    // for a host (Git for Windows's, first on the PATH of GitHub's runners).
    assert_eq!(made.cwd, Path::new("scratch"));
    let words: Vec<_> = made
        .args
        .iter()
        .map(|word| word.to_string_lossy())
        .collect();
    assert_eq!(
        words,
        [
            "czf",
            "tree.tgz",
            "-C",
            "checkout",
            "--null",
            "-T",
            "scratch/files"
        ]
        .map(String::from)
    );
    assert_eq!(made.vars, [("COPYFILE_DISABLE".into(), Some("1".into()))]);
}

#[test]
fn a_name_git_wrote_that_is_no_text_is_refused_and_one_that_is_not_there_is_left_out() {
    let listing = "a.txt\0\u{fffd}.txt\0gone.txt\0\0";
    let said = kept(listing, |_| true).unwrap_err().to_string();
    assert_eq!(
        said,
        "a file of the tree has a name that is not text, and none is sent: \u{fffd}.txt"
    );
    let exists = |name: &str| name != "gone.txt";
    assert_eq!(
        kept("a.txt\0gone.txt\0b.txt\0", exists).unwrap(),
        ["a.txt", "b.txt"]
    );
    assert_eq!(kept("", exists).unwrap(), Vec::<String>::new());
}

/// The checkout's own `app/.gitignore`, read as it is now.
#[cfg(unix)]
fn app_gitignore() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/.gitignore");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A worker's tree has `app/.cache` and `app/src-tauri/resources` as links into the main
/// checkout, which hold what the main checkout staged: the checkout's `app/.gitignore`
/// keeps them from the run, as it keeps the folders they are in the main checkout, and
/// leaves every other file of `app` to go.
#[cfg(unix)]
#[test]
fn the_staged_folders_go_neither_as_folders_nor_as_a_workers_links_into_the_main_checkout() {
    let main = tempfile::tempdir().unwrap();
    write(main.path(), "cache/conpty.zip", "package");
    write(main.path(), "resources/cli/bin/cf.exe", "cf");
    let (dir, context) = checkout(&[
        ("app/.gitignore", &app_gitignore()),
        ("app/package.json", "{}"),
        ("app/src-tauri/tauri.conf.json", "{}"),
    ]);
    write(dir.path(), "app/new.txt", "not added yet");
    std::os::unix::fs::symlink(main.path().join("cache"), dir.path().join("app/.cache")).unwrap();
    std::os::unix::fs::symlink(
        main.path().join("resources"),
        dir.path().join("app/src-tauri/resources"),
    )
    .unwrap();
    let listed = [
        "app/.gitignore",
        "app/new.txt",
        "app/package.json",
        "app/src-tauri/tauri.conf.json",
    ];
    assert_eq!(sorted(files(&context).unwrap()), listed);
    // The main checkout's own: the folders, with what was staged in them.
    let (dir, context) = checkout(&[
        ("app/.gitignore", &app_gitignore()),
        ("app/package.json", "{}"),
        ("app/src-tauri/tauri.conf.json", "{}"),
    ]);
    write(dir.path(), "app/new.txt", "not added yet");
    write(dir.path(), "app/.cache/conpty.zip", "package");
    write(dir.path(), "app/src-tauri/resources/cli/bin/cf.exe", "cf");
    assert_eq!(sorted(files(&context).unwrap()), listed);
    // A file of another folder of that name is the tree's.
    write(dir.path(), "app/ui/.cache", "a file of the page's");
    assert!(files(&context)
        .unwrap()
        .contains(&"app/ui/.cache".to_owned()));
}

#[cfg(unix)]
#[test]
fn a_link_whose_target_is_gone_is_still_a_file_of_the_tree() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    std::os::unix::fs::symlink("nowhere", dir.path().join("dangling")).unwrap();
    assert_eq!(sorted(files(&context).unwrap()), ["dangling", "keep.txt"]);
    let scratch = tempfile::tempdir().unwrap();
    let tarball = pack(&context, &files(&context).unwrap(), scratch.path()).unwrap();
    let there = unpacked(&context, &tarball);
    assert_eq!(
        fs::read_link(there.path().join("dangling")).unwrap(),
        Path::new("nowhere")
    );
}
