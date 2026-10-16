//! The specs viewer's door on the bridge, asked as the app asks it: the folder
//! a project names, else `.specs`, whether its specs are on or off; its tree
//! and its files; whether it holds a file, remembered for a moment and looked
//! at once however many ask; an HTML file opened in the browser as a copy of
//! what was read, and nothing else; every look at the folder off the daemon's
//! executor, so many at once and no more; a folder that is not there found
//! empty, not refused; every way out refused in words; and nothing woken, for
//! nothing changed. And the fixture folders the page's tests show
//! (`app/tests/fixtures/specs/`) read as the page's stand-in answers for them,
//! content and all, so what those tests prove of the page holds against what
//! the daemon says. Not held to a Node recording: these operations are this
//! build's own.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;

use super::*;

/// The page on a bridge, and a project opened on its own folder, which holds
/// `files` (paths from the project's folder, and their text); ConsensFlow's
/// home, where the copies for the browser are made; what the opener was asked
/// to open.
struct Asked {
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    folder: tempfile::TempDir,
    home: tempfile::TempDir,
    project: i64,
    opened: Rc<RefCell<Vec<PathBuf>>>,
}

fn asked(specs: Value, files: &[(&str, &str)]) -> Asked {
    asked_with(specs, files, |specs| specs)
}

/// As [`asked`], the viewer's `Specs` (its opener records what it is asked to
/// open) made over by `given`.
fn asked_with(specs: Value, files: &[(&str, &str)], given: impl FnOnce(Specs) -> Specs) -> Asked {
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let recorded = Rc::clone(&opened);
    let opener: Opener = Rc::new(move |_, file| {
        recorded.borrow_mut().push(file);
        Box::pin(async { Ok(()) })
    });
    let home = tempfile::tempdir().unwrap();
    let env = Env::from_vars([("CONSENSFLOW_HOME", home.path())]);
    let rig = rig_with(env, None, given(Specs::opening_with(opener)));
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let file = folder.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let mut opening = json!({
        "directory": folder.path().to_str().unwrap(),
        "name": "app",
        "chief": { "harness": "claude-code", "agent": "apollo" },
        "staff": [],
    });
    if !specs.is_null() {
        opening["specs"] = specs;
    }
    let project = rig.kit.open_project(opening).unwrap().unwrap().id;
    Asked {
        _daemon: daemon,
        app,
        rig,
        folder,
        home,
        project,
        opened,
    }
}

impl Asked {
    async fn tree(&self) -> Value {
        ask(&self.app, "specs.tree", json!({ "project": self.project })).await
    }

    async fn file(&self, path: &str) -> Value {
        self.asked("specs.file", path).await
    }

    async fn open(&self, path: &str) -> Value {
        self.asked("specs.open", path).await
    }

    async fn present(&self) -> Value {
        ask(
            &self.app,
            "specs.present",
            json!({ "project": self.project }),
        )
        .await
    }

    async fn asked(&self, operation: &str, path: &str) -> Value {
        ask(
            &self.app,
            operation,
            json!({ "project": self.project, "path": path }),
        )
        .await
    }

    fn specs(&self) -> PathBuf {
        self.folder.path().join(".specs")
    }
}

#[tokio::test]
async fn the_tree_of_dot_specs_and_a_file_of_it_are_read_and_nothing_is_woken() {
    LocalSet::new()
        .run_until(async {
            let held = asked(
                Value::Null,
                &[
                    (".specs/registry.md", "# Spec Registry\n"),
                    (".specs/user-auth/SPEC.md", "# Auth\n"),
                    (".specs/user-auth/.DS_Store", "\0"),
                    ("docs/plans/registry.md", "elsewhere"),
                ],
            );
            assert_eq!(
                held.tree().await,
                json!({ "ok": true, "specs": {
                    "folder": ".specs",
                    "found": true,
                    "entries": [
                        { "name": "registry.md", "path": "registry.md", "size": 16 },
                        { "name": "user-auth", "path": "user-auth", "entries": [
                            { "name": "SPEC.md", "path": "user-auth/SPEC.md", "size": 7 },
                        ] },
                    ],
                } })
            );
            assert_eq!(
                held.file("user-auth/SPEC.md").await,
                json!({ "ok": true, "file": { "path": "user-auth/SPEC.md", "size": 7, "text": "# Auth\n" } })
            );
            // A file chosen again is read again: what changed since is what is shown.
            fs::write(held.specs().join("user-auth/SPEC.md"), "# Auth, again\n").unwrap();
            assert_eq!(
                held.file("user-auth/SPEC.md").await["file"]["text"],
                "# Auth, again\n"
            );
            assert_eq!(held.present().await, json!({ "ok": true, "present": true }));
            assert_eq!(held.rig.kicks.get(), 0, "a read wakes nothing");
        })
        .await;
}

