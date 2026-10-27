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
    let tarball = pack(
        &context,
        &listed,
        "none 0123456789abcdef",
        "the run script\n",
        scratch.path(),
    )
    .unwrap();

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
    // And the tree's identity, which the machine holds what it unpacked to and says.
    assert_eq!(held(TREE).as_deref(), Some("none 0123456789abcdef\n"));
    // And the run script, which is what the run is asked for.
    assert_eq!(held(RUN).as_deref(), Some("the run script\n"));
    // And none of the four is left in the checkout.
    for name in [SYNCED, SWEEP, TREE, RUN] {
        assert!(!dir.path().join(name).exists(), "{name}");
    }
    assert!(git(&context, &["status", "--short"]).contains("new.txt"));
}

#[test]
fn a_tarball_that_cannot_be_made_is_said_with_what_tar_said_and_leaves_no_list_behind() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    let scratch = tempfile::tempdir().unwrap();
    // A name with nothing behind it is what the copy broke on before the tree was filtered.
    let said = pack(
        &context,
        &["gone.txt".to_owned()],
        "none x",
        "",
        scratch.path(),
    )
    .unwrap_err();
    let said = said.to_string();
    assert!(said.starts_with("`tar cf "), "{said}");
    // What tar says of it is its own, which differs by system (`gone.txt: Cannot
    // stat` where one has the name, `Couldn't visit directory` where another has not).
    let (_, after) = said.split_once("` ended with status ").unwrap();
    let words = after.trim_start_matches(|c: char| c.is_ascii_digit());
    assert!(words.starts_with(": ") && words.len() > 2, "{said}");
    for name in [SYNCED, SWEEP, TREE, RUN] {
        assert!(!dir.path().join(name).exists(), "{name}");
    }
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
            "cf",
            "tree.tar",
            "-C",
            "checkout",
            "--null",
            "-T",
            "scratch/files"
        ]
        .map(String::from)
    );
    assert_eq!(made.vars, [("COPYFILE_DISABLE".into(), Some("1".into()))]);
    // What the run keeps of its own is appended from its folder in the scratch, in a second
    // step: no one `tar` line reads two folders the same on every system the runner runs on.
    let appended = append(
        Path::new("scratch/state"),
        [".synced-files", ".windows-tree"].into_iter(),
    );
    assert_eq!(appended.program, "tar");
    assert_eq!(appended.cwd, Path::new("scratch/state"));
    let words: Vec<_> = appended
        .args
        .iter()
        .map(|word| word.to_string_lossy())
        .collect();
    assert_eq!(
        words,
        ["rf", "../tree.tar", ".synced-files", ".windows-tree"].map(String::from)
    );
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
fn app_gitignore() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/.gitignore");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A checkout of `app` under the checkout's own `app/.gitignore`, with a new file in it.
fn app_checkout() -> (tempfile::TempDir, Context) {
    let (dir, context) = checkout(&[
        ("app/.gitignore", &app_gitignore()),
        ("app/package.json", "{}"),
        ("app/src-tauri/tauri.conf.json", "{}"),
    ]);
    write(dir.path(), "app/new.txt", "not added yet");
    (dir, context)
}

/// What goes of [`app_checkout`] where the staged folders do not.
const APP_LISTED: [&str; 4] = [
    "app/.gitignore",
    "app/new.txt",
    "app/package.json",
    "app/src-tauri/tauri.conf.json",
];

