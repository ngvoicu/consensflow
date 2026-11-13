//! The rule that the live kit ends a process, closes a window, takes a file away and writes a file
//! only through its door ([`super::Door`], and the writers of [`crate::door`]), read from the
//! kit's Rust as syntax.
//!
//! On 2026-11-12 a live run of a kit branch ended 22 processes that were none of its own, and
//! its review (T-1557) found a rig's start taking other runs' folders and a renewal taking the
//! owner's records by the end of their names; the reviews after it found writes reaching other
//! runs' files as well (T-1594). Every ending and every removal of the live kit now asks the
//! door, which acts only on its grounds and, in a say-only run, says instead
//! ([`super::AUDIT`]); every write asks one of its writers, which allow a path only below a
//! folder this run made (by its real path, never through a link) or a file it made new
//! ([`super::writable`]). What a caller hands the kit (`--claude-config`, `--fixture`, a
//! folder it names for an export) is read, never written. This rule reads the live kit's code
//! (`src/live`, `src/rig`, `src/matrix`, `src/bin` (the stand-in programs), `src/process*`,
//! `src/daemon.rs` and the `tests/live_*` suites: all of `src` but what the door is made of, and
//! the folders of tests that are none of a live run's) and fails on any other way to do either:
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
//!   [`super::made`] and [`super::made_in`]);
//! - a write that does not ask the door's writers: `std::fs::write`, `copy`, `rename`,
//!   `hard_link`, `create_dir[_all]`, `set_permissions`, a symlink, `File::create[_new]`, an
//!   `OpenOptions` opened for writing, `set_len`, `set_modified` or `set_times`, the plain writers
//!   of [`crate::files`], and a program named in a call that starts one to write or move
//!   (`cp`, `mv`, `tee`, `mkdir`, …), including through an import (`use std::fs::{write, …}`).
//!
//! A string compared (`==`, `!=`) is read, not run; documentation is not code.
//!
//! The door's own code (`src/door*`), `src/files.rs`, the descent's (which ends one process it has
//! read again, for the door) and the process primitives (`src/process/pid.rs`) are what the door is
//! made of, and are not read; nor is code only a unit test builds (`#[cfg(test)]`, and the module
//! files such an attribute gates), which runs on folders and programs it made, nor the builder of
//! the programs a suite starts (`src/cf.rs`: it runs `cargo build` in the checkout and keeps a lock
//! and a stamp in the folder the builds share, which no run of the live kit makes; `cargo xtask
//! live` builds first). What a closure given to the door does is the door's to allow. Macros are
//! read token by token: a call named above, followed by its arguments, is one.
//!
//! What it cannot see, and what the door's guards do not stand for either (T-1592): a program or a
//! file named in a string that is built (a shell line made from parts, `format!`ed into a
//! command), a call made through a function value, a trait object or another crate's wrapper
//! (a write another crate does for the kit), a raw system call, an ending or a removal that a
//! destructor does (a dropped terminal hangs its program up, a dropped `tempfile` folder is
//! taken, a dropped child of another crate may be killed), the folders the rule does not read
//! (`src/files.rs`, `src/cf.rs`, `tools/xtask`, the product's own crates, `crates/cf-evals` (the
//! evals' runner: it runs real harnesses on a home and a Claude config of its own, made with
//! `tempfile` and the plain writers of [`crate::files`], and takes its own folders away; it is no
//! run of the live kit, and its `watch` only reads the folders it is handed), the tests that are no
//! `live_*`: those that start the kit's stand-ins run on folders they make through the door
//! as well, and the programs the run starts, harnesses and `git` among them, write as they
//! write: what they may reach is the isolation's (`live::isolation`, `live::login`) to hold, with
//! the check at the end that the owner's own files are as they were. The door's writers refuse a
//! path outside the run's allocations, so a mistake the syntax rule cannot see is an error at the
//! write, and the kit's tests say what a run did by running it (`tests/rig/layout.rs`); this
//! reads the code for the ways that are plain.

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

/// The functions of `std::fs` (and `std::os::*::fs`) that write, copy, rename, link, make or set
/// something in the file system: what only the door's writers ([`crate::door`]) may do in the
/// kit's code, as they refuse a path that is no part of this run's allocations.
const FS_WRITES: &[&str] = &[
    "write",
    "copy",
    "rename",
    "hard_link",
    "soft_link",
    "create_dir",
    "create_dir_all",
    "set_permissions",
    "symlink",
    "symlink_file",
    "symlink_dir",
];

/// The constructors of `File` and `OpenOptions` that open for writing: an `OpenOptions` is read as
/// one, but for a chain that is made and opened at once and says nothing of writing
/// ([`OPENS_FOR_WRITING`]), which only reads.
const FILE_WRITES: &[&str] = &["create", "create_new", "options"];

/// What an `OpenOptions` chain says to open for writing: without one of these it opens to read.
const OPENS_FOR_WRITING: &[&str] = &["write", "append", "create", "create_new", "truncate"];

