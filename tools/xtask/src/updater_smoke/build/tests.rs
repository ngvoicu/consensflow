use super::*;

use crate::context::Context;

/// The checkout these tests are of.
fn checkout() -> PathBuf {
    Context::new(&Env::default()).unwrap().root
}

#[test]
fn the_releases_the_update_goes_to_are_the_bridge_whose_daemon_and_setup_are_nodes_and_the_flip_whose_are_the_native_cfs(
) {
    assert_eq!(Release::ALL, [Release::Bridge, Release::Flip]);
    assert_eq!(
        Release::ALL.map(|release| (release.name(), release.daemon(), release.setup())),
        [
            ("bridge", Kind::Node, Kind::Node),
            ("flip", Kind::Native, Kind::Native)
        ]
    );
    assert_eq!(Release::from_name("bridge"), Some(Release::Bridge));
    assert_eq!(Release::from_name("flip"), Some(Release::Flip));
    assert_eq!(Release::from_name("Flip"), None);
    assert_eq!(Release::from_name(""), None);
    assert_eq!(BRIDGE_TAG, "v3.0.0-alpha.81");
}

#[test]
fn a_build_is_given_the_override_of_the_runs_public_key_and_of_the_version_where_it_is_the_update()
{
    assert_eq!(
        override_config("KEY", None),
        json!({ "plugins": { "updater": { "pubkey": "KEY" } } })
    );
    let with_version = override_config("KEY", Some("3.0.0-alpha.82"));
    assert_eq!(
        with_version,
        json!({ "plugins": { "updater": { "pubkey": "KEY" } }, "version": "3.0.0-alpha.82" })
    );
    // Where it is written, the version is the last of its keys.
    assert_eq!(
        with_version.to_string(),
        r#"{"plugins":{"updater":{"pubkey":"KEY"}},"version":"3.0.0-alpha.82"}"#
    );
}

#[test]
fn a_build_is_held_to_its_key_the_executable_has_the_runs_and_not_the_products() {
    let folder = tempfile::tempdir().unwrap();
    let app = folder.path().join("ConsensFlow.app");
    fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
    let executable = app.join("Contents").join("MacOS").join("app");
    let check = || assert_built_with(&app, "RUNKEY", "PRODUCTKEY", "the app");
    fs::write(&executable, "binary RUNKEY binary").unwrap();
    check().unwrap();
    fs::write(&executable, "binary PRODUCTKEY binary").unwrap();
    assert!(check().unwrap_err().to_string().contains("not built with"));
    fs::write(&executable, "binary RUNKEY PRODUCTKEY binary").unwrap();
    assert!(check()
        .unwrap_err()
        .to_string()
        .contains("carries the product"));
    fs::remove_file(&executable).unwrap();
    assert!(check()
        .unwrap_err()
        .to_string()
        .starts_with("could not read "));
}

#[test]
fn the_product_has_an_updater_key_a_build_must_not_keep() {
    let key = product_key_of(&checkout()).unwrap();
    assert!(!key.is_empty());
    let folder = tempfile::tempdir().unwrap();
    assert!(product_key_of(folder.path()).is_err());
    fs::create_dir_all(folder.path().join("app").join("src-tauri")).unwrap();
    fs::write(
        folder
            .path()
            .join("app")
            .join("src-tauri")
            .join("tauri.conf.json"),
        "{\"plugins\":{}}",
    )
    .unwrap();
    assert!(product_key_of(folder.path())
        .unwrap_err()
        .to_string()
        .contains("names no updater key"));
}

#[test]
fn a_build_leaves_its_bundle_in_the_workspaces_one_build_folder() {
    let expected: PathBuf = [
        "tree",
        "app",
        "src-tauri",
        "target",
        "release",
        "bundle",
        "macos",
        "ConsensFlow.app",
    ]
    .iter()
    .collect();
    assert_eq!(built_app(Path::new("tree")), expected);
}

