//! The tests of the version check: every way the three files can agree, differ,
//! be missing or be unreadable, and every form a version can take.

use super::*;

use tempfile::TempDir;

const VERSION: &str = "3.0.0-alpha.83";

/// A checkout with the three files, each saying what it is given.
struct Sources {
    dir: TempDir,
}

impl Sources {
    fn new(package: &str, cargo: &str, tauri: &str) -> Self {
        let sources = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        sources.write("package.json", &format!(r#"{{"version": "{package}"}}"#));
        sources.write("Cargo.toml", &Self::manifest(cargo));
        sources.write(
            "app/src-tauri/tauri.conf.json",
            &format!(r#"{{"version": "{tauri}"}}"#),
        );
        sources
    }

    fn agreeing(version: &str) -> Self {
        Self::new(version, version, version)
    }

    fn manifest(version: &str) -> String {
        format!(
            "[workspace]\nmembers = [\"app/src-tauri\"]\n\n\
             [workspace.package]\nversion = \"{version}\"\nedition = \"2021\"\n"
        )
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn remove(&self, file: &str) {
        fs::remove_file(self.dir.path().join(file)).unwrap();
    }

    fn version(&self) -> Result<String, VersionError> {
        source_version(self.dir.path())
    }

    fn error(&self) -> String {
        self.version().unwrap_err().to_string()
    }
}

#[test]
fn three_files_that_say_one_version_have_it() {
    for version in [
        VERSION,
        "1.2.3",
        "0.0.0",
        "10.20.30-rc.1",
        "3.0.0-alpha-1.x.0",
    ] {
        assert_eq!(Sources::agreeing(version).version().as_deref(), Ok(version));
    }
}

#[test]
fn any_file_that_says_another_version_is_refused_with_every_file_named() {
    let disagreement = |package: &str, cargo: &str, tauri: &str| {
        format!(
            "source package, Cargo and Tauri versions do not match: \
             package.json {package}, Cargo.toml {cargo}, tauri.conf.json {tauri}"
        )
    };
    let (a, b, c) = ("3.0.0-alpha.83", "3.0.0-alpha.84", "3.0.1");
    assert_eq!(Sources::new(b, a, a).error(), disagreement(b, a, a));
    assert_eq!(Sources::new(a, b, a).error(), disagreement(a, b, a));
    assert_eq!(Sources::new(a, a, b).error(), disagreement(a, a, b));
    assert_eq!(Sources::new(a, b, c).error(), disagreement(a, b, c));
    // Two that agree with each other do not make it right.
    assert_eq!(Sources::new(a, b, b).error(), disagreement(a, b, b));
}

#[test]
fn a_file_that_is_missing_or_cannot_be_read_ends_it_in_the_scripts_words() {
    let sources = Sources::agreeing(VERSION);
    // The path as the platform writes it: a `/` in it would not match on Windows.
    let path = |file: &str| {
        let whole = file
            .split('/')
            .fold(sources.dir.path().to_path_buf(), |dir, part| dir.join(part));
        whole.display().to_string()
    };

    sources.remove("app/src-tauri/tauri.conf.json");
    let said = sources.error();
    assert!(said.starts_with("could not read Tauri config: "), "{said}");
    assert!(
        said.contains(&path("app/src-tauri/tauri.conf.json")),
        "{said}"
    );

    sources.remove("Cargo.toml");
    let said = sources.error();
    assert!(
        said.starts_with("could not read source Cargo.toml: "),
        "{said}"
    );
    assert!(said.contains(&path("Cargo.toml")), "{said}");

    sources.remove("package.json");
    let said = sources.error();
    assert!(
        said.starts_with("could not read source package.json: "),
        "{said}"
    );
    assert!(said.contains(&path("package.json")), "{said}");
}

#[test]
fn a_folder_where_a_file_should_be_is_a_file_that_cannot_be_read() {
    for (file, label) in [
        ("package.json", "source package.json"),
        ("Cargo.toml", "source Cargo.toml"),
        ("app/src-tauri/tauri.conf.json", "Tauri config"),
    ] {
        let sources = Sources::agreeing(VERSION);
        sources.remove(file);
        fs::create_dir(sources.dir.path().join(file)).unwrap();
        let said = sources.error();
        assert!(
            said.starts_with(&format!("could not read {label}: ")),
            "{said}"
        );
    }
}

#[test]
fn json_that_is_no_json_cannot_be_read_either() {
    for (file, label) in [
        ("package.json", "source package.json"),
        ("app/src-tauri/tauri.conf.json", "Tauri config"),
    ] {
        let sources = Sources::agreeing(VERSION);
        sources.write(file, "{ not json");
        let said = sources.error();
        assert!(
            said.starts_with(&format!("could not read {label}: ")),
            "{said}"
        );
    }
}

#[test]
fn the_cargo_manifest_needs_a_workspace_package_with_a_version() {
    let none = "source Cargo.toml has no workspace package version";
    for manifest in [
        "[workspace]\nmembers = []\n",
        "[package]\nname = \"x\"\nversion = \"3.0.0-alpha.83\"\n",
        "[workspace.package]\nedition = \"2021\"\n",
        "[workspace.package]\nedition = \"2021\"\n\n[workspace.dependencies]\nversion = \"3.0.0-alpha.83\"\n",
        "[workspace.package]\nversion = \"\"\n",
        "[workspace.package]\nversion.workspace = true\n",
        "",
    ] {
        let sources = Sources::agreeing(VERSION);
        sources.write("Cargo.toml", manifest);
        assert_eq!(sources.error(), none, "{manifest:?}");
    }
}

#[test]
fn the_cargo_manifest_is_read_as_the_script_read_it() {
    let found = |manifest: &str| workspace_version(manifest).map(str::to_string);
    let table = "[workspace.package]\n";
    let says = |text: String, version: &str| {
        assert_eq!(found(&text).as_deref(), Some(version), "{text:?}");
    };
    says(format!("{table}version = \"1.2.3\"\n"), "1.2.3");
    says(format!("{table}version=\"1.2.3\""), "1.2.3");
    says(format!("{table}version  =  \"1.2.3\" # now\n"), "1.2.3");
    says(
        format!("{table}edition = \"2021\"\r\nversion = \"1.2.3\"\r\n"),
        "1.2.3",
    );
    says(
        format!("{table}rust-version = \"1.95\"\nversion = \"1.2.3\"\n"),
        "1.2.3",
    );
    // Its pattern could not go past a `[`: an array before the version hides it.
    assert_eq!(
        found(&format!("{table}authors = [\"a\"]\nversion = \"1.2.3\"\n")),
        None
    );
    assert_eq!(
        found("[workspace.package] # [x]\nversion = \"1.2.3\"\n"),
        None
    );
    // The first line of the table is where it starts, not the first mention.
    says(
        "# [workspace.package]\n[workspace.package]\nversion = \"1\"\n".into(),
        "1",
    );
}

#[test]
fn a_version_that_is_not_text_is_not_a_semantic_version() {
    for (file, label) in [
        ("package.json", "source package.json version"),
        (
            "app/src-tauri/tauri.conf.json",
            "source tauri.conf.json version",
        ),
    ] {
        for json in [
            r#"{"version": 3}"#,
            r#"{"version": null}"#,
            "{}",
            "[]",
            "null",
        ] {
            let sources = Sources::agreeing(VERSION);
            sources.write(file, json);
            assert_eq!(
                sources.error(),
                format!("{label} must be a semantic version"),
                "{json}"
            );
        }
    }
}

#[test]
fn each_file_is_held_to_the_canonical_form_under_its_own_name() {
    let bad = "1.0.0+build.5";
    let said = |label: &str| format!("{label} is not a canonical semantic version: {bad}");
    assert_eq!(
        Sources::new(bad, VERSION, VERSION).error(),
        said("source package.json version")
    );
    assert_eq!(
        Sources::new(VERSION, bad, VERSION).error(),
        said("source Cargo.toml version")
    );
    assert_eq!(
        Sources::new(VERSION, VERSION, bad).error(),
        said("source tauri.conf.json version")
    );
    // Agreeing is not enough: all three can say the same wrong thing.
    assert_eq!(
        Sources::agreeing(bad).error(),
        said("source package.json version")
    );
}

#[test]
fn what_is_not_canonical_is_named_and_what_is_canonical_is_not() {
    for bad in [
        "1",
        "1.0",
        "1.0.0.0",
        "01.0.0",
        "1.00.0",
        "1.0.01",
        "v1.0.0",
        " 1.0.0",
        "1.0.0 ",
        "1.0.0\n",
        "1.0.0+1",
        "1.0.0-alpha+1",
        "1.0.0-",
        "1.0.0-alpha.",
        "1.0.0-.alpha",
        "1.0.0-alpha..1",
        "1.0.0-al_pha",
        "1.0.0-álpha",
        "-1.0.0",
        "1.-1.0",
        "a.b.c",
        "",
    ] {
        let sources = Sources::agreeing(VERSION);
        sources.write("package.json", &format!(r#"{{"version": {bad:?}}}"#));
        assert_eq!(
            sources.error(),
            format!("source package.json version is not a canonical semantic version: {bad}"),
            "{bad:?}"
        );
    }
    for good in [
        "0.0.0",
        "1.0.0-0",
        "1.0.0-0alpha",
        "1.0.0-alpha-01",
        "1.0.0--",
        "9.9.9-x.10.y",
    ] {
        assert_eq!(
            Sources::agreeing(good).version().as_deref(),
            Ok(good),
            "{good}"
        );
    }
}

#[test]
fn a_prerelease_number_with_a_leading_zero_is_refused() {
    for bad in [
        "1.0.0-alpha.01",
        "1.0.0-00",
        "1.0.0-01.alpha",
        "1.0.0-a.b.007",
    ] {
        let sources = Sources::agreeing(bad);
        assert_eq!(
            sources.error(),
            "source package.json version has a leading-zero prerelease identifier",
            "{bad}"
        );
    }
}

#[test]
fn this_checkouts_own_sources_agree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    // Every crate takes its version from the workspace, so this crate's is the Cargo.toml's.
    assert_eq!(
        source_version(root).as_deref(),
        Ok(env!("CARGO_PKG_VERSION"))
    );
}

/// Runs `cf-release version <args>`: its status, what it printed, what it said.
fn ask(args: &[&str]) -> (u8, String, String) {
    let mut line = vec![OsString::from("version")];
    line.extend(args.iter().map(OsString::from));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = crate::cli::run(&Env::default(), &line, &mut out, &mut err);
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn the_command_prints_the_version_of_the_checkout_it_is_given() {
    let sources = Sources::agreeing(VERSION);
    let repo = sources.dir.path().to_str().unwrap();
    assert_eq!(
        ask(&["--repo", repo]),
        (0, format!("{VERSION}\n"), String::new())
    );
}

#[test]
fn the_command_without_a_checkout_asks_the_folder_it_is_run_in() {
    // The tests run in this crate's folder, which is no checkout's root.
    let (status, out, err) = ask(&[]);
    assert_eq!((status, out.as_str()), (1, ""));
    assert!(
        err.starts_with("cf-release version: could not read source package.json: "),
        "{err}"
    );
}

#[test]
fn the_command_tells_why_the_sources_disagree_and_fails() {
    let sources = Sources::new("3.0.0", "3.0.1", "3.0.0");
    let repo = sources.dir.path().to_str().unwrap();
    let (status, out, err) = ask(&["--repo", repo]);
    assert_eq!((status, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "cf-release version: source package, Cargo and Tauri versions do not match: \
         package.json 3.0.0, Cargo.toml 3.0.1, tauri.conf.json 3.0.0\n"
    );
}

#[test]
fn the_command_refuses_arguments_in_the_words_of_the_script_it_ports() {
    for (args, said) in [
        (vec!["--bundle", "x"], "unknown argument: --bundle"),
        (vec!["x"], "unknown argument: x"),
        (vec!["--repo"], "--repo needs a value"),
        (vec!["--repo", "--repo"], "--repo needs a value"),
        (
            vec!["--repo", "a", "--repo", "b"],
            "duplicate argument: --repo",
        ),
    ] {
        let (status, out, err) = ask(&args);
        assert_eq!((status, out.as_str()), (2, ""), "{args:?}");
        assert!(
            err.starts_with(&format!("cf-release version: {said}\n")),
            "{args:?}: {err}"
        );
    }
}