/// The plain helpers of [`crate::files`] that write, which do not ask the door
/// (they are for the tests and the tools that are none of a live run); the door's own are
/// `crate::door::write` and its kin.
const PLAIN_WRITES: &[&str] = &[
    "write",
    "copy",
    "make_dir",
    "write_executable",
    "write_whole",
    "say_its_process",
];

/// The methods that change a file or its folder's entry by a handle or a path.
const METHOD_WRITES: &[&str] = &["set_len", "set_modified", "set_times", "set_permissions"];

/// The programs that write, copy, move or make in the file system, named in a call that starts one.
const WRITING_PROGRAMS: &[&str] = &[
    "cp", "mv", "install", "rsync", "tee", "dd", "ln", "touch", "mkdir", "truncate", "ditto",
    "robocopy", "xcopy", "copy", "move", "mklink",
];

/// Whether the path `segments` is one that writes in the file system other than through the door.
fn writes_raw(segments: &[String]) -> bool {
    let last = segments.last().map(String::as_str).unwrap_or_default();
    let before = segments.iter().rev().nth(1).map(String::as_str);
    match before {
        Some("fs") => FS_WRITES.contains(&last),
        Some("File") => FILE_WRITES.contains(&last),
        Some("OpenOptions") => last == "new",
        Some("files") => PLAIN_WRITES.contains(&last),
        _ => false,
    }
}

