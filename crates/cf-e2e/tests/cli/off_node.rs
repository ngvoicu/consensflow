//! `Node's sources are gone for good`: nothing imports what the deletion took
//! away, step 4 of the Rust rewrite: Node's daemon, CLI and ledger (`src/`,
//! `bin/cf.mjs`) and the host libraries they shared (`hosts/lib/`, but for the
//! question door the OpenCode plugin loads). What stays runs the native daemon and
//! `cf`, or fixtures of its own.
//!
//! The dangling-import check is the guard of the imports: a script that imports a
//! file that was deleted names a file that is not there, so it fails here, in
//! whatever way the import is written, and not when someone runs the tool. The
//! one hole is a file put back, which an import would then find: the deleted
//! tree is held deleted, by name. What is read at run time, by a path in a
//! string, an import cannot show: a last test reads every file of the tree and
//! finds the deleted paths named, but where a recording or an older release's
//! layout says them on purpose.
//!
//! Each check is a function of a tree, so the last tests plant what the checks
//! are for (an import of a deleted file, a deleted file put back, a path in a
//! string) in a tree of their own, and see each one found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cf_e2e::process::Run;
use cf_e2e::{checkout, files};
use regex::Regex;

use crate::Outcome;

/// What a check finds, or why it could not look.
type Found<T> = Result<T, Box<dyn std::error::Error>>;

/// The folders whose scripts are held: tests, the page's specs and its build, and
/// the host extensions.
const ROOTS: [&str; 4] = ["tests", "app/tests", "app/scripts", "hosts"];

/// What the deletion took away.
const DELETED: [&str; 7] = [
    "src",
    "bin/cf.mjs",
    "hosts/lib/completion",
    "hosts/lib/completion.js",
    "hosts/lib/presets.js",
    "hosts/lib/quota.js",
    "hosts/lib/windows.js",
];

/// The one library of the host extensions that stays.
const DOOR: &str = "question-door.js";

/// This file holds samples of what it looks for, which are not imports.
const SELF: &str = "crates/cf-e2e/tests/cli/off_node.rs";

/// The deleted paths, as a text names them.
const NAMED: &str = r"(?-u:\b)src/(?:core|adapters|channels)/|(?-u:\b)src/[a-z/-]+\.js(?-u:\b)|bin/cf\.mjs|hosts/lib/(?:completion|presets|quota|windows)";

/// Where the deleted paths are named on purpose, and why: data recorded from
/// Node and from the harnesses, fixed since (and the one test that gives the
/// dispatcher the text of a task a recorded trace holds, `src/parse.js`, a file
/// of an imagined project); the layout of the bundles older releases shipped,
/// which the updater and its smoke still meet, and the publisher holds the
/// bridge to; the command an older build wrote, which names a `cf.mjs` and is read
/// and repaired; the plants of this very check, which plant a path of Node's;
/// and the page's bundles of what others wrote (`app/ui/vendor`), which keep
/// their licences, one of which links jQuery's own `src/event.js` (Mermaid's).
const SAYS_THEM: [&str; 11] = [
    r"^crates/[^/]+/tests/(?:goldens|traces)/",
    r"^crates/cf-engine/tests/dispatcher/review\.rs$",
    r"^tests/(?:engine/)?fixtures/",
    r"^tools/cf-publish/",
    r"^app/src-tauri/src/update_install\.rs$",
    r"^tools/xtask/src/updater_smoke/",
    r"^crates/cf-launcher/",
    r"^crates/cf/tests/cli_goldens/",
    r"^tests/plants/cli/admin\.toml$",
    r"^tests/plants/cli/off-node\.toml$",
    r"^app/ui/vendor/",
];

/// Folders no source lives in: what is built, installed or fetched.
const UNSOURCED: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    ".cache",
    "binaries",
    "resources",
];

/// The list of the files the Windows machine's runner sent, at the root of its copy
/// (`cargo xtask windows`: `tools/xtask/src/windows/script.rs`).
const SYNCED: &str = ".synced-files";