#[tokio::test]
async fn the_folder_a_project_names_is_the_one_read_and_off_or_on_does_not_matter() {
    LocalSet::new()
        .run_until(async {
            let files = [
                (".specs/registry.md", "dot specs"),
                ("docs/plans/registry.md", "the named folder"),
            ];
            let named = asked(json!({ "on": true, "folder": "docs/plans" }), &files);
            assert_eq!(named.tree().await["specs"]["folder"], "docs/plans");
            assert_eq!(
                named.file("registry.md").await["file"]["text"],
                "the named folder"
            );
            // Specs off: the folder is still shown, and it is `.specs` where none is named.
            let off = asked(json!({ "on": false }), &files);
            assert_eq!(off.tree().await["specs"]["folder"], ".specs");
            assert_eq!(off.file("registry.md").await["file"]["text"], "dot specs");
            let unsaid = asked(Value::Null, &files);
            assert_eq!(
                unsaid.file("registry.md").await["file"]["text"],
                "dot specs"
            );
        })
        .await;
}

/// A project that names no folder keeps its specs in the one the app names for
/// every project (Settings → Specs, `specsDefaults` in the agents file), as its
/// chief's launch is told, specs on or off; one that names its own keeps them
/// there; and where the app names none, it is `.specs`.
#[tokio::test]
async fn a_project_that_names_no_folder_is_read_in_the_one_the_app_names_on_or_off() {
    LocalSet::new()
        .run_until(async {
            let files = [
                (".specs/registry.md", "dot specs"),
                ("docs/specs/registry.md", "the app's folder"),
                ("plans/registry.md", "the project's own"),
            ];
            let app_names = |held: &Asked| {
                fs::write(
                    held.home.path().join("agents.json"),
                    r#"{"schemaVersion":1,"agents":[],"specsDefaults":{"folder":"docs/specs"}}"#,
                )
                .unwrap();
            };
            for specs in [Value::Null, json!({ "on": true }), json!({ "on": false })] {
                let held = asked(specs.clone(), &files);
                assert_eq!(held.tree().await["specs"]["folder"], ".specs", "{specs}");
                app_names(&held);
                assert_eq!(
                    held.tree().await["specs"]["folder"],
                    "docs/specs",
                    "{specs}"
                );
                assert_eq!(
                    held.file("registry.md").await["file"]["text"],
                    "the app's folder",
                    "{specs}"
                );
                assert_eq!(held.present().await["present"], true, "{specs}");
            }
            let own = asked(json!({ "on": false, "folder": "plans" }), &files);
            app_names(&own);
            assert_eq!(own.tree().await["specs"]["folder"], "plans");
            assert_eq!(
                own.file("registry.md").await["file"]["text"],
                "the project's own"
            );
        })
        .await;
}

#[tokio::test]
async fn a_folder_that_is_not_there_is_found_empty_and_its_files_are_refused() {
    LocalSet::new()
        .run_until(async {
            let held = asked_with(Value::Null, &[("README.md", "no specs here")], |specs| {
                specs.remembering_for(Duration::ZERO)
            });
            assert_eq!(
                held.tree().await,
                json!({ "ok": true, "specs": { "folder": ".specs", "found": false, "entries": [] } })
            );
            assert_eq!(held.present().await, json!({ "ok": true, "present": false }));
            assert_eq!(
                held.file("registry.md").await,
                json!({ "ok": false, "error": "the project has no specs folder .specs" })
            );
            // The folder made, a folder in it and no file: still nothing to show.
            fs::create_dir_all(held.specs().join("first")).unwrap();
            assert_eq!(held.present().await["present"], false);
            // The chief writes the first spec: the next read finds it.
            fs::write(held.specs().join("first/SPEC.md"), "# First\n").unwrap();
            assert_eq!(held.present().await["present"], true);
            let tree = held.tree().await;
            assert_eq!(tree["specs"]["found"], true);
            assert_eq!(
                tree["specs"]["entries"][0]["entries"][0]["path"],
                "first/SPEC.md"
            );
        })
        .await;
}

