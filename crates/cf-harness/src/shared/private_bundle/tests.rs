//! A bundle published as `tests/opencode-install.test.mjs` and
//! `tests/pi-install.test.mjs` hold Node's, and as Node's hash names its
//! folder.

use std::fs;

use super::*;

/// What a bundle that generates nothing generates.
fn nothing(_: &str) -> Vec<(String, Vec<u8>)> {
    Vec::new()
}

const PAIR: [File; 2] = [("a.txt", b"hello"), ("b/c.txt", "w\u{f6}rld".as_bytes())];

/// A home of its own, ConsensFlow's folder in it, and where `pi`'s bundles go.
fn home() -> (tempfile::TempDir, Env, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let app = path::join(&[&dir.path().to_string_lossy(), "consensflow"]);
    let root = PathBuf::from(path::join(&[&app, "extensions", "pi"]));
    (dir, Env::from_vars([("CONSENSFLOW_HOME", app)]), root)
}

fn names(folder: &Path) -> Vec<String> {
    let mut left: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    left
}

#[test]
fn a_folder_is_named_by_the_hash_of_its_files_as_node_names_it() {
    // Probed on Node v26.8.1: `createHash('sha256')` over each file's name,
    // a NUL byte, its bytes and a NUL byte.
    assert_eq!(
        hash_of(PAIR.iter().copied()),
        "5cfc3e5103e61c8d0fa6744ba24e30860415073df945fab666e05aa440b927ba"
    );
    assert_eq!(
        hash_of([].into_iter()),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hash_of([("x", &b""[..])].into_iter()),
        "758d56805faf3cd54dc7a7995808137ca3ade13246ac045c28e3988fbd95900c"
    );
    let generated: Vec<File> = vec![("a.txt", b"hello"), ("g.json", b"/home/me/extensions/pi")];
    assert_eq!(
        hash_of(generated.into_iter()),
        "25423d4177fadd85b8bcb4c751aee080a4d2d902035f9a8c682f18a884045238"
    );
}

#[test]
fn a_bundle_is_published_whole_in_the_folder_its_hash_names() {
    let (_dir, env, root) = home();
    let published = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap();
    let hash = "5cfc3e5103e61c8d0fa6744ba24e30860415073df945fab666e05aa440b927ba";
    assert_eq!(
        published,
        path::join(&[&root.to_string_lossy(), hash]),
        "extensions/<kind>/<hash> in ConsensFlow's folder"
    );
    assert_eq!(
        fs::read(path::join(&[&published, "a.txt"])).unwrap(),
        b"hello"
    );
    assert_eq!(
        fs::read(path::join(&[&published, "b", "c.txt"])).unwrap(),
        "w\u{f6}rld".as_bytes()
    );
    assert_eq!(names(&root), [hash], "no temporary left beside it");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &str| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&published), 0o700);
        assert_eq!(mode(&path::join(&[&published, "b"])), 0o700);
        assert_eq!(mode(&path::join(&[&published, "b", "c.txt"])), 0o600);
        assert_eq!(mode(&root.to_string_lossy()), 0o700);
    }
}

#[test]
fn a_second_preparation_reuses_the_folder_and_makes_nothing() {
    let (_dir, env, root) = home();
    let first = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap();
    let file = path::join(&[&first, "a.txt"]);
    let before = cf_base::file::identity(&fs::File::open(&file).unwrap()).unwrap();
    let second = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap();
    assert_eq!(second, first);
    assert_eq!(names(&root).len(), 1);
    let after = cf_base::file::identity(&fs::File::open(&file).unwrap()).unwrap();
    assert_eq!(before, after, "the same file, never written again");
}

#[test]
fn a_file_that_differs_from_this_build_is_refused_and_every_file_preserved() {
    let (_dir, env, _root) = home();
    let published = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap();
    let file = path::join(&[&published, "a.txt"]);
    fs::write(&file, "damaged").unwrap();
    assert_eq!(
        prepare_private_integration(&env, "pi", &PAIR, &nothing),
        Err(
            "Private pi integration differs from this build; existing files were preserved"
                .to_owned()
        )
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "damaged");
    let other = prepare_private_integration(&env, "opencode", &PAIR, &nothing).unwrap();
    assert!(other.contains("opencode"), "each kind has its folder");
}

#[test]
fn a_file_that_is_gone_is_a_failure_in_node_s_words() {
    let (_dir, env, _root) = home();
    let published = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap();
    let file = path::join(&[&published, "b", "c.txt"]);
    fs::remove_file(&file).unwrap();
    assert_eq!(
        prepare_private_integration(&env, "pi", &PAIR, &nothing),
        Err(format!("ENOENT: no such file or directory, open '{file}'"))
    );
}