/// The checkout's `app/.gitignore` keeps `app/.cache` and `app/src-tauri/resources` from
/// the run whatever they are: the main checkout's folders, with what was staged in them,
/// or anything else of those names, as a worker's links to them are to git, which looks
/// at a link and not at what it points at (a pattern that ends in `/` matches a folder
/// only). A link is a plain file here, which every system makes: worker trees, and their
/// links into the main checkout, are the Mac's, and Windows makes a link only with a
/// privilege (the links themselves: the next test, on Unix). Every other file of `app`
/// goes.
#[test]
fn the_staged_folders_go_neither_as_the_main_checkouts_folders_nor_as_anything_else_of_their_names()
{
    let (dir, context) = app_checkout();
    write(dir.path(), "app/.cache/conpty.zip", "package");
    write(dir.path(), "app/src-tauri/resources/cli/bin/cf.exe", "cf");
    assert_eq!(sorted(files(&context).unwrap()), APP_LISTED);
    // In a worker's tree they are no folders.
    let (dir, context) = app_checkout();
    write(dir.path(), "app/.cache", "where a worker has a link");
    write(
        dir.path(),
        "app/src-tauri/resources",
        "where a worker has a link",
    );
    assert_eq!(sorted(files(&context).unwrap()), APP_LISTED);
    // A file of another folder of that name is the tree's.
    write(dir.path(), "app/ui/.cache", "a file of the page's");
    assert!(files(&context)
        .unwrap()
        .contains(&"app/ui/.cache".to_owned()));
}

/// A worker's tree as it is on the Mac: `app/.cache` and `app/src-tauri/resources` are
/// links into the main checkout, which hold what the main checkout staged, and they do
/// not go.
#[cfg(unix)]
#[test]
fn a_workers_links_into_the_main_checkouts_staged_folders_do_not_go() {
    let main = tempfile::tempdir().unwrap();
    write(main.path(), "cache/conpty.zip", "package");
    write(main.path(), "resources/cli/bin/cf.exe", "cf");
    let (dir, context) = app_checkout();
    std::os::unix::fs::symlink(main.path().join("cache"), dir.path().join("app/.cache")).unwrap();
    std::os::unix::fs::symlink(
        main.path().join("resources"),
        dir.path().join("app/src-tauri/resources"),
    )
    .unwrap();
    assert_eq!(sorted(files(&context).unwrap()), APP_LISTED);
}

#[cfg(unix)]
#[test]
fn a_link_whose_target_is_gone_is_still_a_file_of_the_tree() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    std::os::unix::fs::symlink("nowhere", dir.path().join("dangling")).unwrap();
    assert_eq!(sorted(files(&context).unwrap()), ["dangling", "keep.txt"]);
    let scratch = tempfile::tempdir().unwrap();
    let tarball = pack(
        &context,
        &files(&context).unwrap(),
        "none x",
        "",
        scratch.path(),
    )
    .unwrap();
    let there = unpacked(&context, &tarball);
    assert_eq!(
        fs::read_link(there.path().join("dangling")).unwrap(),
        Path::new("nowhere")
    );
}

/// The identity of the tree as the checkout is now.
fn identity_of(context: &Context) -> String {
    identity(context, &files(context).unwrap()).unwrap()
}