#[tokio::test]
async fn every_way_out_of_the_folder_and_every_body_it_cannot_read_is_refused_in_words() {
    LocalSet::new()
        .run_until(async {
            let held = asked(
                Value::Null,
                &[(".specs/a/SPEC.md", "# A"), ("secret.env", "TOKEN=1")],
            );
            for (path, words) in [
                (
                    json!("../secret.env"),
                    "../secret.env climbs out of the specs folder (..)",
                ),
                (
                    json!("a/../../secret.env"),
                    "a/../../secret.env climbs out of the specs folder (..)",
                ),
                (
                    json!("/etc/hosts"),
                    "/etc/hosts is absolute: a path in the specs folder is relative to it",
                ),
                (json!("a"), "a is a folder, not a file"),
                (json!(7), "path names a file of the specs folder, not 7"),
                (
                    Value::Null,
                    "path names a file of the specs folder, not null",
                ),
            ] {
                for operation in ["specs.file", "specs.open"] {
                    let answer = ask(
                        &held.app,
                        operation,
                        json!({ "project": held.project, "path": path }),
                    )
                    .await;
                    assert_eq!(answer, json!({ "ok": false, "error": words }), "{path}");
                }
            }
            for operation in ["specs.tree", "specs.file", "specs.present", "specs.open"] {
                assert_eq!(
                    ask(
                        &held.app,
                        operation,
                        json!({ "project": 99, "path": "a/SPEC.md" })
                    )
                    .await,
                    json!({ "ok": false, "error": "no project 99" })
                );
                assert_eq!(
                    ask(&held.app, operation, json!({ "project": "1" })).await,
                    json!({ "ok": false, "error": "project is a whole number, not 1" })
                );
            }
        })
        .await;
}

#[tokio::test]
async fn a_specs_folder_that_is_a_file_is_refused_and_holds_nothing() {
    LocalSet::new()
        .run_until(async {
            let held = asked(Value::Null, &[(".specs", "not a folder")]);
            let refused =
                json!({ "ok": false, "error": "the specs folder .specs is a file, not a folder" });
            assert_eq!(held.tree().await, refused);
            assert_eq!(held.file("registry.md").await, refused);
            assert_eq!(held.open("a.html").await, refused);
            assert_eq!(
                held.present().await,
                json!({ "ok": true, "present": false })
            );
        })
        .await;
}

/// Where the page's tests keep the fixture folders, beside what the page's
/// stand-in answers for each: `<name>.tree.json` is its `specs.tree`, written by
/// `node app/tests/specs-trees.mjs` when a fixture changes.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/tests/fixtures/specs")
}

const FIXTURES: [&str; 5] = ["core", "core-html", "tdd", "tdd-html", "hostile"];

/// What the daemon answers for the fixture file at `path`, whose bytes are
/// `bytes`, as the page's stand-in answers it (`app/tests/specs-fixtures.mjs`):
/// an image's bytes by its extension, text that is UTF-8 with no NUL, and the
/// rest withheld with its size. Every fixture is under 1 KB but the assets.
fn answered(path: &str, bytes: Vec<u8>) -> Value {
    let size = bytes.len();
    if path.ends_with(".png") || path.ends_with(".svg") {
        let image = base64::engine::general_purpose::STANDARD.encode(&bytes);
        return json!({ "path": path, "size": size, "image": image });
    }
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => json!({ "path": path, "size": size, "text": text }),
        _ => json!({
            "path": path,
            "size": size,
            "withheld": format!("{path} is {size} bytes that is neither text nor an image"),
        }),
    }
}

#[tokio::test]
async fn the_fixture_folders_read_as_the_page_s_stand_in_answers_for_them() {
    LocalSet::new()
        .run_until(async {
            for name in FIXTURES {
                let held = asked(Value::Null, &[]);
                // The fixture is the project's `.specs`.
                copy_folder(&fixtures().join(name), &held.specs());
                let tree = held.tree().await;
                let said: Value = serde_json::from_str(
                    &fs::read_to_string(fixtures().join(format!("{name}.tree.json"))).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    tree["specs"]["entries"], said,
                    "{name}: what the stand-in answers (a fixture changed? node app/tests/specs-trees.mjs)"
                );
                let mut paths = Vec::new();
                files_of(&said, &mut paths);
                assert!(!paths.is_empty(), "{name}");
                for path in paths {
                    let bytes = fs::read(fixtures().join(name).join(&path)).unwrap();
                    assert_eq!(
                        held.file(&path).await["file"],
                        answered(&path, bytes),
                        "{name}/{path}"
                    );
                }
            }
        })
        .await;
}

/// The paths of every file of a tree, as the page asks for them.
fn files_of(entries: &Value, paths: &mut Vec<String>) {
    for entry in entries.as_array().unwrap() {
        match entry.get("entries") {
            Some(inner) => files_of(inner, paths),
            None => paths.push(entry["path"].as_str().unwrap().to_owned()),
        }
    }
}

/// `from` copied whole into `to`, as the chief would have written it there.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

mod looks;
mod opening;
