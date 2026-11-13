//! The rule that the live kit ends a process, closes a window and takes a file away only
//! through its door ([`super::Door`]), read from the kit's Rust as syntax.
//!
//! On 2026-11-12 a live run of a kit branch ended 22 processes that were none of its own, and
//! its review (T-1557) found a rig's start taking other runs' folders and a renewal taking the
//! owner's records by the end of their names. Every ending and every removal of the live kit
//! now asks the door, which acts only on its grounds and, in a say-only run, says instead
//! ([`super::AUDIT`]). This rule reads the live kit's code (`src/live`, `src/rig`, `src/matrix`,
//! `src/bin` (the stand-in programs), `src/process*`, `src/daemon.rs` and the `tests/live_*`
//! suites) and fails on any other way to do either:
//!
//! - a call of `remove_file`, `remove_dir`, `remove_dir_all`, the descent's `end_chosen` /
//!   `pause_chosen` (and their `_by`), `signal`, `stall`, `end_all`, libc's or nix's `kill` and
//!   `killpg`, the descent's `.hold(pid)` of a process to end, or Windows' `TerminateProcess` /
//!   `TerminateJobObject`;
//! - a string that runs a program that ends processes (`kill`, `pkill`, `killall`, `taskkill`,
//!   `tskill`, `Stop-Process`) or asks the pane host to end a pane (`pane.kill`);
//! - `.kill()` in a file that holds a child program itself (names a type `Child`), but on
//!   `self` (a type of the kit's own whose `kill` goes through the door);
//! - a path or an import of the `tempfile` crate: its folders and files are taken away when
//!   they are dropped, past the door and in a say-only run too (the door's own are
//!   [`super::made`] and [`super::made_in`]).
//!
//! A string compared (`==`, `!=`) is read, not run; documentation is not code.
//!
//! The door's own code, the descent's (which ends one process it has read again, for the door)
//! and the process primitives (`src/process/pid.rs`) are what the door is made of, and are not
//! read; nor is code only a unit test builds (`#[cfg(test)]`, and the module files such an
//! attribute gates), which runs on folders and programs it made. What a closure given to the door does is the door's to allow.
//! Macros are read token by token: a call named above, followed by its arguments, is one.
//!
//! What it cannot see, and what the kit's guards do not stand for either (T-1592): a program or a
//! file named in a string that is built (a shell line made from parts, `format!`ed into a
//! command), a call made through a function value, a trait object or another crate's wrapper, an
//! ending or a removal that a destructor does (a dropped terminal hangs its program up, a
//! dropped `tempfile` folder is taken, a dropped child of another crate may be killed), the
//! folders the rule does not read (`src/files.rs`, `tools/xtask`, the product's own crates), and
//! a raw `std::fs` write: the write guard ([`crate::files::guard`]) stands where the kit's own
//! writers ([`crate::files`]) are called, and a raw `std::fs::write` or `File::create` in a
//! suite is none of its business. The kit's tests say what a run did by running it
//! (`tests/rig/layout.rs`); this reads the code for the ways that are plain.

use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{
    Attribute, BinOp, Expr, ExprBinary, ExprCall, ExprMethodCall, ItemFn, ItemMod, ItemUse, LitStr,
    Macro, Meta, UseTree,
};

/// The calls that end, pause or signal a process, or take a file away.
const CALLS: &[&str] = &[
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "end_chosen",
    "end_chosen_by",
    "pause_chosen",
    "pause_chosen_by",
    "signal",
    "killpg",
    "stall",
    "end_all",
    "TerminateProcess",
    "TerminateJobObject",
];

/// The door's methods that are handed what to do, done only where the door allows it.
const DOOR_CLOSURES: &[&str] = &["close_pane", "close_own_terminal", "quit_own_terminal"];

/// Where a string runs a program that ends processes, or asks the pane host to end a pane.
fn ends_in_words(text: &str) -> bool {
    let program = text.rsplit(['/', '\\']).next().unwrap_or(text);
    let program = program.strip_suffix(".exe").unwrap_or(program);
    matches!(
        program,
        "kill" | "pkill" | "killall" | "taskkill" | "tskill"
    ) || [
        "taskkill",
        "pkill ",
        "killall ",
        "Stop-Process",
        "pane.kill",
    ]
    .iter()
    .any(|word| text.contains(word))
        || sends_a_signal(text)
}

