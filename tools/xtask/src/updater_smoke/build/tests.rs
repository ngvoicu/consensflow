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
    // Every build is of the smoke's identity, never of the live app's.
    assert_eq!(
        override_config("KEY", None),
        json!({ "identifier": "dev.ngvoicu.consensflow.smoke", "plugins": { "updater": { "pubkey": "KEY" } } })
    );
    let with_version = override_config("KEY", Some("3.0.0-alpha.82"));
    assert_eq!(
        with_version,
        json!({ "identifier": "dev.ngvoicu.consensflow.smoke", "plugins": { "updater": { "pubkey": "KEY" } }, "version": "3.0.0-alpha.82" })
    );
    // Where it is written, the version is the last of its keys.
    assert_eq!(
        with_version.to_string(),
        r#"{"identifier":"dev.ngvoicu.consensflow.smoke","plugins":{"updater":{"pubkey":"KEY"}},"version":"3.0.0-alpha.82"}"#
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

    /// An `npm` that writes what it was asked, and where, into `calls`, and ends with
    /// `status`; where it ends well it leaves the folder's `node_modules`, as `npm ci` does,
    /// with a Tauri CLI that says its version.
    fn npm_in(folder: &Path, status: i32) -> (Env, PathBuf) {
        npm_with_cli(folder, status, "echo tauri-cli 2.11.4")
    }

    /// `npm_in`, its Tauri CLI the script `cli`.
    fn npm_with_cli(folder: &Path, status: i32, cli: &str) -> (Env, PathBuf) {
        let calls = folder.join("npm-calls");
        program(
            &folder.join("programs"),
            "npm",
            &format!(
                "echo \"npm $* in $(pwd)\" >> '{}'\n[ {status} -eq 0 ] || exit {status}\n\
                 mkdir -p node_modules/own node_modules/.bin\n\
                 printf '%s\\n' '#!/bin/sh' '{cli}' > node_modules/.bin/tauri\n\
                 chmod +x node_modules/.bin/tauri",
                calls.display()
            ),
        );
        let process = Env::from_process();
        let mut path = folder.join("programs").into_os_string();
        if let Some(search) = process.search_path() {
            path.push(":");
            path.push(search);
        }
        let env = Env::from_vars(
            process
                .iter()
                .filter(|(name, _)| *name != "PATH")
                .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
                .chain([("PATH".into(), path)]),
        );
        (env, calls)
    }

    /// A repository whose tag `v3.0.0-alpha.82` holds a lock in its root and its app, and
    /// whose own folder holds node modules and the download cache.
    fn repo_with_a_release(repo: &Path) -> Repo<'_> {
        repo_with_a_release_locking(repo, "{}")
    }

    /// `repo_with_a_release`, the app's lock at the tag `app_lock`.
    fn repo_with_a_release_locking<'a>(repo: &'a Path, app_lock: &str) -> Repo<'a> {
        fs::create_dir_all(repo.join("app")).unwrap();
        let git = Repo::new(repo);
        fs::write(repo.join("biome.json"), "{}").unwrap();
        fs::write(repo.join("package-lock.json"), "{}").unwrap();
        fs::write(repo.join("app").join("package.json"), "{\"name\":\"old\"}").unwrap();
        fs::write(repo.join("app").join("package-lock.json"), app_lock).unwrap();
        git.git(&["add", "-A"]);
        git.commit("the release", Some("v3.0.0-alpha.82"));
        fs::write(repo.join("app").join("package.json"), "{\"name\":\"new\"}").unwrap();
        git.git(&["add", "-A"]);
        git.commit("after it", None);
        // The repository's own packages, which the release is not built with, and the
        // download cache, which it is given.
        fs::create_dir_all(repo.join("node_modules").join("a")).unwrap();
        fs::create_dir_all(repo.join("app").join("node_modules").join("b")).unwrap();
        fs::create_dir_all(repo.join("app").join(".cache").join("c")).unwrap();
        git
    }

    #[test]
    fn a_release_is_exported_from_its_tag_once_with_its_own_packages_and_the_repos_downloads() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        let git = repo_with_a_release(&repo);
        let (env, calls) = npm_in(folder.path(), 0);
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
            format!(
                "v3.0.0-alpha.82 {} dev.ngvoicu.consensflow.smoke\n",
                commit_of(&git, "v3.0.0-alpha.82")
            )
        );
        assert!(!PathBuf::from(format!("{}.tar", into.display())).exists());
        // Its packages are its own locks', installed from npm's cache, in each folder that has one.
        let real = fs::canonicalize(&into).unwrap();
        assert_eq!(
            fs::read_to_string(&calls).unwrap(),
            format!(
                "npm ci --offline in {0}\nnpm ci --offline in {0}/app\n",
                real.display()
            )
        );
        for packages in [
            into.join("node_modules"),
            into.join("app").join("node_modules"),
        ] {
            assert!(
                !fs::symlink_metadata(&packages).unwrap().is_symlink()
                    && packages.join("own").is_dir(),
                "{} is the release's own, not the repository's",
                packages.display()
            );
        }
        // The downloads are the repository's, linked.
        assert!(
            fs::symlink_metadata(into.join("app").join(".cache"))
                .unwrap()
                .is_symlink(),
            "a link, not a copy"
        );
        assert!(into.join("app").join(".cache").join("c").is_dir());

        // Once: what a build left in it stays, since a tag does not change, and npm is not
        // asked again.
        fs::write(into.join("built"), "yes").unwrap();
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert!(into.join("built").exists());
        assert_eq!(fs::read_to_string(&calls).unwrap().lines().count(), 2);
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
            format!(
                "v3.0.0-alpha.84 {} dev.ngvoicu.consensflow.smoke\n",
                commit_of(&git, "v3.0.0-alpha.84")
            )
        );
    }

    /// A release checks that the installed app and its update are of the live app's
    /// identity, word for word: its export checks the smoke's, which its build is given,
    /// and nothing else of it changes. An export an older run made, with the check as the
    /// tag has it, is made again.
    #[test]
    fn a_release_is_exported_to_check_for_the_smokes_identity_where_it_checked_the_live_apps() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        let source = repo.join("app").join("src-tauri").join("src");
        fs::create_dir_all(&source).unwrap();
        let check = "fn installed(id: &str) -> bool {\n    \
                     if id != \"dev.ngvoicu.consensflow\" { return false; }\n    \
                     id != \"dev.ngvoicu.consensflow\"\n}\n";
        fs::write(source.join("update_install.rs"), check).unwrap();
        fs::write(
            source.join("lib.rs"),
            "const ID: &str = \"dev.ngvoicu.consensflow\";\n",
        )
        .unwrap();
        let git = Repo::new(&repo);
        git.git(&["add", "-A"]);
        git.commit("the release", Some("v3.0.0-alpha.82"));
        let env = Env::from_process();
        let into = folder.path().join("flip-source");
        let exported = into.join("app").join("src-tauri").join("src");

        // An export from before the smoke had an identity of its own: the tag's tree, marked by its tag.
        fs::create_dir_all(&exported).unwrap();
        fs::write(exported.join("update_install.rs"), check).unwrap();
        fs::write(into.join(MARK), "v3.0.0-alpha.82\n").unwrap();
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert_eq!(
            fs::read_to_string(exported.join("update_install.rs")).unwrap(),
            check.replace(
                "!= \"dev.ngvoicu.consensflow\"",
                "!= \"dev.ngvoicu.consensflow.smoke\""
            )
        );
        assert_eq!(
            fs::read_to_string(exported.join("lib.rs")).unwrap(),
            "const ID: &str = \"dev.ngvoicu.consensflow\";\n",
            "what is not the update's check is the tag's"
        );
        assert_eq!(
            fs::read_to_string(into.join(MARK)).unwrap(),
            format!(
                "v3.0.0-alpha.82 {} dev.ngvoicu.consensflow.smoke\n",
                commit_of(&git, "v3.0.0-alpha.82")
            )
        );
    }

    /// A release that checks the live app's identity by another spelling than the one the
    /// export rewrites (a const) is refused, the line named: its app would refuse every update
    /// of the smoke's for a reason that is not the one a case is about. Its tests' words are its
    /// own, and refuse nothing.
    #[test]
    fn a_release_whose_check_is_left_naming_the_live_apps_identity_is_not_exported() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        let source = repo.join("app").join("src-tauri").join("src");
        fs::create_dir_all(&source).unwrap();
        let check = "const ID: &str = \"dev.ngvoicu.consensflow\";\n\
                     fn installed(id: &str) -> bool {\n    id == ID\n}\n\
                     #[cfg(test)]\nmod tests {\n    const ID: &str = \"dev.ngvoicu.consensflow\";\n}\n";
        fs::write(source.join("update_install.rs"), check).unwrap();
        let git = Repo::new(&repo);
        git.git(&["add", "-A"]);
        git.commit("the release", Some("v3.0.0-alpha.90"));
        let into = folder.path().join("flip-source");
        let said = export_release(&repo, &into, "v3.0.0-alpha.90", &Env::from_process())
            .unwrap_err()
            .to_string();
        assert!(said.contains("update_install.rs:1"), "{said}");
        assert!(said.contains("the live app's identity"), "{said}");
        assert!(
            !into.join(MARK).exists(),
            "an export it refused is not marked as made"
        );
        // Its tests alone name it: exported.
        let tests_only = "fn installed(id: &str) -> bool {\n    id != \"dev.ngvoicu.consensflow\"\n}\n\
                          #[cfg(test)]\nmod tests {\n    const ID: &str = \"dev.ngvoicu.consensflow\";\n}\n";
        fs::write(source.join("update_install.rs"), tests_only).unwrap();
        git.git(&["add", "-A"]);
        git.commit("the next", Some("v3.0.0-alpha.91"));
        export_release(&repo, &into, "v3.0.0-alpha.91", &Env::from_process()).unwrap();
    }

    /// The commit a tag of `git` names.
    fn commit_of(git: &Repo, tag: &str) -> String {
        git.git(&["rev-parse", &format!("{tag}^{{commit}}")])
            .trim()
            .to_string()
    }

    /// An export is made once for the commit its tag named, with packages of its own (T-941):
    /// its mark names the tag and that commit, a tag moved to another commit makes it again, and
    /// so does an export whose packages are a link (as the smoke made them before) or gone.
    #[test]
    fn an_export_is_kept_only_for_its_tags_commit_and_with_its_own_packages() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        let git = repo_with_a_release(&repo);
        let (env, calls) = npm_in(folder.path(), 0);
        let into = folder.path().join("exports").join("flip-source");
        fs::create_dir_all(into.parent().unwrap()).unwrap();
        let npm_runs = || fs::read_to_string(&calls).unwrap().lines().count();

        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        let first = commit_of(&git, "v3.0.0-alpha.82");
        assert_eq!(
            fs::read_to_string(into.join(MARK)).unwrap(),
            format!("v3.0.0-alpha.82 {first} {IDENTITY}\n")
        );
        assert_eq!(npm_runs(), 2);

        // Packages that are a link, not the release's own: made again.
        let packages = into.join("app").join("node_modules");
        fs::remove_dir_all(&packages).unwrap();
        link_dir(&repo.join("app").join("node_modules"), &packages).unwrap();
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert_eq!(npm_runs(), 4);
        assert!(!fs::symlink_metadata(&packages).unwrap().is_symlink());

        // Packages gone: made again.
        fs::remove_dir_all(into.join("node_modules")).unwrap();
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert_eq!(npm_runs(), 6);

        // The tag moved to another commit: made again, from that commit.
        git.git(&["tag", "-f", "v3.0.0-alpha.82", "HEAD"]);
        export_release(&repo, &into, "v3.0.0-alpha.82", &env).unwrap();
        assert_eq!(npm_runs(), 8);
        assert_eq!(
            fs::read_to_string(into.join("app").join("package.json")).unwrap(),
            "{\"name\":\"new\"}"
        );
        let moved = commit_of(&git, "v3.0.0-alpha.82");
        assert_ne!(moved, first);
        assert_eq!(
            fs::read_to_string(into.join(MARK)).unwrap(),
            format!("v3.0.0-alpha.82 {moved} {IDENTITY}\n")
        );
    }

    /// `npm ci --offline` skips an optional package npm's cache lacks and ends well: the
    /// Tauri CLI's native one is optional in every lock. The export names the package this
    /// platform needs and is refused unmarked (T-941); the packages of other platforms are not
    /// asked for.
    #[test]
    fn a_release_whose_platform_package_npm_has_not_kept_is_refused_naming_it() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        let (os, cpu) = npm_platform();
        let native = format!("@tauri-apps/cli-{os}-{cpu}");
        let lock = serde_json::json!({
            "packages": {
                "": {"name": "app"},
                "node_modules/@tauri-apps/cli": {"version": "2.11.4"},
                format!("node_modules/{native}"): {"optional": true, "os": [os], "cpu": [cpu]},
                "node_modules/@tauri-apps/cli-elsewhere": {"optional": true, "os": ["plan9"]},
            }
        });
        repo_with_a_release_locking(&repo, &lock.to_string());
        let into = folder.path().join("exports").join("flip-source");
        fs::create_dir_all(into.parent().unwrap()).unwrap();
        let (env, _) = npm_with_cli(
            folder.path(),
            0,
            "echo Cannot find native binding >&2; exit 1",
        );
        let said = export_release(&repo, &into, "v3.0.0-alpha.82", &env)
            .unwrap_err()
            .to_string();
        assert!(
            said.contains(&native) && !said.contains("cli-elsewhere"),
            "{said}"
        );
        assert!(
            !into.join(MARK).exists(),
            "an export that failed is not kept"
        );
    }

    /// The export's own Tauri CLI must run, or the export is refused unmarked with what the CLI
    /// said, whatever npm thought it installed (T-941).
    #[test]
    fn a_release_whose_own_tauri_cli_does_not_run_is_refused_with_its_words() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        repo_with_a_release(&repo);
        let into = folder.path().join("exports").join("flip-source");
        fs::create_dir_all(into.parent().unwrap()).unwrap();
        let (env, _) = npm_with_cli(
            folder.path(),
            0,
            "echo the binding of this platform is not here >&2; exit 1",
        );
        let said = export_release(&repo, &into, "v3.0.0-alpha.82", &env)
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("the Tauri CLI of the release v3.0.0-alpha.82 does not run")
                && said.contains("the binding of this platform is not here"),
            "{said}"
        );
        assert!(
            !into.join(MARK).exists(),
            "an export that failed is not kept"
        );
    }

    #[test]
    fn a_release_whose_packages_npm_has_not_kept_is_refused_unmarked_and_made_again() {
        let folder = tempfile::tempdir().unwrap();
        let repo = folder.path().join("repo");
        repo_with_a_release(&repo);
        let into = folder.path().join("exports").join("flip-source");
        fs::create_dir_all(into.parent().unwrap()).unwrap();
        let (offline, _) = npm_in(&folder.path().join("offline"), 1);
        let said = export_release(&repo, &into, "v3.0.0-alpha.82", &offline)
            .unwrap_err()
            .to_string();
        assert!(
            said.contains(
                "the npm packages of the release v3.0.0-alpha.82 are not all in npm's cache"
            ) && said.contains("`npm ci` in "),
            "{said}"
        );
        assert!(
            !into.join(MARK).exists(),
            "an export that failed is not kept"
        );
        let (online, calls) = npm_in(&folder.path().join("online"), 0);
        export_release(&repo, &into, "v3.0.0-alpha.82", &online).unwrap();
        assert_eq!(fs::read_to_string(&calls).unwrap().lines().count(), 2);
        assert!(into.join(MARK).exists());
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
        // What a build of the product leaves of its plist and its icon.
        let plist = folder.join("Info.plist");
        fs::write(
            &plist,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
             <key>CFBundleExecutable</key><string>app</string>\
             <key>CFBundleIdentifier</key><string>dev.ngvoicu.consensflow.smoke</string>\
             <key>CFBundleName</key><string>ConsensFlow</string>\
             <key>CFBundleDisplayName</key><string>ConsensFlow</string>\
             <key>CFBundleIconFile</key><string>icon.icns</string>\
             <key>CFBundlePackageType</key><string>APPL</string></dict></plist>",
        )
        .unwrap();
        let contents = built_app(&tree).join("Contents");
        program(
            &tree.join("app").join("node_modules").join(".bin"),
            "tauri",
            &format!(
                "{record}\nmkdir -p '{macos}' '{resources}'\ncat \"$5\" > '{macos}/app'\n\
                 cp '{plist}' '{contents}/Info.plist'\nprintf icon > '{resources}/icon.icns'",
                macos = contents.join("MacOS").display(),
                resources = contents.join("Resources").display(),
                plist = plist.display(),
                contents = contents.display(),
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
        // It goes by the smoke's name and has no icon of the product's, sealed again.
        for field in ["CFBundleName", "CFBundleDisplayName"] {
            assert_eq!(
                crate::updater_smoke::bundle::plist_value(&built, field).unwrap(),
                "ConsensFlow Smoke",
                "{field}"
            );
        }
        assert!(crate::updater_smoke::bundle::plist_value(&built, "CFBundleIconFile").is_err());
        assert!(!built.join("Contents/Resources/icon.icns").exists());
        crate::updater_smoke::bundle::verify_seal(&built, &env).unwrap();
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
            "{\n  \"identifier\": \"dev.ngvoicu.consensflow.smoke\",\n  \"plugins\": {\n    \"updater\": {\n      \"pubkey\": \"RUNKEY\"\n    }\n  },\n  \"version\": \"3.0.0-alpha.83\"\n}\n"
        );
    }

    /// A release exported from its tag is built by the Tauri CLI its own lock installed, held
    /// to the CLI's check of its crates against its packages as the release was: never told a
    /// mismatch is meant, which would build an app no user has (T-930).
    #[test]
    fn a_release_exported_from_its_tag_is_built_held_to_the_clis_version_check() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, calls, env) = checkout_with_programs(folder.path(), 0);
        fs::write(tree.join(MARK), "v3.0.6-alpha.3\n").unwrap();
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
        let calls = fs::read_to_string(&calls).unwrap();
        let tauri = calls
            .lines()
            .find(|line| line.starts_with("tauri "))
            .unwrap();
        assert!(
            tauri.contains(&format!(
                "build --bundles app --config {} in ",
                work.join("tauri-as-is.json").display()
            )),
            "{tauri}"
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

    /// A checkout named to build that checks for the live app's identity is built from
    /// no more: its app would refuse every update of the smoke's identity.
    #[test]
    fn a_checkout_that_checks_for_the_live_apps_identity_is_not_built() {
        let folder = tempfile::tempdir().unwrap();
        let (tree, calls, env) = checkout_with_programs(folder.path(), 0);
        let source = tree.join("app").join("src-tauri").join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("update_install.rs"),
            "if id != \"dev.ngvoicu.consensflow\" {}\n",
        )
        .unwrap();
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
            said.contains("checks for the live app's identity"),
            "{said}"
        );
        assert!(!calls.exists(), "nothing ran");
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