#[cfg(unix)]
mod with_programs {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A git repository in `folder`, a commit and its tag at a time.
    struct Repo<'a>(&'a Path);

    impl Repo<'_> {
        fn git(&self, args: &[&str]) -> String {
            let ran = process::capture(
                &Invocation::new("git", self.0)
                    .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                    .args(args.iter().copied()),
                &Env::from_process(),
            )
            .unwrap();
            assert_eq!(ran.code, 0, "git {args:?}: {}", ran.stderr);
            ran.stdout
        }

        fn commit(&self, what: &str, tag: Option<&str>) {
            fs::write(self.0.join("f"), what).unwrap();
            self.git(&["add", "f"]);
            self.git(&["commit", "-q", "-m", what]);
            if let Some(tag) = tag {
                self.git(&["tag", tag]);
            }
        }

        fn new(folder: &Path) -> Repo<'_> {
            let repo = Repo(folder);
            repo.git(&["init", "-q"]);
            repo
        }
    }

    #[test]
    fn has_the_flip_among_the_tags_of_this_checkouts_history_but_for_the_one_under_test() {
        let folder = tempfile::tempdir().unwrap();
        let repo = Repo::new(folder.path());
        repo.commit("the release before the bridge", Some("v3.0.0-alpha.80"));
        repo.commit("the bridge", Some("v3.0.0-alpha.81"));
        repo.commit("the flip", Some("v3.0.0-alpha.82"));
        // A tag that is no release of ours is no release.
        repo.git(&["tag", "deploy-1"]);
        repo.commit("the release under test, tagged", Some("v3.0.0-alpha.83"));
        let env = Env::from_process();
        // On its commit, that tag is the release under test and not one it follows.
        let mut earlier = earlier_releases(folder.path(), &env).unwrap();
        earlier.sort();
        assert_eq!(
            earlier,
            ["v3.0.0-alpha.80", "v3.0.0-alpha.81", "v3.0.0-alpha.82"]
        );
        assert_eq!(
            flip_release(folder.path(), &env).unwrap(),
            "v3.0.0-alpha.82"
        );
        // A commit after it, untagged: the tag is behind it, and is the newest release there is.
        repo.commit("after it", None);
        assert_eq!(earlier_releases(folder.path(), &env).unwrap().len(), 4);
        assert_eq!(
            flip_release(folder.path(), &env).unwrap(),
            "v3.0.0-alpha.83"
        );
    }

    #[test]
    fn has_no_flip_where_the_history_has_no_release_after_the_bridge() {
        let folder = tempfile::tempdir().unwrap();
        let repo = Repo::new(folder.path());
        repo.commit("the bridge", Some(BRIDGE_TAG));
        let said = flip_release(folder.path(), &Env::from_process())
            .unwrap_err()
            .to_string();
        assert!(said.contains("name the flip with --flip-ref"), "{said}");
        // Nowhere git is no repository, which is said.
        let nowhere = tempfile::tempdir().unwrap();
        let said = earlier_releases(nowhere.path(), &Env::from_process())
            .unwrap_err()
            .to_string();
        assert!(
            said.starts_with("git tag --points-at HEAD failed: "),
            "{said}"
        );
    }

    #[test]
    fn a_release_is_exported_from_its_tag_once_with_the_caches_of_the_repo_it_is_built_from() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        fs::create_dir_all(repo.join("app")).unwrap();
        let git = Repo::new(&repo);
        fs::write(repo.join("biome.json"), "{}").unwrap();
        fs::write(repo.join("app").join("package.json"), "{\"name\":\"old\"}").unwrap();
        git.git(&["add", "-A"]);
        git.commit("the release", Some("v3.0.0-alpha.82"));
        fs::write(repo.join("app").join("package.json"), "{\"name\":\"new\"}").unwrap();
        git.git(&["add", "-A"]);
        git.commit("after it", None);
        // The caches a build needs and a fetch is not given: two of the three are there.
        fs::create_dir_all(repo.join("node_modules").join("a")).unwrap();
        fs::create_dir_all(repo.join("app").join("node_modules").join("b")).unwrap();
        let env = Env::from_process();
        let into = folder.path().join("exports").join("flip-source");
        fs::create_dir_all(into.parent().unwrap()).unwrap();

        let made = export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert_eq!(made, into);
        assert_eq!(
            fs::read_to_string(into.join("app").join("package.json")).unwrap(),
            "{\"name\":\"old\"}",
            "the tag's tree, not the checkout's"
        );
        assert!(
            !into.join("biome.json").exists(),
            "a second root configuration would stop the linter"
        );
        assert_eq!(
            fs::read_to_string(into.join(MARK)).unwrap(),
            "v3.0.0-alpha.82\n"
        );
        assert!(!PathBuf::from(format!("{}.tar", into.display())).exists());
        assert_eq!(
            fs::canonicalize(into.join("node_modules")).unwrap(),
            fs::canonicalize(repo.join("node_modules")).unwrap()
        );
        assert!(into.join("app").join("node_modules").join("b").is_dir());
        assert!(
            fs::symlink_metadata(into.join("node_modules"))
                .unwrap()
                .is_symlink(),
            "a link, not a copy"
        );
        assert!(
            fs::symlink_metadata(into.join("app").join(".cache")).is_err(),
            "what is not there to give is left out"
        );

        // Once: what a build left in it stays, since a tag does not change.
        fs::write(into.join("built"), "yes").unwrap();
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert!(into.join("built").exists());
        // Another tag is another tree.
        git.git(&["tag", "v3.0.0-alpha.84"]);
        export_release(&repo, &into, "v3.0.0-alpha.84", &env).unwrap();
        assert!(!into.join("built").exists());
        assert_eq!(
            fs::read_to_string(into.join("app").join("package.json")).unwrap(),
            "{\"name\":\"new\"}"
        );
        assert_eq!(
            fs::read_to_string(into.join(MARK)).unwrap(),
            "v3.0.0-alpha.84\n"
        );
    }

    #[test]
    fn a_release_that_is_not_in_the_repository_says_how_to_fetch_it() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        Repo::new(&repo).commit("a commit", None);
        let said = export_release(
            &repo,
            &folder.path().join("out"),
            "v9.9.9",
            &Env::from_process(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            said.starts_with("the release v9.9.9 is not in this repository (git fetch --tags): "),
            "{said}"
        );
    }

    /// A program of `name` in `folder`: a script that is `body`.
    fn program(folder: &Path, name: &str, body: &str) -> PathBuf {
        fs::create_dir_all(folder).unwrap();
        let file = folder.join(name);
        fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        file
    }

    /// A checkout whose `npm` and Tauri CLI are scripts that write what they were given into
    /// `calls`, the CLI leaving a bundle that holds the override it was given.
    fn checkout_with_programs(folder: &Path, status_of_npm: i32) -> (PathBuf, PathBuf, Env) {
        let tree = folder.join("tree");
        let calls = folder.join("calls");
        fs::create_dir_all(tree.join("app").join("src-tauri")).unwrap();
        fs::write(
            tree.join("app").join("src-tauri").join("tauri.conf.json"),
            "{\"plugins\":{\"updater\":{\"pubkey\":\"PRODUCTKEY\"}}}",
        )
        .unwrap();
        let record = format!(
            "echo \"$(basename \"$0\") $* in $(pwd) offline=$CARGO_NET_OFFLINE tauri=${{TAURI_SIGNING_PRIVATE_KEY:-none}} apple=${{APPLE_ID:-none}}\" >> '{}'",
            calls.display()
        );
        program(
            &tree.join("app").join("node_modules").join(".bin"),
            "tauri",
            &format!(
                "{record}\nmkdir -p '{macos}'\ncat \"$5\" > '{macos}/app'",
                macos = built_app(&tree).join("Contents").join("MacOS").display()
            ),
        );
        program(
            &folder.join("programs"),
            "npm",
            &format!("{record}\nexit {status_of_npm}"),
        );
        let env = Env::from_vars([
            (
                "PATH",
                format!("{}:/usr/bin:/bin", folder.join("programs").display()),
            ),
            ("TAURI_SIGNING_PRIVATE_KEY", "production".to_string()),
            ("APPLE_ID", "someone@example.com".to_string()),
        ]);
        (tree, calls, env)
    }

    #[test]
    fn an_app_is_built_with_the_trees_own_scripts_and_cli_the_override_and_a_clean_environment() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, calls, env) = checkout_with_programs(folder.path(), 0);
        let work = folder.path().join("work");
        let built = build_app(
            &Build {
                checkout: &tree,
                work: &work,
                public_key: "RUNKEY",
                version: Some("3.0.0-alpha.83"),
            },
            &env,
        )
        .unwrap();
        assert_eq!(built, built_app(&tree));
        let app = tree.join("app");
        let there = |program: &str, args: &str| {
            format!(
                "{program} {args} in {} offline=true tauri=none apple=none",
                fs::canonicalize(&app).unwrap().display()
            )
        };
        let override_file = work.join("tauri-3.0.0-alpha.83.json");
        assert_eq!(
            fs::read_to_string(&calls)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            [
                there("npm", "run bundle:ui"),
                there("npm", "run prepare-sidecar"),
                there(
                    "tauri",
                    &format!("build --bundles app --config {}", override_file.display())
                ),
            ]
        );
        assert_eq!(
            fs::read_to_string(&override_file).unwrap(),
            "{\n  \"plugins\": {\n    \"updater\": {\n      \"pubkey\": \"RUNKEY\"\n    }\n  },\n  \"version\": \"3.0.0-alpha.83\"\n}\n"
        );
    }

    #[test]
    fn an_app_that_is_not_the_update_is_built_with_no_version_of_its_own_to_give() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, _calls, env) = checkout_with_programs(folder.path(), 0);
        let work = folder.path().join("work");
        build_app(
            &Build {
                checkout: &tree,
                work: &work,
                public_key: "RUNKEY",
                version: None,
            },
            &env,
        )
        .unwrap();
        let text = fs::read_to_string(work.join("tauri-as-is.json")).unwrap();
        assert!(!text.contains("version"), "{text}");
    }

    #[test]
    fn a_step_that_fails_ends_the_build_and_says_which() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, calls, env) = checkout_with_programs(folder.path(), 3);
        let said = build_app(
            &Build {
                checkout: &tree,
                work: &folder.path().join("work"),
                public_key: "RUNKEY",
                version: None,
            },
            &env,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(said, "npm run bundle:ui ended with status 3");
        assert_eq!(
            fs::read_to_string(&calls).unwrap().lines().count(),
            1,
            "nothing after it ran"
        );
    }

    #[test]
    fn a_build_that_does_not_carry_the_runs_key_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, _calls, env) = checkout_with_programs(folder.path(), 0);
        // A configuration that is the product's own, which the CLI is asked to build with.
        program(
            &tree.join("app").join("node_modules").join(".bin"),
            "tauri",
            &format!(
                "mkdir -p '{macos}'\necho PRODUCTKEY > '{macos}/app'",
                macos = built_app(&tree).join("Contents").join("MacOS").display()
            ),
        );
        let said = build_app(
            &Build {
                checkout: &tree,
                work: &folder.path().join("work"),
                public_key: "RUNKEY",
                version: None,
            },
            &env,
        )
        .unwrap_err()
        .to_string();
        assert!(
            said.contains("was not built with this run's updater key"),
            "{said}"
        );
        assert!(said.starts_with("the app built from "), "{said}");
    }
}
