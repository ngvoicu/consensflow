//! The extension `cf setup` installs for Pi, held to loading from its own folder
//! alone and never from this checkout. The bundle holds no copy of the extension
//! to load: it is inside the `cf`, which writes it where Pi is told to load it
//! from when it finds a Pi.
//!
//! What a module loads is what it imports, so the extension is read for what it
//! imports: each import is one of Node's own modules (`node:fs`) or a file
//! inside the folder the extension was written to, and what such a file imports
//! is read the same way. Anything else — a package by its bare name, a path that
//! leaves the folder, a URL, a name computed when it runs — would be found
//! somewhere that is not the folder, and is named. The smoke runs no Node (the
//! bundle ships none, and the machine it runs on needs none), so what the
//! runtime would resolve is not asked of it: the source is read, which is what
//! the runtime would resolve it from.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use cf_e2e::{files, pattern, Result};
use regex::Regex;

/// Where the extension's file is, in the folder `cf setup` made for it.
pub fn extension_in(folder: &Path) -> PathBuf {
    ["hosts", "pi-extension", "consensflow-delivery.mjs"]
        .iter()
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// What a module imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Import {
    /// A module by its name, as the source writes it.
    Module(String),
    /// A dynamic import of a name that is worked out when it runs, which no
    /// reading of the source can say.
    Computed,
}

/// What `source` imports, in the order it says so: `import … from`, `export …
/// from`, an import for its effects alone, and `import()` of a name written out.
pub fn imports(source: &str) -> Vec<Import> {
    static NAMED: OnceLock<Regex> = OnceLock::new();
    static COMPUTED: OnceLock<Regex> = OnceLock::new();
    // A word that is the keyword and not a property (`Array.from('x')` is no import).
    let named = pattern::once(
        &NAMED,
        r#"(?:^|[^.\w$])(?:from\s*|import\s*\(?\s*)(?:'([^'\n]*)'|"([^"\n]*)")"#,
    );
    let computed = pattern::once(&COMPUTED, r#"(?:^|[^.\w$])import\s*\(\s*[^'"\s)]"#);
    let mut found: Vec<(usize, Import)> = named
        .captures_iter(source)
        .filter_map(|caught| {
            let whole = caught.get(0)?;
            let name = caught.get(1).or_else(|| caught.get(2))?;
            Some((whole.start(), Import::Module(name.as_str().to_owned())))
        })
        .collect();
    found.extend(
        computed
            .find_iter(source)
            .map(|at| (at.start(), Import::Computed)),
    );
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, import)| import).collect()
}

/// Whether `source` has a default export that is a function, which is what
/// Pi calls to load an extension.
pub fn exports_a_function_by_default(source: &str) -> bool {
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    pattern::once(&DEFAULT, r"(?m)^export default (?:async )?function\b").is_match(source)
}

/// `path` with its `.` and `..` worked out by name, as the folder is not asked.
fn lexical(path: &Path) -> PathBuf {
    let mut worked = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                worked.pop();
            }
            Component::CurDir => {}
            other => worked.push(other.as_os_str()),
        }
    }
    worked
}

/// Where the imports of the module `file` go that are not to Node's own
/// modules or to a file in `root`, each as a sentence; and, for each module
/// that is a file in `root`, the same of its own imports. None for an extension
/// that loads from its folder alone.
pub fn strays(root: &Path, file: &Path) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut strays = Vec::new();
    visit(root, file, &mut seen, &mut strays)?;
    Ok(strays)
}