/// The `OpenOptions::new()` (or `File::options()`) a chain that ends in `.open(…)` begins with, and
/// the names it calls between.
fn open_chain(call: &ExprMethodCall) -> Option<(&ExprCall, Vec<String>)> {
    let mut names = Vec::new();
    let mut at: &Expr = &call.receiver;
    while let Expr::MethodCall(inner) = at {
        names.push(inner.method.to_string());
        at = &inner.receiver;
    }
    let Expr::Call(root) = at else { return None };
    let Expr::Path(path) = &*root.func else {
        return None;
    };
    let named: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|part| part.ident.to_string())
        .collect();
    let tail: Vec<&str> = named.iter().rev().take(2).map(String::as_str).collect();
    matches!(
        tail.as_slice(),
        ["new", "OpenOptions"] | ["options", "File"]
    )
    .then_some((root, names))
}

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
    /// The `OpenOptions::new()` calls that a chain opens to read, which write nothing.
    read_opens: std::collections::HashSet<usize>,
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
                    if called && METHOD_WRITES.contains(&name.as_str()) {
                        self.flag(format!("{name}(…) in a macro"));
                    }
                    // `fs :: write (`: the two names before the call, with their `::`.
                    let before = at
                        .checked_sub(3)
                        .filter(|from| {
                            matches!(&trees[*from + 1], TokenTree::Punct(first) if first.as_char() == ':')
                                && matches!(&trees[*from + 2], TokenTree::Punct(second) if second.as_char() == ':')
                        })
                        .and_then(|from| match &trees[from] {
                            TokenTree::Ident(before) => Some(before.to_string()),
                            _ => None,
                        });
                    if let (true, Some(before)) = (called, before) {
                        if writes_raw(&[before.clone(), name.clone()]) {
                            self.flag(format!("{before}::{name}(…) in a macro"));
                        }
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
        if self
            .read_opens
            .contains(&(std::ptr::from_ref(call) as usize))
        {
            for arg in &call.args {
                self.visit_expr(arg);
            }
            return;
        }
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
            // A program that writes, started by name: `Run::new("cp")`, `Command::new("mv")`.
            if last == "new" && matches!(before, Some("Run" | "Command")) {
                if let Some(Expr::Lit(literal)) = call.args.first() {
                    if let syn::Lit::Str(text) = &literal.lit {
                        let value = text.value();
                        let program = value.rsplit(['/', '\\']).next().unwrap_or(&value);
                        let program = program.strip_suffix(".exe").unwrap_or(program);
                        if WRITING_PROGRAMS.contains(&program) {
                            self.flag(format!("{value:?} started as a program that writes"));
                        }
                    }
                }
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
        if method == "open" {
            if let Some((root, names)) = open_chain(call) {
                if !names
                    .iter()
                    .any(|name| OPENS_FOR_WRITING.contains(&name.as_str()))
                {
                    self.read_opens.insert(std::ptr::from_ref(root) as usize);
                }
            }
        }
        if METHOD_WRITES.contains(&method.as_str()) {
            self.flag(format!(
                ".{method}(…) (a change made by a path or a handle)"
            ));
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
        let named: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if writes_raw(&named) {
            self.flag(format!(
                "{} (a write that does not ask the door's writers)",
                named.join("::")
            ));
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
        for imported in imports(&item.tree, Vec::new()) {
            if writes_raw(&imported)
                || (imported.last().is_some_and(|last| last == "*")
                    && imported
                        .iter()
                        .rev()
                        .nth(1)
                        .is_some_and(|before| before == "fs"))
            {
                self.flag(format!(
                    "use {} (a write that does not ask the door's writers)",
                    imported.join("::")
                ));
            }
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

/// Every path a `use` brings in, as its names (`use std::fs::{write, File}` is `std::fs::write`
/// and `std::fs::File`; a glob ends in `*`).
fn imports(tree: &UseTree, mut above: Vec<String>) -> Vec<Vec<String>> {
    match tree {
        UseTree::Path(path) => {
            above.push(path.ident.to_string());
            imports(&path.tree, above)
        }
        UseTree::Name(name) => {
            above.push(name.ident.to_string());
            vec![above]
        }
        UseTree::Rename(rename) => {
            above.push(rename.ident.to_string());
            vec![above]
        }
        UseTree::Glob(_) => {
            above.push("*".to_owned());
            vec![above]
        }
        UseTree::Group(group) => group
            .items
            .iter()
            .flat_map(|item| imports(item, above.clone()))
            .collect(),
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
        read_opens: std::collections::HashSet::new(),
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

/// The kit's files the rule reads, and the root they are named from: all of `src` but what the
/// door is made of (`src/door*`, `src/files.rs`, the descent's files and the process primitives
/// `src/process/pid.rs`) and the code only a test build has, and the `tests/live_*` suites. A
/// folder of tests that is none of a live run's (`tests/rig`, `tests/regressions`, `tests/cli`,
/// `tests/smoke`, `tests/daemon`, …) is left out by its folder: it runs on folders it makes itself.
fn live_files() -> (PathBuf, Vec<PathBuf>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    let mut files = rust_files(&src);
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
    let door = src.join("door");
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
            || file.starts_with(&door)
            || *file == door.with_extension("rs")
            || *file == src.join("files.rs")
            || *file == src.join("process").join("pid.rs")
            // The builder of the programs a suite starts: it runs `cargo build` in the checkout and
            // keeps a lock and a stamp in the folder the builds share (`cf-e2e-builds` below the
            // checkout's target folder), which no run of the live kit makes: `cargo xtask live`
            // builds first and names the folder (`CF_E2E_BUILT`), and the builder then takes the
            // programs from it and writes nothing.
            || *file == src.join("cf.rs");
        let unit_test = gated.iter().any(|gate| file.starts_with(gate));
        !own && !unit_test
    });
    (root, files)
}

#[test]
fn the_live_kit_ends_removes_and_writes_only_through_its_door() {
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
        // Writes that do not ask the door's writers.
        "fn f() { std::fs::write(p, b\"x\").unwrap(); }",
        "fn f() { fs::copy(a, b).unwrap(); }",
        "fn f() { let _ = fs::rename(a, b); }",
        "fn f() { fs::create_dir_all(p).unwrap(); }",
        "fn f() { std::fs::create_dir(p).unwrap(); }",
        "fn f() { let file = File::create(p).unwrap(); }",
        "fn f() { let file = std::fs::File::create_new(p).unwrap(); }",
        "fn f() { let file = OpenOptions::new().append(true).open(p); }",
        "fn f() { let file = OpenOptions::new().read(true).write(true).open(p); }",
        "fn f() { let mut open = OpenOptions::new(); open.read(true); }",
        "fn f() { let file = std::fs::OpenOptions::new().write(true).open(p); }",
        "fn f() { let file = File::options().write(true).open(p); }",
        "fn f() { files::write(p, \"x\").unwrap(); }",
        "fn f() { crate::files::copy(a, b).unwrap(); }",
        "fn f() { crate::files::make_dir(p).unwrap(); }",
        "fn f() { crate::files::write_executable(p, \"\").unwrap(); }",
        "fn f() { file.set_len(0).unwrap(); }",
        "fn f() { file.set_permissions(mode).unwrap(); }",
        "fn f() { std::os::unix::fs::symlink(a, b).unwrap(); }",
        "fn f() { let write = fs::write; }",
        "use std::fs::{write, File};",
        "use std::fs::*;",
        "fn f() { let _ = Run::new(\"cp\").args([a, b]); }",
        "fn f() { let _ = std::process::Command::new(\"/bin/mv\"); }",
        "fn f() { assert!(fs::write(p, \"x\").is_ok()); }",
        "fn f() { paths.iter().for_each(|p| { let _ = fs::write(p, \"\"); }); }",
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
        // What reads, and what asks the door's writers.
        "fn f() { let _ = crate::door::write(p, \"x\"); }",
        "fn f() { let _ = std::fs::read_to_string(p); let _ = File::open(p); let _ = fs::metadata(p); }",
        "fn f() { let _ = fs::read_dir(p); let _ = fs::canonicalize(p); }",
        "#[cfg(test)]\nmod tests { fn f() { fs::write(p, \"x\").unwrap(); } }",
        "use std::fs::{self, File, OpenOptions};",
        "fn f() { let _ = Run::new(\"ls\"); let _ = Command::new(\"copier\"); }",
        "fn f() { let file = OpenOptions::new().read(true).custom_flags(1).open(p); }",
        "fn f() { let file = File::options().read(true).open(p); }",
        "fn f() { writer.write(b\"x\"); }",
        "fn f() { let _ = files::read(p); let _ = files::read_string(p); }",
        "// fs::write(p, \"x\")\nfn f() {}",
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
