use super::*;

/// A git repository in a folder, where the test commits what it writes.
struct Repo(PathBuf);

impl Repo {
    fn git(&self, words: &[&str]) -> String {
        let ran = process::capture(
            &Invocation::new("git", &self.0)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.com",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(words.iter().copied()),
            &Env::from_process(),
        )
        .unwrap();
        assert_eq!(ran.code, 0, "git {words:?}: {}", ran.stderr);
        ran.stdout
    }

    /// A checkout named `name` under `source`, with these files committed.
    fn new(source: &Path, name: &str, files: &[(&str, &str)]) -> Self {
        let folder = source.join(name);
        fs::create_dir_all(&folder).unwrap();
        let repo = Self(folder);
        repo.git(&["init", "-q"]);
        repo.put(files);
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "a commit"]);
        repo
    }

    fn put(&self, files: &[(&str, &str)]) {
        for (name, text) in files {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }
}

/// The files a skill of this variant ships in a repository, and others it does not.
fn skill_files(variant: &str) -> Vec<(String, String)> {
    let html = variant.ends_with("html");
    let mut files: Vec<(String, String)> = [
        ("SKILL.md", format!("---\nname: specmint-{variant}\n---\n")),
        ("LICENSE", "MIT".to_owned()),
        ("references/spec-format.md", format!("{variant} format")),
        ("references/nested/deep.md", "deep".to_owned()),
        // What a repository has that is no part of the skill.
        ("README.md", "about the repository".to_owned()),
        (
            "skills/specmint/SKILL.md",
            "a copy under a folder".to_owned(),
        ),
        ("tests/run.sh", "echo".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text))
    .collect();
    if html {
        files.push(("assets/spec-styles.css".to_owned(), "body {}".to_owned()));
    }
    files
}

/// Four checkouts under `source`, each committed.
fn checkouts(source: &Path) -> Vec<Repo> {
    VARIANTS
        .iter()
        .map(|variant| {
            let files = skill_files(variant);
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str()))
                .collect();
            Repo::new(source, &format!("specmint-{variant}"), &files)
        })
        .collect()
}

fn context(root: &Path) -> Context {
    Context {
        root: root.to_path_buf(),
        env: Env::from_process(),
    }
}

/// The command run with `source` as the folder of checkouts, in a checkout of
/// `root`: its answer, and what it said on standard error.
fn refresh(root: &Path, source: &Path) -> (Result<i32, Failure>, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    let answered = run(
        &context(root),
        &[source.as_os_str().to_owned()],
        &mut console,
    );
    (answered, String::from_utf8(err).unwrap())
}

/// Every file under `folder`, by its name under it in slashes.
fn files_under(folder: &Path) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    collect_all(folder, folder, &mut found);
    found
}

fn collect_all(base: &Path, folder: &Path, found: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_all(base, &path, found);
        } else {
            let name = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(name, fs::read_to_string(&path).unwrap());
        }
    }
}

#[test]
fn a_refresh_vendors_what_ships_of_each_checkout_and_writes_the_manifest_that_pins_it() {
    let (root, source) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let repos = checkouts(source.path());
    let (answered, said) = refresh(root.path(), source.path());
    assert_eq!(answered.unwrap(), 0, "{said}");
    let vendored = root.path().join("skill").join("specmint");
    for (variant, repo) in VARIANTS.iter().zip(&repos) {
        let found = files_under(&vendored.join(variant));
        let mut expected: Vec<&str> = vec![
            "LICENSE",
            "SKILL.md",
            "references/nested/deep.md",
            "references/spec-format.md",
        ];
        if variant.ends_with("html") {
            expected.insert(2, "assets/spec-styles.css");
        }
        assert_eq!(
            found.keys().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "{variant}: what ships, and none of the repository's other files"
        );
        assert_eq!(found["LICENSE"], "MIT");
        assert!(
            said.contains(&format!(
                "{variant}: {} ({} files)",
                repo.head(),
                expected.len()
            )),
            "{said}"
        );
    }
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vendored.join("manifest.json")).unwrap()).unwrap();
    let names: Vec<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, VARIANTS);
    for (variant, repo) in VARIANTS.iter().zip(&repos) {
        assert_eq!(manifest[variant]["commit"], repo.head());
        let files = manifest[variant]["files"].as_object().unwrap();
        for (name, text) in files_under(&vendored.join(variant)) {
            assert_eq!(files[&name], sha256(text.as_bytes()), "{variant}/{name}");
        }
        assert_eq!(files.len(), files_under(&vendored.join(variant)).len());
    }
}