fn visit(
    root: &Path,
    file: &Path,
    seen: &mut BTreeSet<PathBuf>,
    strays: &mut Vec<String>,
) -> Result {
    if !seen.insert(file.to_path_buf()) {
        return Ok(());
    }
    let name = file
        .strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string();
    for import in imports(&files::read_string(file)?) {
        let Import::Module(specifier) = import else {
            strays.push(format!(
                "{name} imports a module by a name it works out when it runs"
            ));
            continue;
        };
        if specifier.starts_with("node:") {
            continue;
        }
        if !(specifier.starts_with("./") || specifier.starts_with("../")) {
            strays.push(format!(
                "{name} imports {specifier}, which is no module of Node's and no file of its own"
            ));
            continue;
        }
        let target = lexical(&file.parent().unwrap_or(root).join(&specifier));
        if !target.starts_with(root) {
            strays.push(format!(
                "{name} imports {specifier}, which is outside the folder it was written to"
            ));
        } else if !target.is_file() {
            strays.push(format!("{name} imports {specifier}, which is not there"));
        } else {
            visit(root, &target, seen, strays)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(name: &str) -> Import {
        Import::Module(name.to_owned())
    }

    #[test]
    fn what_a_module_imports_is_read_however_the_import_is_written() {
        let source = r#"
import { watch } from 'node:fs'
import {
  access,
  mkdir,
} from "node:fs/promises"
import * as everything from './everything.mjs'
import fallback from '../up.mjs'
import './for-its-effects.mjs'
export { thing } from './thing.mjs'
export * from "./all.mjs"
const later = await import('node:os')
const computed = await import(path.join(here, 'x.mjs'))
const letters = Array.from('abc')
const bytes = Buffer.from("def")
const loaded = await loader.import('plugin')
const name = 'from'
"#;
        assert_eq!(
            imports(source),
            [
                module("node:fs"),
                module("node:fs/promises"),
                module("./everything.mjs"),
                module("../up.mjs"),
                module("./for-its-effects.mjs"),
                module("./thing.mjs"),
                module("./all.mjs"),
                module("node:os"),
                Import::Computed,
            ]
        );
        assert_eq!(imports("const a = 1\n"), Vec::<Import>::new());
    }

    #[test]
    fn a_default_export_is_a_function_when_it_is_declared_as_one() {
        assert!(exports_a_function_by_default(
            "const a = 1\nexport default function consensflowDelivery(pi) {}\n"
        ));
        assert!(exports_a_function_by_default(
            "export default async function (pi) {}\n"
        ));
        for source in [
            "export function named() {}\n",
            "export default class Extension {}\n",
            "export default 42\n",
            "  export default function indented() {}\n",
            "",
        ] {
            assert!(!exports_a_function_by_default(source), "{source}");
        }
    }

    /// A folder written the way `cf setup` writes an extension, with `files`
    /// (names under it, and what they say) in it.
    fn written(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (name, source) in files {
            let path = name
                .split('/')
                .fold(root.path().to_path_buf(), |path, part| path.join(part));
            cf_e2e::files::write(&path, source).unwrap();
        }
        root
    }

    #[test]
    fn an_extension_that_imports_only_node_s_modules_loads_from_its_folder_alone() {
        let root = written(&[(
            "hosts/pi-extension/consensflow-delivery.mjs",
            "import { watch } from 'node:fs'\nimport { join } from 'node:path'\n",
        )]);
        let extension = extension_in(root.path());
        assert!(extension.is_file());
        assert_eq!(
            strays(root.path(), &extension).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_file_of_its_own_is_read_for_what_it_imports_too_and_going_round_is_no_loop() {
        let root = written(&[
            (
                "hosts/pi-extension/consensflow-delivery.mjs",
                "import './a.mjs'\nimport '../lib/b.mjs'\nimport 'node:fs'\n",
            ),
            (
                "hosts/pi-extension/a.mjs",
                "import './consensflow-delivery.mjs'\n",
            ),
            (
                "hosts/lib/b.mjs",
                "import 'ws'\nimport '../../../outside.mjs'\n",
            ),
        ]);
        let extension = extension_in(root.path());
        assert_eq!(
            strays(root.path(), &extension).unwrap(),
            [
                "hosts/lib/b.mjs imports ws, which is no module of Node's and no file of its own",
                "hosts/lib/b.mjs imports ../../../outside.mjs, which is outside the folder it was written to",
            ]
        );
    }

    #[test]
    fn what_is_found_anywhere_but_the_folder_is_named() {
        let source = "\
import '../../../../elsewhere.mjs'
import '/the/checkout/hosts/lib/live.mjs'
import 'file:///the/checkout/hosts/lib/live.mjs'
import 'https://example.test/x.mjs'
import './missing.mjs'
import express from 'express'
await import(pick())
";
        let root = written(&[("hosts/pi-extension/consensflow-delivery.mjs", source)]);
        let own = "hosts/pi-extension/consensflow-delivery.mjs";
        let abroad = "which is no module of Node's and no file of its own";
        assert_eq!(
            strays(root.path(), &extension_in(root.path())).unwrap(),
            [
                format!("{own} imports ../../../../elsewhere.mjs, which is outside the folder it was written to"),
                format!("{own} imports /the/checkout/hosts/lib/live.mjs, {abroad}"),
                format!("{own} imports file:///the/checkout/hosts/lib/live.mjs, {abroad}"),
                format!("{own} imports https://example.test/x.mjs, {abroad}"),
                format!("{own} imports ./missing.mjs, which is not there"),
                format!("{own} imports express, {abroad}"),
                format!("{own} imports a module by a name it works out when it runs"),
            ]
        );
    }
}