#[test]
fn a_folder_that_cannot_be_made_is_said_in_node_s_words() {
    let (dir, env, root) = home();
    let app = dir.path().join("consensflow");
    fs::create_dir(&app).unwrap();
    // A file where `extensions` should be.
    fs::write(app.join("extensions"), "cannot create directory here").unwrap();
    let said = prepare_private_integration(&env, "pi", &PAIR, &nothing).unwrap_err();
    // Probed on Node v26.8.1: `fs.mkdirSync(path, { recursive: true })`
    // names the folder asked for.
    #[cfg(unix)]
    assert_eq!(
        said,
        format!("ENOTDIR: not a directory, mkdir '{}'", root.display())
    );
    #[cfg(windows)]
    assert!(
        said.contains(&format!("mkdir '{}'", root.display())),
        "{said}"
    );
}

#[cfg(unix)]
#[test]
fn a_folder_the_system_refuses_to_make_is_named_by_the_folder_asked_for() {
    use std::os::unix::fs::PermissionsExt;
    // The synchronous `mkdir` names the folder asked for, where the promised
    // one names the level that failed: Probed on Node v26.8.1.
    let (dir, env, root) = home();
    let app = dir.path().join("consensflow");
    fs::create_dir(&app).unwrap();
    fs::set_permissions(&app, fs::Permissions::from_mode(0o555)).unwrap();
    let said = prepare_private_integration(&env, "pi", &PAIR, &nothing);
    fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
    // A user the mask does not bind (root) makes the folder, and is refused nothing.
    if !root.exists() {
        assert_eq!(
            said,
            Err(format!(
                "EACCES: permission denied, mkdir '{}'",
                root.display()
            ))
        );
    }
}

#[test]
fn a_bundle_that_fails_halfway_leaves_no_folder_of_its_own() {
    let (_dir, env, root) = home();
    // `a` is a file, and then a folder too.
    let files: [File; 2] = [("a", b"x"), ("a/b", b"y")];
    let said = prepare_private_integration(&env, "pi", &files, &nothing).unwrap_err();
    assert!(
        said.starts_with("EEXIST: file already exists, mkdir '")
            && said.ends_with(&format!("{}a'", std::path::MAIN_SEPARATOR)),
        "{said}"
    );
    assert!(
        names(&root).is_empty(),
        "neither the temporary nor a bundle"
    );
}

#[test]
fn another_process_that_published_first_is_no_failure() {
    let dir = tempfile::tempdir().unwrap();
    let made = dir.path().join("made");
    let destination = dir.path().join("destination");
    fs::create_dir(&made).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("file"), "published").unwrap();
    publish(&made, &destination).unwrap();
    assert!(made.exists(), "left for the caller to remove");
    let missing = dir.path().join("missing");
    assert_eq!(
        publish(&missing, &dir.path().join("elsewhere"))
            .unwrap_err()
            .to_string(),
        format!(
            "ENOENT: no such file or directory, rename '{}' -> '{}'",
            missing.display(),
            dir.path().join("elsewhere").display()
        )
    );
}

#[test]
fn a_generated_file_is_hashed_as_it_reads_in_the_parent_and_written_as_it_reads_in_the_folder() {
    let (_dir, env, root) = home();
    let names_the_folder = |folder: &str| vec![("g.json".to_owned(), folder.as_bytes().to_vec())];
    let published = prepare_private_integration(&env, "pi", &PAIR[..1], &names_the_folder).unwrap();
    assert_eq!(
        fs::read_to_string(path::join(&[&published, "g.json"])).unwrap(),
        published,
        "written with the folder it is in"
    );
    let root = root.to_string_lossy().into_owned();
    let hashed = [PAIR[0], ("g.json", root.as_bytes())];
    assert_eq!(
        published,
        path::join(&[&root, &hash_of(hashed.into_iter())]),
        "hashed with the folder above"
    );
    assert_eq!(
        prepare_private_integration(&env, "pi", &PAIR[..1], &names_the_folder),
        Ok(published.clone()),
        "a second preparation finds it as it was written"
    );
    // A build that writes it differently is a new bundle, and the old stays.
    let differently = |folder: &str| vec![("g.json".to_owned(), format!("{folder}!").into_bytes())];
    let other = prepare_private_integration(&env, "pi", &PAIR[..1], &differently).unwrap();
    assert_ne!(other, published);
    assert_eq!(names(Path::new(&root)).len(), 2);
}

#[test]
fn an_environment_that_names_no_home_has_no_folder_to_publish_in() {
    assert_eq!(
        prepare_private_integration(&Env::default(), "pi", &PAIR, &nothing),
        Err("missing home in env".to_owned())
    );
}