/// Commits what is added, as no one in particular.
fn commit(context: &Context) -> String {
    git(
        context,
        &[
            "-c",
            "user.name=windows-test",
            "-c",
            "user.email=windows-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "a commit",
        ],
    );
    git(context, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// The words of an identity: the commit, and the hash of what goes over.
fn words(identity: &str) -> (&str, &str) {
    let (commit, content) = identity.split_once(' ').unwrap();
    assert!(!content.contains(' '), "{identity}");
    (commit, content)
}

#[test]
fn a_tree_is_the_commit_the_checkout_is_at_and_a_hash_of_what_goes_over() {
    let (dir, context) = checkout(&[("keep.txt", "kept"), ("src/lib.rs", "fn lib() {}\n")]);
    // No commit yet: said so, as the first words, and the hash is 16 hex digits.
    let (commit_before, content) = {
        let identity = identity_of(&context);
        let (commit, content) = words(&identity);
        (commit.to_owned(), content.to_owned())
    };
    assert_eq!(commit_before, "none");
    assert!(
        content.len() == 16
            && content
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{content}"
    );
    // At a commit, the commit is what `git rev-parse HEAD` says, whole; what goes over
    // is the same, so its hash is.
    let head = commit(&context);
    let identity = identity_of(&context);
    assert_eq!(words(&identity), (head.as_str(), content.as_str()));
    assert_eq!(head.len(), 40);
    // Asked twice, it says the same.
    assert_eq!(identity_of(&context), identity);
    drop(dir);
}

#[test]
fn what_goes_over_changing_changes_the_tree_whatever_git_calls_the_change() {
    let (dir, context) = checkout(&[("keep.txt", "kept"), ("src/lib.rs", "fn lib() {}\n")]);
    let head = commit(&context);
    let clean = identity_of(&context);
    let hash = |identity: &str| words(identity).1.to_owned();
    let mut seen = vec![hash(&clean)];
    let mut differs = |what: &str, context: &Context| {
        let identity = identity_of(context);
        // The commit is the checkout's, and has not moved: only what goes over has.
        assert_eq!(words(&identity).0, head, "{what}");
        assert!(
            !seen.contains(&hash(&identity)),
            "{what}: the tree is as one before"
        );
        seen.push(hash(&identity));
    };
    // A tracked file changed, and not added.
    write(dir.path(), "keep.txt", "kept, and changed");
    differs("a tracked file changed", &context);
    // And back as it was: the same tree as at the commit, whichever way it got there.
    write(dir.path(), "keep.txt", "kept");
    assert_eq!(identity_of(&context), clean);
    // A file that git has never been told of, which goes over all the same.
    write(dir.path(), "new/file.txt", "not added yet");
    differs("a new file", &context);
    // A file of another name with the same bytes is another tree.
    fs::rename(
        dir.path().join("new/file.txt"),
        dir.path().join("new/other.txt"),
    )
    .unwrap();
    differs("a file renamed", &context);
    fs::remove_file(dir.path().join("new/other.txt")).unwrap();
    // A tracked file deleted.
    fs::remove_file(dir.path().join("src/lib.rs")).unwrap();
    differs("a file deleted", &context);
    // An ignored file does not go over, and is not part of the tree.
    write(dir.path(), ".gitignore", "ignored.out\n");
    let with_ignore = {
        let before = identity_of(&context);
        write(dir.path(), "ignored.out", "build output");
        before
    };
    assert_eq!(identity_of(&context), with_ignore);
}

#[test]
fn what_one_file_ends_with_and_the_next_begins_with_is_no_other_tree_than_where_each_ends() {
    // "a" holding "bc" and "ab" holding "c" are not the same bytes to a hash that only
    // read them end to end: the lengths are part of it.
    let (_one, first) = checkout(&[("a", "bc")]);
    let (_two, second) = checkout(&[("ab", "c")]);
    assert_ne!(
        words(&identity_of(&first)).1,
        words(&identity_of(&second)).1
    );
    let (_three, third) = checkout(&[("a", "b"), ("c", "d")]);
    let (_four, fourth) = checkout(&[("a", ""), ("bc", "d")]);
    assert_ne!(
        words(&identity_of(&third)).1,
        words(&identity_of(&fourth)).1
    );
}

#[test]
fn the_same_files_at_another_commit_are_another_tree_by_the_commit_and_the_same_by_the_hash() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    let first = commit(&context);
    let before = identity_of(&context);
    write(dir.path(), "other.txt", "other");
    git(&context, &["add", "-A"]);
    let second = commit(&context);
    assert_ne!(first, second);
    // Take the file out again, and commit that: the files are those of the first commit.
    fs::remove_file(dir.path().join("other.txt")).unwrap();
    git(&context, &["add", "-A"]);
    let third = commit(&context);
    let after = identity_of(&context);
    assert_ne!(words(&before).0, words(&after).0);
    assert_eq!(words(&after).0, third);
    assert_eq!(words(&before).1, words(&after).1);
}

#[cfg(unix)]
#[test]
fn a_link_is_known_by_its_name_alone_and_never_followed_for_a_machine_makes_no_link_of_it() {
    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    // A link, whose target is gone, is part of the tree as it is a file of it; what a
    // machine's file system makes of it is not what the Mac has, so it is `other`.
    std::os::unix::fs::symlink("nowhere", dir.path().join("link")).unwrap();
    let one = identity_of(&context);
    fs::remove_file(dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink("elsewhere", dir.path().join("link")).unwrap();
    assert_eq!(identity_of(&context), one);
    // But it is there: without it the tree is another.
    fs::remove_file(dir.path().join("link")).unwrap();
    assert_ne!(words(&identity_of(&context)).1, words(&one).1);
    // Nor is a link to a folder read as a file.
    std::os::unix::fs::symlink(dir.path(), dir.path().join("link")).unwrap();
    identity(&context, &["keep.txt".to_owned(), "link".to_owned()]).unwrap();
}

#[test]
fn a_tree_whose_file_cannot_be_read_is_said_with_the_file() {
    let (_dir, context) = checkout(&[("keep.txt", "kept")]);
    let said = identity(&context, &["gone.txt".to_owned()])
        .unwrap_err()
        .to_string();
    assert!(said.starts_with("could not read "), "{said}");
    assert!(said.contains("gone.txt"), "{said}");
}

/// What a run keeps of its own while it packs is in its own scratch folder and never in the
/// checkout: a checkout whose root cannot be written to is packed all the same. (Before, the
/// list, the identity, the sweep and the run script were files of the root for as long as
/// `tar` read it, and a second run from the same tree, to the other machine, wrote over them
/// and took them away: T-664, 2026-10-26, a run's upload failed.)
#[cfg(unix)]
#[test]
fn a_run_writes_nothing_into_the_checkout_it_packs() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, context) = checkout(&[("keep.txt", "kept")]);
    let mut closed = fs::metadata(dir.path()).unwrap().permissions();
    let open = closed.clone();
    closed.set_mode(0o555);
    fs::set_permissions(dir.path(), closed).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let packed = pack(
        &context,
        &["keep.txt".to_owned()],
        "none 0123456789abcdef",
        "the run script\n",
        scratch.path(),
    );
    fs::set_permissions(dir.path(), open).unwrap();
    let tarball = packed.unwrap();
    let there = unpacked(&context, &tarball);
    for (name, text) in [
        (SYNCED, "keep.txt\n"),
        (TREE, "none 0123456789abcdef\n"),
        (RUN, "the run script\n"),
    ] {
        assert_eq!(
            fs::read_to_string(there.path().join(name)).unwrap(),
            text,
            "{name}"
        );
    }
    assert_eq!(
        fs::read_to_string(there.path().join(SWEEP)).unwrap(),
        sweep_script()
    );
    assert_eq!(
        fs::read_to_string(there.path().join("keep.txt")).unwrap(),
        "kept"
    );
}