/// Whether `text` holds `kill -<signal>` that sends one: `kill -0` only asks whether the process
/// is there, and ends none.
fn sends_a_signal(text: &str) -> bool {
    text.match_indices("kill -").any(|(at, word)| {
        let after = &text[at + word.len()..];
        let mut chars = after.chars();
        let asks_only =
            chars.next() == Some('0') && !chars.next().is_some_and(|c| c.is_ascii_digit());
        !asks_only
    })
}

/// One place the rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    file: String,
    item: String,
    what: String,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.file, self.item, self.what)
    }
}

struct Reader {
    file: String,
    item: Vec<String>,
    /// Whether this file holds a child program itself (names a type `Child`).
    holds_children: bool,
    /// How deep in closures given to the door this is.
    in_door: usize,
    /// How deep in a comparison this is, where a string is read.
    reading: usize,
    found: Vec<Found>,
}

impl Reader {
    fn flag(&mut self, what: String) {
        if self.in_door > 0 {
            return;
        }
        self.found.push(Found {
            file: self.file.clone(),
            item: self.item.last().cloned().unwrap_or_else(|| "file".into()),
            what,
        });
    }

    /// The names called and the strings in `tokens`, a macro's.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        for (at, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    let called = matches!(
                        trees.get(at + 1),
                        Some(TokenTree::Group(next))
                            if next.delimiter() == proc_macro2::Delimiter::Parenthesis
                    );
                    if called && CALLS.contains(&name.as_str()) {
                        self.flag(format!("{name}(…) in a macro"));
                    }
                }
                TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        if ends_in_words(&text.value()) {
                            self.flag(format!("{:?} in a macro", text.value()));
                        }
                    }
                }
                TokenTree::Punct(_) => {}
            }
        }
    }
}

/// Whether `attrs` hold a `#[cfg(…)]` only a test build has: `test`, or `all(test, …)`.
fn for_tests_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let Meta::List(list) = &attr.meta else {
            return false;
        };
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let said = list.tokens.to_string().replace(' ', "");
        said == "test"
            || (said.starts_with("all(") && said[4..].split([',', ')']).any(|part| part == "test"))
    })
}