#[test]
fn a_refresh_made_again_from_the_same_checkouts_leaves_the_same_bytes_and_a_file_a_repository_dropped_is_gone(
) {
    let (root, source) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let repos = checkouts(source.path());
    refresh(root.path(), source.path()).0.unwrap();
    let vendored = root.path().join("skill").join("specmint");
    let first = (
        files_under(&vendored),
        fs::read(vendored.join("manifest.json")).unwrap(),
    );
    refresh(root.path(), source.path()).0.unwrap();
    assert_eq!(
        (
            files_under(&vendored),
            fs::read(vendored.join("manifest.json")).unwrap()
        ),
        first,
        "replayable"
    );
    // The first repository drops a file; its refresh takes it from the copy.
    fs::remove_file(repos[0].0.join("references").join("nested").join("deep.md")).unwrap();
    repos[0].git(&["add", "-A"]);
    repos[0].git(&["commit", "-q", "-m", "drop"]);
    refresh(root.path(), source.path()).0.unwrap();
    assert!(!vendored.join("core/references/nested/deep.md").exists());
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vendored.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["core"]["commit"], repos[0].head());
    assert!(manifest["core"]["files"]
        .get("references/nested/deep.md")
        .is_none());
}

#[test]
fn a_checkout_with_changes_that_are_not_committed_is_refused_and_nothing_vendored_moves() {
    for how in [
        "a tracked file changed",
        "a file the checkout does not track yet",
    ] {
        let (root, source) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let repos = checkouts(source.path());
        let vendored = root.path().join("skill").join("specmint");
        fs::create_dir_all(vendored.join("tdd")).unwrap();
        fs::write(vendored.join("tdd").join("kept.md"), "as it was").unwrap();
        if how.starts_with("a tracked") {
            repos[2].put(&[("SKILL.md", "edited")]);
        } else {
            repos[2].put(&[("notes.md", "new")]);
        }
        let (answered, _) = refresh(root.path(), source.path());
        let Err(Failure::Specmint(Error::Dirty { path, changes })) = answered else {
            panic!("{how}: a dirty checkout is refused");
        };
        assert_eq!(path, repos[2].0, "{how}");
        assert!(!changes.is_empty(), "{how}");
        assert_eq!(
            fs::read_to_string(vendored.join("tdd").join("kept.md")).unwrap(),
            "as it was",
            "{how}: nothing was written"
        );
        assert!(
            !vendored.join("core").exists(),
            "{how}: not even the checkouts read before it"
        );
        assert!(!vendored.join("manifest.json").exists(), "{how}");
    }
}

#[test]
fn a_missing_checkout_one_that_is_no_repository_and_one_with_no_skill_are_each_refused() {
    let root = tempfile::tempdir().unwrap();
    // No checkout at all.
    let source = tempfile::tempdir().unwrap();
    let refused = refresh(root.path(), source.path()).0.unwrap_err();
    assert!(
        matches!(&refused, Failure::Specmint(Error::NoCheckout { variant, .. }) if variant == "core"),
        "{refused}"
    );
    // A folder that is no repository.
    fs::create_dir_all(source.path().join("specmint-core")).unwrap();
    let refused = refresh(root.path(), source.path()).0.unwrap_err();
    assert!(
        matches!(&refused, Failure::Specmint(Error::NotARepository { .. })),
        "{refused}"
    );
    // A repository with no SKILL.md.
    let source = tempfile::tempdir().unwrap();
    let repos = checkouts(source.path());
    repos[1].git(&["rm", "-q", "SKILL.md"]);
    repos[1].git(&["commit", "-q", "-m", "no skill"]);
    let refused = refresh(root.path(), source.path()).0.unwrap_err();
    assert!(
        matches!(&refused, Failure::Specmint(Error::Missing { file, .. }) if file == "SKILL.md"),
        "{refused}"
    );
    assert!(!root.path().join("skill").exists(), "nothing was written");
}

#[test]
fn the_command_takes_the_one_folder_and_no_other_words() {
    let root = tempfile::tempdir().unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };
    for args in [vec![], vec![OsString::from("a"), OsString::from("b")]] {
        let refused = run(&context(root.path()), &args, &mut console).unwrap_err();
        assert!(matches!(refused, Failure::Usage(_)), "{refused}");
    }
    assert!(out.is_empty() && err.is_empty());
    assert!(matches!(COMMANDS[0].run, Run::Native(_)));
}

/// The folder of the skills this checkout vendors.
fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skill")
        .join("specmint")
}

#[test]
fn the_variants_are_the_four_the_committed_manifest_names_in_its_order() {
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(committed().join("manifest.json")).unwrap())
            .unwrap();
    let names: Vec<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, VARIANTS);
}

#[test]
fn the_committed_manifest_is_what_this_command_writes_of_the_folders_it_pins() {
    let manifest_text = fs::read_to_string(committed().join("manifest.json")).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_text).unwrap();
    let taken: Vec<Taken> = VARIANTS
        .iter()
        .map(|&variant| Taken {
            variant,
            commit: manifest[variant]["commit"].as_str().unwrap().to_owned(),
            files: files_under(&committed().join(variant))
                .into_iter()
                .map(|(name, text)| (name, text.into_bytes()))
                .collect(),
        })
        .collect();
    assert_eq!(
        super::manifest(&taken),
        manifest_text,
        "a refresh from the commits the manifest names gives the manifest it has"
    );
}