/// Runs of one tree to two machines at once, each with a run script of its own: each tarball
/// holds its own, and the list of what went over is the tree's alone, none of it another run's
/// files.
#[test]
fn two_runs_from_one_tree_at_once_each_send_their_own_run_script_and_the_trees_files_alone() {
    const RUNS: usize = 4;
    const ROUNDS: usize = 8;
    let (_dir, context) = checkout(&[("keep.txt", "kept"), ("src/lib.rs", "fn lib() {}\n")]);
    let ready = std::sync::Barrier::new(RUNS);
    std::thread::scope(|scope| {
        let mut all = Vec::new();
        for run in 0..RUNS {
            let (context, ready) = (&context, &ready);
            all.push(scope.spawn(move || {
                for round in 0..ROUNDS {
                    ready.wait();
                    let scratch = tempfile::tempdir().unwrap();
                    let script = format!("run {run} round {round}\n");
                    let listed = files(context).unwrap();
                    let tarball = pack(context, &listed, "none x", &script, scratch.path())
                        .unwrap_or_else(|error| panic!("run {run} round {round}: {error}"));
                    let there = unpacked(context, &tarball);
                    let held = |name: &str| fs::read_to_string(there.path().join(name)).unwrap();
                    assert_eq!(held(RUN), script, "run {run} round {round}");
                    assert_eq!(
                        sorted(held(SYNCED).lines().map(str::to_owned).collect()),
                        ["keep.txt", "src/lib.rs"],
                        "run {run} round {round}"
                    );
                }
            }));
        }
        for one in all {
            one.join().unwrap();
        }
    });
}