impl<'ast> Visit<'ast> for Reader {
    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_expr_binary(&mut self, binary: &'ast ExprBinary) {
        if matches!(binary.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            self.reading += 1;
            visit::visit_expr_binary(self, binary);
            self.reading -= 1;
        } else {
            visit::visit_expr_binary(self, binary);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if !for_tests_only(&item.attrs) {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if for_tests_only(&item.attrs) {
            return;
        }
        self.item.push(item.sig.ident.to_string());
        visit::visit_item_fn(self, item);
        self.item.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if for_tests_only(&item.attrs) {
            return;
        }
        self.item.push(item.sig.ident.to_string());
        visit::visit_impl_item_fn(self, item);
        self.item.pop();
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = &*call.func {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let last = segments.last().map(String::as_str).unwrap_or_default();
            let before = segments.iter().rev().nth(1).map(String::as_str);
            // An exit status's own signal is read, not sent.
            let read = last == "signal" && before == Some("ExitStatusExt");
            let libc_kill = last == "kill" && matches!(before, Some("libc" | "signal"));
            if (CALLS.contains(&last) && !read) || libc_kill {
                self.flag(format!("{}(…)", segments.join("::")));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        let on_self = matches!(&*call.receiver, Expr::Path(path) if path.path.is_ident("self"));
        if method == "kill" && call.args.is_empty() && self.holds_children && !on_self {
            self.flag(".kill() in a file that holds a child program".into());
        }
        // The descent's hold of a process, which it ends: the descent's own.
        if method == "hold" && call.args.len() == 1 {
            self.flag(".hold(…) of a process to end".into());
        }
        if DOOR_CLOSURES.contains(&method.as_str()) {
            self.visit_expr(&call.receiver);
            self.in_door += 1;
            for arg in &call.args {
                self.visit_expr(arg);
            }
            self.in_door -= 1;
            return;
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "tempfile")
        {
            let named: Vec<String> = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            self.flag(format!("{} (taken away when dropped)", named.join("::")));
        }
        visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        if for_tests_only(&item.attrs) {
            return;
        }
        if matches!(&item.tree, UseTree::Path(path) if path.ident == "tempfile") {
            self.flag("use tempfile::… (taken away when dropped)".into());
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        if self.reading == 0 && ends_in_words(&literal.value()) {
            self.flag(format!("{:?}", literal.value()));
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        self.tokens(mac.tokens.clone());
    }
}

/// Whether `text` names a type `Child` (std's, or a terminal's child), as a word.
fn holds_children(text: &str) -> bool {
    text.match_indices("Child").any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + "Child".len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// The file a `#[path = "…"]` attribute among `attrs` names.
fn path_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        match &attr.meta {
            Meta::NameValue(value) => match &value.value {
                Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    })
}

/// The module files that `file`'s `#[cfg(test)] mod name;` items gate: `name.rs` and the
/// folder `name`, beside `file` where it is a `mod.rs` or `lib.rs`, and below its own folder
/// otherwise.
fn gated_by(file: &Path, text: &str) -> Vec<PathBuf> {
    let Ok(parsed) = syn::parse_file(text) else {
        return Vec::new();
    };
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let folder = file.parent().unwrap_or(Path::new(""));
    let base = if matches!(stem, "mod" | "lib" | "main") {
        folder.to_path_buf()
    } else {
        folder.join(stem)
    };
    parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.content.is_none() && for_tests_only(&module.attrs) => {
                Some((module.ident.to_string(), path_attribute(&module.attrs)))
            }
            _ => None,
        })
        .flat_map(|(name, path)| match path {
            // `#[path = "x.rs"]`: beside the file that declares it.
            Some(path) => vec![folder.join(path)],
            None => vec![base.join(format!("{name}.rs")), base.join(name)],
        })
        .collect()
}

/// What the rule finds in `text`, the file `name`.
fn read(name: &str, text: &str) -> Vec<Found> {
    let file = syn::parse_file(text).unwrap_or_else(|error| panic!("{name}: {error}"));
    let mut reader = Reader {
        file: name.to_owned(),
        item: Vec::new(),
        holds_children: holds_children(text),
        in_door: 0,
        reading: 0,
        found: Vec::new(),
    };
    reader.visit_file(&file);
    reader.found
}

/// The Rust files at `path`: the file, or every one in the folder and the folders in it.
fn rust_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return if path.extension().is_some_and(|kind| kind == "rs") {
            vec![path.to_owned()]
        } else {
            Vec::new()
        };
    }
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .flat_map(|entry| rust_files(&entry.path()))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// The live kit's files the rule reads, and the root they are named from.
fn live_files() -> (PathBuf, Vec<PathBuf>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    let mut files = Vec::new();
    for part in [
        "live",
        "rig",
        "matrix",
        "bin",
        "process",
        "process.rs",
        "daemon.rs",
    ] {
        files.extend(rust_files(&src.join(part)));
    }
    for entry in fs::read_dir(root.join("tests")).unwrap().flatten() {
        let live = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("live_"));
        if live {
            files.extend(rust_files(&entry.path()));
        }
    }
    // What the door is made of, and what only a test build has.
    let descent = src.join("live").join("members").join("descent");
    let mut gated: Vec<PathBuf> = Vec::new();
    for file in rust_files(&src)
        .iter()
        .chain(rust_files(&root.join("tests")).iter())
    {
        gated.extend(gated_by(
            file,
            &fs::read_to_string(file).unwrap_or_default(),
        ));
    }
    files.retain(|file| {
        let own = file.starts_with(&descent)
            || *file == descent.with_extension("rs")
            || *file == src.join("process").join("pid.rs");
        let unit_test = gated.iter().any(|gate| file.starts_with(gate));
        !own && !unit_test
    });
    (root, files)
}

#[test]
fn the_live_kit_ends_and_removes_only_through_its_door() {
    let (root, files) = live_files();
    for suite in [
        "tests/live_finish/leaving.rs",
        "tests/live_stops/stuck.rs",
        "tests/live_restart_cut.rs",
        "tests/live_parity/claude.rs",
        "src/rig/sweep.rs",
        "src/live/renew.rs",
        "src/live/login.rs",
        "src/process/spawned.rs",
        "src/daemon.rs",
        "src/matrix/run.rs",
        "src/matrix/evidence.rs",
        "src/matrix/scenario/window_life.rs",
        "src/bin/fake_agent.rs",
        "src/bin/fake_agent/tool.rs",
        "tests/live_muse/tui.rs",
    ] {
        assert!(files.contains(&root.join(suite)), "{suite} is read");
    }
    let found: Vec<String> = files
        .iter()
        .flat_map(|file| {
            let name = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            read(&name, &fs::read_to_string(file).unwrap())
        })
        .map(|found| found.to_string())
        .collect();
    assert!(
        found.is_empty(),
        "the live kit ends or removes other than through its door: {found:#?}"
    );
}

#[test]
fn the_rule_finds_each_way_to_end_or_remove_and_not_what_only_reads_or_asks_the_door() {
    let found = |text: &str| read("x.rs", text);
    // What it finds.
    for text in [
        "fn f() { let _ = std::fs::remove_dir_all(&p); }",
        "fn f() { fs::remove_file(p).ok(); }",
        "fn f() { assert!(std::fs::remove_dir(&p).is_ok()); }",
        "fn f() { let _ = signal(pid, Signal::Kill); }",
        "fn f() { crate::process::stall(pid); }",
        "fn f() { end_chosen(&chosen); }",
        "fn f() { unsafe { libc::kill(pid, 9) }; }",
        "fn f() { let _ = nix::sys::signal::kill(pid, Signal::SIGKILL); }",
        "fn f() { let _ = killpg(Pid::from_raw(leader), Signal::SIGKILL); }",
        "fn f() { let _ = nix::sys::signal::killpg(group, Signal::SIGTERM); }",
        "fn f() { let held = ThisMachine.hold(pid)?; }",
        r#"fn f() { Run::new("taskkill").args(["/F"]); }"#,
        r#"fn f() { Run::new("/bin/kill").arg(pid); }"#,
        r#"fn f() { rig.host("pane.kill", pane); }"#,
        r#"fn f() { let s = format!("pkill -f {mark}"); }"#,
        r#"fn f() { let s = "kill -9 $pid"; }"#,
        r#"fn f() { let s = "kill -KILL 4711"; }"#,
        "use std::process::Child;\nfn f(child: &mut Child) { let _ = child.kill(); }",
        "struct S { child: Box<dyn portable_pty::Child> }\nimpl S { fn f(&mut self) { let _ = self.child.kill(); } }",
        r#"fn f() { assert!(std::fs::remove_file(p).is_ok() == true); }"#,
        "fn f() { let folder = tempfile::tempdir().unwrap(); }",
        "struct S { folder: tempfile::TempDir }",
        "use tempfile::TempDir;",
    ] {
        assert_eq!(found(text).len(), 1, "{text}");
    }
    // What it does not.
    for text in [
        "fn f() { let _ = Door::of_run().remove_made(&p); }",
        "use std::process::Child;\nfn f(child: &mut Child) { door.quit_own_terminal(\"t\", || { let _ = child.kill(); }); }",
        "fn f() { window.kill(); }",
        "fn f() { let code = ExitStatusExt::signal(&status); }",
        r#"fn f() { Run::new("ps").args(["-p", &pid]); }"#,
        r#"fn f() { Run::new("killer-app"); }"#,
        "#[cfg(test)]\nmod tests { fn f() { std::fs::remove_dir_all(&p).unwrap(); } }",
        "use std::process::Child;\nfn f(child: &mut Child) { door.close_own_terminal(\"t\", || { let _ = child.kill(); }); }",
        r#"fn f() { door.close_pane(&pane, |op, body| rig.host(op, body)); }"#,
        r#"fn f() { if frame["op"] == "pane.kill" { kills.push(1); } }"#,
        "/// Every `pane.kill` the daemon asked.\nfn f() {}",
        "use std::process::ChildStdout;\nfn f() { window.kill(); }",
        "use std::process::Child;\nimpl S { fn drop(&mut self) { self.kill(); } }",
        "#[cfg(all(test, unix))]\nmod tests { fn f() { std::fs::remove_file(&p).unwrap(); } }",
        "// std::fs::remove_dir_all(&p)\nfn f() {}",
        "fn f() { let folder = crate::door::made(\"x-\").unwrap(); }",
        r#"fn f() { let s = "kill -0 \"$PPID\" 2>/dev/null || exit 0"; }"#,
        "#[cfg(test)]\nmod tests { fn f() { let folder = tempfile::tempdir().unwrap(); } }",
    ] {
        assert_eq!(found(text), Vec::new(), "{text}");
    }
}

#[test]
fn a_module_a_test_cfg_and_a_path_gate_is_the_file_the_path_names() {
    let gated = gated_by(
        Path::new("/s/turn.rs"),
        "#[cfg(test)]\n#[path = \"turn_tests.rs\"]\nmod tests;\n#[cfg(test)]\nmod plain;\nmod live;",
    );
    assert_eq!(
        gated,
        [
            PathBuf::from("/s/turn_tests.rs"),
            PathBuf::from("/s/turn/plain.rs"),
            PathBuf::from("/s/turn/plain"),
        ]
    );
}