/// `relative`, a path written with `/`, under `root`.
fn under(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// `path`, which is under `root`, written from it with `/`.
fn from_root(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every script under the roots of the tree `root`, written from it.
fn scripts(root: &Path) -> Found<Vec<String>> {
    let mut found = Vec::new();
    for dir in ROOTS {
        for file in checkout::files_below(&under(root, dir), &["node_modules"])? {
            let name = from_root(root, &file);
            if [".mjs", ".js", ".cjs"]
                .iter()
                .any(|end| name.ends_with(end))
            {
                found.push(name);
            }
        }
    }
    Ok(found)
}

/// Where the import `path` of the script `file` points, written from the root of
/// the tree: the folder of the script, then the path, with its `.` and `..` read.
fn resolved(file: &str, path: &str) -> String {
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The relative modules the `text` of the script `file` imports, written from the
/// root of the tree: however the import is written (`from`, `import()`, a bare
/// `import`), and only the ones that begin `./` or `../`.
fn imports_in(text: &str, file: &str) -> Found<Vec<String>> {
    let specifier = Regex::new(
        r#"(?:(?-u:\b)from\s*|(?-u:\b)import\s*\(?\s*)(?:'(\.{1,2}/[^']+)'|"(\.{1,2}/[^"]+)")"#,
    )?;
    Ok(specifier
        .captures_iter(text)
        .filter_map(|found| found.get(1).or_else(|| found.get(2)))
        .map(|path| resolved(file, path.as_str()))
        .collect())
}

/// The scripts of the tree `root` that import a file that is not there, each as
/// `script imports module`.
fn dangling(root: &Path) -> Found<Vec<String>> {
    let mut found = Vec::new();
    for file in scripts(root)? {
        for module in imports_in(&files::read_string(&under(root, &file))?, &file)? {
            if !under(root, &module).exists() {
                found.push(format!("{file} imports {module}"));
            }
        }
    }
    Ok(found)
}

/// What the deletion took away that the tree `root` has again, and any host
/// library beside the question door.
fn revived(root: &Path) -> Found<Vec<String>> {
    let mut found = Vec::new();
    for path in DELETED {
        if holds_a_file(&under(root, path))? {
            found.push((*path).to_owned());
        }
    }
    let lib = under(root, "hosts/lib");
    if lib.exists() {
        for file in checkout::files_below(&lib, &[])? {
            let Some(first) = file.strip_prefix(&lib)?.components().next() else {
                continue;
            };
            let name = first.as_os_str().to_string_lossy();
            let beside = format!("hosts/lib/{name}");
            if !name.starts_with('.') && name != DOOR && !found.contains(&beside) {
                found.push(beside);
            }
        }
    }
    Ok(found)
}

/// Whether `path` is a file, or a folder with a file somewhere in it: a folder
/// emptied of its files is no path put back (a copy that is synced file by file,
/// as the Windows machine's is, keeps the folders of the files it took away).
fn holds_a_file(path: &Path) -> Found<bool> {
    Ok(path.is_file() || (path.is_dir() && !checkout::files_below(path, &[])?.is_empty()))
}

/// Where the `listed` files of the tree `root` name a deleted path but where it is
/// said on purpose, each as `file:line`.
fn named(root: &Path, listed: &[String]) -> Found<Vec<String>> {
    let named = Regex::new(NAMED)?;
    let said_on_purpose = SAYS_THEM
        .iter()
        .map(|pattern| Regex::new(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let mut found = Vec::new();
    for file in listed {
        if file == SELF || said_on_purpose.iter().any(|says| says.is_match(file)) {
            continue;
        }
        let bytes = files::read(&under(root, file))?;
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if let Some(line) = text.split('\n').position(|line| named.is_match(line)) {
            found.push(format!("{file}:{}", line + 1));
        }
    }
    Ok(found)
}

/// Every file under `root` but what is built, installed or fetched, written from it.
fn walked(root: &Path) -> Found<Vec<String>> {
    Ok(checkout::files_below(root, &UNSOURCED)?
        .iter()
        .map(|file| from_root(root, file))
        .collect())
}

/// Every file of this repository that git knows or would take, as written on this
/// disk. Where there is no git: the files the Windows machine's runner sent, as its
/// list of them says ([`SYNCED`]: a copy it syncs keeps whatever else was put
/// there, which is none of the repository's), or else (the commit gate tests an
/// exported copy) every file under the root but what is built, installed or
/// fetched ([`walked`]).
fn tree(root: &Path) -> Found<Vec<String>> {
    let listed = Run::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .cwd(root)
        .inheriting_env()
        .run();
    match listed {
        Ok(ran) if ran.code == Some(0) => Ok(ran
            .stdout
            .split('\0')
            .filter(|file| !file.is_empty() && under(root, file).exists())
            .map(str::to_owned)
            .collect()),
        _ => match files::read_string(&under(root, SYNCED)) {
            Ok(synced) => Ok(synced
                .lines()
                .map(str::trim)
                .filter(|file| !file.is_empty() && under(root, file).exists())
                .map(str::to_owned)
                .collect()),
            Err(_) => walked(root),
        },
    }
}

/// The tree of the checkout the suite runs in.
fn repository() -> PathBuf {
    checkout::root()
}

#[test]
fn finds_a_module_however_it_is_imported_and_only_a_relative_one() -> Outcome {
    // What it looks for, shown on text of its own.
    let text = [
        "import { rosterPath } from '../../src/roster.js'",
        "export * from '../../hosts/lib/windows.js'",
        "const { Bridge } = await import('../../src/bridge.js')",
        "import '../../hosts/lib/quota.js'",
        "import { ask } from \"../../hosts/lib/question-door.js\"",
        "import { join } from 'node:path'",
        "import { readRecord } from './helpers/records.mjs'",
    ]
    .join("\n");
    assert_eq!(
        imports_in(&text, "tests/engine/example.test.mjs")?,
        [
            "src/roster.js",
            "hosts/lib/windows.js",
            "src/bridge.js",
            "hosts/lib/quota.js",
            "hosts/lib/question-door.js",
            "tests/engine/helpers/records.mjs",
        ]
    );
    Ok(())
}

#[test]
fn reads_the_scripts_it_is_to_hold() -> Outcome {
    let held = scripts(&repository())?;
    for file in [
        "tests/pi-extension.test.mjs",
        "app/tests/core-page.spec.mjs",
        "app/scripts/bundle-ui.mjs",
        "hosts/opencode-extension/consensflow-session.mjs",
    ] {
        assert!(held.iter().any(|held| held == file), "{file} is read");
    }
    let plugin = "hosts/opencode-extension/consensflow-session.mjs";
    assert_eq!(
        imports_in(&files::read_string(&under(&repository(), plugin))?, plugin)?,
        [format!("hosts/lib/{DOOR}")],
        "and the one library they load"
    );
    Ok(())
}

// A module deleted under a script that still imports it fails there and then, and
// not when someone runs the tool.
#[test]
fn has_every_script_import_files_that_are_there() -> Outcome {
    assert_eq!(dangling(&repository())?, Vec::<String>::new());
    Ok(())
}

#[test]
fn holds_the_deleted_tree_deleted() -> Outcome {
    assert_eq!(
        revived(&repository())?,
        Vec::<String>::new(),
        "Node's sources are gone: use the native side"
    );
    assert!(
        under(&repository(), &format!("hosts/lib/{DOOR}")).exists(),
        "the question door stays"
    );
    Ok(())
}

#[test]
fn finds_a_deleted_path_named_and_not_a_rust_crates_own_or_what_stays() -> Outcome {
    let named = Regex::new(NAMED)?;
    for text in [
        "../src/roster.js",
        "src/core/daemon.js",
        "join(REPO, \"bin/cf.mjs\")",
        "hosts/lib/presets.js",
    ] {
        assert!(named.is_match(text), "{text}");
    }
    for text in [
        "crates/cf-ledger/src/ledger.rs",
        "crates/cf-daemon/src/screens/pages/agents.html",
        "hosts/lib/question-door.js",
        "hosts/pi-extension/consensflow-delivery.mjs",
    ] {
        assert!(!named.is_match(text), "{text}");
    }
    Ok(())
}

// What is read at run time by a path in a string, an import cannot show.
#[test]
fn names_none_of_them_anywhere_but_where_a_recording_or_an_older_release_says_them() -> Outcome {
    let root = repository();
    assert_eq!(
        named(&root, &tree(&root)?)?,
        Vec::<String>::new(),
        "Node's sources are gone: say what they did in words"
    );
    Ok(())
}

mod what_the_checks_are_for_planted_in_a_tree_of_their_own {
    use super::*;

    /// A tree of the shape the checks read (a script in each root, the OpenCode
    /// plugin and the question door it loads) with the `planted` files besides, or
    /// in place of those, on disk: the tree and the files it holds.
    fn on_tree(planted: &[(&str, &str)]) -> Found<(tempfile::TempDir, Vec<String>)> {
        let plugin = format!("import {{ ask }} from '../lib/{DOOR}'\n");
        let example = format!("import {{ ask }} from '../hosts/lib/{DOOR}'\n");
        let mut tree: BTreeMap<String, String> = BTreeMap::from([
            ("tests/example.test.mjs".to_owned(), example),
            ("app/tests/page.spec.mjs".to_owned(), String::new()),
            ("app/scripts/build.mjs".to_owned(), String::new()),
            (
                "hosts/opencode-extension/consensflow-session.mjs".to_owned(),
                plugin,
            ),
            (
                format!("hosts/lib/{DOOR}"),
                "export const ask = () => {}\n".to_owned(),
            ),
        ]);
        for (file, text) in planted {
            tree.insert((*file).to_owned(), (*text).to_owned());
        }
        let root = tempfile::tempdir()?;
        for (file, text) in &tree {
            files::write(&under(root.path(), file), text)?;
        }
        Ok((root, tree.into_keys().collect()))
    }

    #[test]
    fn walks_every_file_but_what_is_built_installed_or_fetched_where_there_is_no_git() -> Outcome {
        let (kept, _) = on_tree(&[])?;
        let wanted = walked(kept.path())?;
        assert_eq!(wanted.len(), 5, "{wanted:?}");
        let (root, _) = on_tree(&[
            ("node_modules/dep/index.js", ""),
            ("app/ui/node_modules/x/index.js", ""),
            ("app/src-tauri/target/debug/out.rs", ""),
            ("bin/.cache/key", ""),
            ("app/src-tauri/binaries/cf", ""),
            ("app/src-tauri/resources/cli", ""),
            (".git/HEAD", ""),
        ])?;
        assert_eq!(walked(root.path())?, wanted);
        Ok(())
    }

    #[test]
    fn finds_nothing_in_a_tree_that_has_none_of_it() -> Outcome {
        let (root, listed) = on_tree(&[])?;
        assert_eq!(dangling(root.path())?, Vec::<String>::new());
        assert_eq!(revived(root.path())?, Vec::<String>::new());
        assert_eq!(named(root.path(), &listed)?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn finds_a_script_that_imports_a_file_the_deletion_took_away() -> Outcome {
        let (root, listed) = on_tree(&[(
            "tests/revived.mjs",
            "import { rosterPath } from '../src/roster.js'\n",
        )])?;
        assert_eq!(
            dangling(root.path())?,
            ["tests/revived.mjs imports src/roster.js"]
        );
        assert_eq!(named(root.path(), &listed)?, ["tests/revived.mjs:1"]);
        Ok(())
    }

    #[test]
    fn finds_the_plugin_of_opencode_importing_a_library_that_is_gone() -> Outcome {
        let plugin = "hosts/opencode-extension/consensflow-session.mjs";
        let (root, _) = on_tree(&[(plugin, "import { quotaOf } from '../lib/quota.js'\n")])?;
        assert_eq!(
            dangling(root.path())?,
            [format!("{plugin} imports hosts/lib/quota.js")]
        );
        Ok(())
    }

    // The hole of the dangling-import check: the file is there again, so an import
    // of it resolves.
    #[test]
    fn a_synced_copy_is_the_files_its_list_names_and_what_else_lies_there_is_none_of_it() -> Outcome
    {
        let (root, listed) = on_tree(&[("paste-chars2.mjs", "import '../src/x.js'\n")])?;
        let sent: Vec<&String> = listed
            .iter()
            .filter(|file| *file != "paste-chars2.mjs")
            .collect();
        let names: Vec<&str> = sent.iter().map(|file| file.as_str()).collect();
        files::write(
            &under(root.path(), SYNCED),
            format!("{}\n", names.join("\n")),
        )?;
        assert_eq!(tree(root.path())?, names);
        Ok(())
    }

    #[test]
    fn a_deleted_folder_emptied_of_its_files_is_not_put_back() -> Outcome {
        let (root, _) = on_tree(&[])?;
        std::fs::create_dir_all(under(root.path(), "src/core"))?;
        assert_eq!(revived(root.path())?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn finds_the_deleted_tree_put_back_even_where_an_import_now_finds_it() -> Outcome {
        let (root, _) = on_tree(&[
            ("tests/revived.mjs", "import '../src/roster.js'\n"),
            ("src/roster.js", ""),
        ])?;
        assert_eq!(dangling(root.path())?, Vec::<String>::new());
        assert_eq!(revived(root.path())?, ["src"]);
        Ok(())
    }

    #[test]
    fn finds_a_host_library_put_back_and_any_other_beside_the_question_door() -> Outcome {
        let (root, _) = on_tree(&[("hosts/lib/presets.js", ""), ("hosts/lib/newcomer.js", "")])?;
        let mut found = revived(root.path())?;
        found.sort();
        assert_eq!(
            found,
            ["hosts/lib/newcomer.js", "hosts/lib/presets.js"]
                .map(str::to_owned)
                .to_vec()
        );
        Ok(())
    }

    #[test]
    fn finds_a_deleted_path_named_in_a_string_where_no_import_shows_it() -> Outcome {
        let (root, listed) = on_tree(&[(
            "tests/revived.mjs",
            "export const cf = join(root, 'bin/cf.mjs')\n",
        )])?;
        assert_eq!(dangling(root.path())?, Vec::<String>::new());
        assert_eq!(named(root.path(), &listed)?, ["tests/revived.mjs:1"]);
        Ok(())
    }

    #[test]
    fn leaves_a_recording_that_says_a_deleted_path_on_purpose_alone() -> Outcome {
        let (root, listed) = on_tree(&[(
            "crates/cf/tests/goldens/cli.json",
            "{\"runs\": \"$REPO/bin/cf.mjs\"}\n",
        )])?;
        assert_eq!(named(root.path(), &listed)?, Vec::<String>::new());
        Ok(())
    }
}
