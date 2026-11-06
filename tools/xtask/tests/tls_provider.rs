//! Every HTTPS client and TLS config the product builds has its TLS provider (T-1247,
//! 3.0.7-alpha.2). The workspace's reqwest is `rustls-no-provider`: it brings none, and the build
//! of a client panics where the process has none installed ("No rustls crypto provider is
//! configured"). The app's update check on Windows panicked so at every start, since
//! `update_page::read_feed` was written: the updater's plugin installs ring before it builds its
//! own client, and on Windows no check goes through the plugin. This holds the rule over the
//! whole workspace, from its sources read as Rust (`syn`), one construction at a time:
//!
//! - **A reqwest client** is a call of `Client::new`, `Client::default`, `Client::builder`,
//!   `ClientBuilder::new` or `reqwest::get`, however the name got there (`use reqwest::Client as
//!   Http`, `use reqwest as r`, `type Http = reqwest::Client`, a glob of reqwest). It has its
//!   provider if the chain of calls it heads names one (`tls_backend_preconfigured`,
//!   `use_preconfigured_tls`: the daemon's way, a rustls config built on ring and handed to the
//!   client), or if a call before it, in the same function, installs one for the process
//!   (`install_ring_as_default`, `install_default`: the app's way, `app/src-tauri/src/tls.rs`).
//!   Nothing is granted to a file: a second client beside a configured one is judged on its own.
//! - **A rustls config** is never built on the process's default provider (`ClientConfig::
//!   builder()`, `ServerConfig::builder()`, `WebPkiServerVerifier::builder()`...):
//!   `builder_with_provider(ring)` names it, in a process that has none installed and in one that
//!   has.
//! - **The build holds ring alone:** a second provider (aws-lc-rs, which reqwest's own `rustls`
//!   feature builds in) is a second one to choose from where none is installed, and `Cargo.lock`
//!   has none.
//!
//! What is read is code: a comment, a doc comment or a string that holds the example of a
//! construction is no construction. What is not seen: a client made by another crate's code, and
//! a name that reaches a file through a private alias of another file's (`use super::*` over a
//! `type Http = reqwest::Client`); a public alias or re-export of reqwest's or rustls's types is
//! itself refused, so that none leaves a file. The install is judged by its place in the
//! function, not by the paths that reach it (`if cond { install() }` counts).
#![allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Expr, ExprCall, ImplItemFn, ItemEnum, ItemFn, ItemStruct, ItemTrait, ItemType, ItemUnion,
    ItemUse, Macro, Token, TraitItemFn, Type, UseTree, Visibility,
};

use common::checkout;

/// Where a client's chain of calls starts: the chain may name the provider.
const CLIENTS_WITH_A_CHAIN: &[&[&str]] = &[
    &["reqwest", "Client", "builder"],
    &["reqwest", "ClientBuilder", "new"],
    &["reqwest", "ClientBuilder", "default"],
    &["reqwest", "blocking", "Client", "builder"],
    &["reqwest", "blocking", "ClientBuilder", "new"],
    &["reqwest", "blocking", "ClientBuilder", "default"],
];

/// A client made whole, or from a builder out of sight: only an install before it gives it one.
const CLIENTS_MADE_WHOLE: &[&[&str]] = &[
    &["reqwest", "Client", "new"],
    &["reqwest", "Client", "default"],
    &["reqwest", "get"],
    &["reqwest", "ClientBuilder", "build"],
    &["reqwest", "blocking", "Client", "new"],
    &["reqwest", "blocking", "Client", "default"],
    &["reqwest", "blocking", "get"],
    &["reqwest", "blocking", "ClientBuilder", "build"],
];

/// What takes the process's default provider where it should name one.
const ON_THE_DEFAULT: &[&[&str]] = &[
    &["rustls", "ClientConfig", "builder"],
    &["rustls", "ClientConfig", "builder_with_protocol_versions"],
    &["rustls", "ServerConfig", "builder"],
    &["rustls", "ServerConfig", "builder_with_protocol_versions"],
    &["rustls", "client", "WebPkiServerVerifier", "builder"],
    &["rustls", "server", "WebPkiClientVerifier", "builder"],
];

/// The methods of a client's chain that hand it a config of its own.
const NAMES_A_PROVIDER: &[&str] = &["tls_backend_preconfigured", "use_preconfigured_tls"];

/// The function the app installs its provider with (app/src-tauri/src/tls.rs), and the rustls
/// method that does it.
const INSTALLING_FUNCTION: &str = "install_ring_as_default";
const INSTALLING_METHOD: &str = "install_default";

/// The last names of what a file must not pass on under a public name of its own.
const SENSITIVE: &[&str] = &[
    "Client",
    "ClientBuilder",
    "get",
    "ClientConfig",
    "ServerConfig",
    "WebPkiServerVerifier",
    "WebPkiClientVerifier",
];

/// Which test a finding belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A reqwest client with no provider, or a file this cannot read.
    Client,
    /// A rustls config on the default provider.
    Config,
}

#[derive(Clone, Debug)]
struct Finding {
    kind: Kind,
    text: String,
}

/// What a file's `use`s and aliases make a name stand for, and what it defines itself.
#[derive(Default)]
struct Names {
    /// A local name and the path it stands for (whose first segment may be a local name too).
    stands_for: BTreeMap<String, Vec<String>>,
    /// The paths of the glob imports: `use reqwest::*` is `["reqwest"]`.
    globs: Vec<Vec<String>>,
    /// The types the file defines: a `Client` of its own is no reqwest's.
    defined: BTreeSet<String>,
    /// What the pass found before the judging: public aliases and re-exports.
    findings: Vec<Finding>,
}

impl Names {
    /// The path `segments` names, with the local names in front of it replaced.
    fn resolve(&self, segments: &[String]) -> Vec<String> {
        let mut path = segments.to_vec();
        for _ in 0..5 {
            let Some(first) = path.first() else { break };
            let Some(full) = self.stands_for.get(first) else {
                break;
            };
            let mut next = full.clone();
            next.extend_from_slice(&path[1..]);
            if next == path {
                break;
            }
            path = next;
        }
        path
    }
}

/// `table` holds `path`.
fn listed(table: &[&[&str]], path: &[String]) -> bool {
    table
        .iter()
        .any(|entry| entry.iter().copied().eq(path.iter().map(String::as_str)))
}

fn is_a_crate_of_ours(segment: &str) -> bool {
    segment == "reqwest" || segment == "rustls"
}

fn segments_of(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

/// A `use` tree's names, as `prefix` leads to them.
fn collect_use(tree: &UseTree, prefix: &mut Vec<String>, into: &mut Names) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use(&path.tree, prefix, into);
            prefix.pop();
        }
        UseTree::Name(name) => {
            let ident = name.ident.to_string();
            if ident == "self" {
                if let Some(last) = prefix.last() {
                    into.stands_for.insert(last.clone(), prefix.clone());
                }
            } else {
                let mut full = prefix.clone();
                full.push(ident.clone());
                into.stands_for.insert(ident, full);
            }
        }
        UseTree::Rename(rename) => {
            let mut full = prefix.clone();
            if rename.ident != "self" {
                full.push(rename.ident.to_string());
            }
            into.stands_for.insert(rename.rename.to_string(), full);
        }
        UseTree::Glob(_) => into.globs.push(prefix.clone()),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use(item, prefix, into);
            }
        }
    }
}

/// The first pass over a file: its names.
struct Collect<'a> {
    label: &'a str,
    names: &'a mut Names,
}

impl Collect<'_> {
    /// A public name that passes reqwest's or rustls's on to other files, where it is not seen.
    fn refuse_a_public(&mut self, what: &str, resolved: &[String]) {
        let Some(first) = resolved.first() else {
            return;
        };
        let last = resolved.last().map(String::as_str).unwrap_or_default();
        if is_a_crate_of_ours(first) && SENSITIVE.contains(&last) {
            let kind = if first == "reqwest" {
                Kind::Client
            } else {
                Kind::Config
            };
            self.names.findings.push(Finding {
                kind,
                text: format!(
                    "{}: {what} `{}` under a name that is not private: build it where it is named, in full",
                    self.label,
                    resolved.join("::")
                ),
            });
        }
    }
}

impl<'ast> Visit<'ast> for Collect<'_> {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        collect_use(&item.tree, &mut Vec::new(), self.names);
        if !matches!(item.vis, Visibility::Inherited) {
            let mut seen = Names::default();
            collect_use(&item.tree, &mut Vec::new(), &mut seen);
            let imported: Vec<Vec<String>> = seen
                .stands_for
                .into_values()
                .chain(seen.globs.into_iter().map(|mut glob| {
                    glob.push("*".to_owned());
                    glob
                }))
                .collect();
            for path in imported {
                if path.last().map(String::as_str) == Some("*")
                    && path.first().is_some_and(|first| is_a_crate_of_ours(first))
                {
                    let kind = if path.first().map(String::as_str) == Some("reqwest") {
                        Kind::Client
                    } else {
                        Kind::Config
                    };
                    self.names.findings.push(Finding {
                        kind,
                        text: format!(
                            "{}: re-exports `{}` under a name that is not private",
                            self.label,
                            path.join("::")
                        ),
                    });
                } else {
                    self.refuse_a_public("re-exports", &path);
                }
            }
        }
        visit::visit_item_use(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast ItemType) {
        if let Type::Path(path) = &*item.ty {
            let target = segments_of(&path.path);
            if !matches!(item.vis, Visibility::Inherited) {
                let resolved = self.names.resolve(&target);
                self.refuse_a_public("aliases", &resolved);
            }
            self.names.stands_for.insert(item.ident.to_string(), target);
        }
        visit::visit_item_type(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        self.names.defined.insert(item.ident.to_string());
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        self.names.defined.insert(item.ident.to_string());
        visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast ItemUnion) {
        self.names.defined.insert(item.ident.to_string());
        visit::visit_item_union(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
        self.names.defined.insert(item.ident.to_string());
        visit::visit_item_trait(self, item);
    }
}

/// What a call builds, as far as this rule is concerned.
enum Made {
    /// A client from a builder: its chain may name the provider.
    WithAChain(String),
    /// A client with nothing in sight to name one.
    Whole(String),
    /// A rustls config on the process's default provider.
    OnTheDefault(String),
    /// A name that comes from a glob import of which crate is not told.
    Unknown(String),
}

/// A function the judging is in: whether a call before the place has installed a provider.
struct Frame {
    name: String,
    installed: bool,
}

/// The second pass over a file: each construction.
struct Judge<'a> {
    label: &'a str,
    names: &'a Names,
    frames: Vec<Frame>,
    found: Vec<Finding>,
}

impl Judge<'_> {
    /// What the call of `segments` makes, if it is a construction this rule judges.
    fn made_by(&self, segments: &[String]) -> Option<Made> {
        let resolved = self.names.resolve(segments);
        let table_hit = |path: &[String]| {
            if listed(CLIENTS_WITH_A_CHAIN, path) {
                Some(Made::WithAChain(path.join("::")))
            } else if listed(CLIENTS_MADE_WHOLE, path) {
                Some(Made::Whole(path.join("::")))
            } else if listed(ON_THE_DEFAULT, path) {
                Some(Made::OnTheDefault(path.join("::")))
            } else {
                None
            }
        };
        if let Some(made) = table_hit(&resolved) {
            return Some(made);
        }
        if is_a_crate_of_ours(resolved.first()?) {
            return None;
        }
        // A glob of reqwest's or rustls's brings the name in: `use reqwest::*; Client::new()`.
        for glob in &self.names.globs {
            if glob.first().is_some_and(|name| is_a_crate_of_ours(name)) {
                let mut through = glob.clone();
                through.extend(resolved.iter().cloned());
                if let Some(made) = table_hit(&through) {
                    return Some(made);
                }
            }
        }
        // `Client::new()` with the name from where this cannot tell which crate's it is: a
        // `use super::Client`, or a glob that is not of these crates (a `use super::*`).
        let original = segments.first()?;
        let from_here = self
            .names
            .stands_for
            .get(original)
            .and_then(|path| path.first())
            .is_some_and(|first| matches!(first.as_str(), "super" | "crate" | "self"));
        let bound = (self.names.stands_for.contains_key(original) && !from_here)
            || self.names.defined.contains(original);
        let constructs = segments.len() == 2
            && matches!(
                segments[1].as_str(),
                "new" | "builder" | "default" | "builder_with_protocol_versions"
            );
        if constructs
            && !bound
            && ["Client", "ClientBuilder", "ClientConfig", "ServerConfig"]
                .contains(&original.as_str())
            && (from_here || !self.names.globs.is_empty())
        {
            return Some(Made::Unknown(format!("{original}::{}", segments[1])));
        }
        None
    }

    /// Where the judging is: the function, or the file's top.
    fn place(&self) -> String {
        self.frames.last().map_or_else(
            || "the top of the file".to_owned(),
            |frame| format!("fn {}", frame.name),
        )
    }

    fn installed(&self) -> bool {
        self.frames.last().is_some_and(|frame| frame.installed)
    }

    fn install(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.installed = true;
        }
    }

    /// A construction, with the methods of the chain it heads (outermost first).
    fn judge(&mut self, made: Made, methods: &[String]) {
        let place = self.place();
        let label = self.label;
        let finding = match made {
            Made::OnTheDefault(path) => Some(Finding {
                kind: Kind::Config,
                text: format!(
                    "{label}: in {place}: `{path}` builds a TLS config on the process's default provider: name it with builder_with_provider(ring)"
                ),
            }),
            Made::Unknown(path) => Some(Finding {
                kind: if path.contains("Config") {
                    Kind::Config
                } else {
                    Kind::Client
                },
                text: format!(
                    "{label}: in {place}: `{path}` comes from a glob import, and which crate's it is cannot be told: write it in full"
                ),
            }),
            Made::Whole(path) if !self.installed() => Some(Finding {
                kind: Kind::Client,
                text: format!(
                    "{label}: in {place}: `{path}` builds a client on the process's default provider, and no call before it in the function installs one ({INSTALLING_FUNCTION})"
                ),
            }),
            Made::WithAChain(path)
                if !self.installed()
                    && !methods
                        .iter()
                        .any(|method| NAMES_A_PROVIDER.contains(&method.as_str())) =>
            {
                Some(Finding {
                    kind: Kind::Client,
                    text: format!(
                        "{label}: in {place}: `{path}` builds a client whose chain hands it no config of its own ({}) and no call before it in the function installs a provider ({INSTALLING_FUNCTION})",
                        NAMES_A_PROVIDER.join(", ")
                    ),
                })
            }
            Made::Whole(_) | Made::WithAChain(_) => None,
        };
        self.found.extend(finding);
    }

    /// The calls in the tokens of a macro that is not a list of expressions, in order.
    fn scan(&mut self, tokens: TokenStream) {
        let mut flat = Vec::new();
        flatten(tokens, &mut flat);
        let mut at = 0;
        while at < flat.len() {
            let Tok::Ident(first) = &flat[at] else {
                at += 1;
                continue;
            };
            let after_a_dot_or_path = at > 0 && matches!(flat[at - 1], Tok::Punct('.' | ':'));
            if after_a_dot_or_path {
                // A method: only the installing one matters.
                if first == INSTALLING_METHOD
                    && flat.get(at + 1) == Some(&Tok::Open(Delimiter::Parenthesis))
                {
                    self.install();
                }
                at += 1;
                continue;
            }
            // `a::b::c`
            let mut path = vec![first.clone()];
            let mut end = at + 1;
            while let (Some(Tok::Punct(':')), Some(Tok::Punct(':')), Some(Tok::Ident(next))) =
                (flat.get(end), flat.get(end + 1), flat.get(end + 2))
            {
                path.push(next.clone());
                end += 3;
            }
            if flat.get(end) == Some(&Tok::Open(Delimiter::Parenthesis)) {
                if path.last().map(String::as_str) == Some(INSTALLING_FUNCTION) {
                    self.install();
                } else if let Some(made) = self.made_by(&path) {
                    let methods = chain_after(&flat, end);
                    self.judge(made, &methods);
                }
            }
            at += 1;
        }
    }
}

/// A token of a macro's body, flat: its groups are an opening and a closing.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Punct(char),
    Open(Delimiter),
    Close,
    Other,
}

fn flatten(tokens: TokenStream, into: &mut Vec<Tok>) {
    for tree in tokens {
        match tree {
            TokenTree::Group(group) => {
                into.push(Tok::Open(group.delimiter()));
                flatten(group.stream(), into);
                into.push(Tok::Close);
            }
            TokenTree::Ident(ident) => into.push(Tok::Ident(ident.to_string())),
            TokenTree::Punct(punct) => into.push(Tok::Punct(punct.as_char())),
            TokenTree::Literal(_) => into.push(Tok::Other),
        }
    }
}

/// The index of the token that closes the group opened at `open`.
fn closing(flat: &[Tok], open: usize) -> usize {
    let mut depth = 0usize;
    for (at, token) in flat.iter().enumerate().skip(open) {
        match token {
            Tok::Open(_) => depth += 1,
            Tok::Close => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
    }
    flat.len()
}

/// The methods called one after the other on the call whose arguments open at `open`.
fn chain_after(flat: &[Tok], open: usize) -> Vec<String> {
    let mut methods = Vec::new();
    let mut at = closing(flat, open) + 1;
    while let (Some(Tok::Punct('.')), Some(Tok::Ident(method))) = (flat.get(at), flat.get(at + 1)) {
        methods.push(method.clone());
        at += 2;
        // A turbofish: `::<T>`.
        if flat.get(at) == Some(&Tok::Punct(':')) {
            while at < flat.len() && flat[at] != Tok::Open(Delimiter::Parenthesis) {
                at += 1;
            }
        }
        if flat.get(at) == Some(&Tok::Open(Delimiter::Parenthesis)) {
            at = closing(flat, at) + 1;
        } else {
            break;
        }
    }
    methods
}

/// The chain `expr` ends: its first call, and the methods called on it, outermost first.
fn chain_of(mut expr: &Expr) -> (&Expr, Vec<String>) {
    let mut methods = Vec::new();
    loop {
        match expr {
            Expr::MethodCall(call) => {
                methods.push(call.method.to_string());
                expr = &call.receiver;
            }
            Expr::Paren(paren) => expr = &paren.expr,
            Expr::Group(group) => expr = &group.expr,
            _ => return (expr, methods),
        }
    }
}

/// The path a call is made on, with `<T>::f` and `<T as Trait>::f` read as `T::f`.
fn callee(call: &ExprCall) -> Option<Vec<String>> {
    let Expr::Path(path) = &*call.func else {
        return None;
    };
    match &path.qself {
        None => Some(segments_of(&path.path)),
        Some(qself) => {
            let Type::Path(on) = &*qself.ty else {
                return None;
            };
            let mut segments = segments_of(&on.path);
            segments.push(path.path.segments.last()?.ident.to_string());
            Some(segments)
        }
    }
}

fn installs(call: &ExprCall) -> bool {
    matches!(&*call.func, Expr::Path(path)
        if path.path.segments.last().is_some_and(|last| last.ident == INSTALLING_FUNCTION))
}

impl<'ast> Visit<'ast> for Judge<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.frames.push(Frame {
            name: item.sig.ident.to_string(),
            installed: false,
        });
        visit::visit_item_fn(self, item);
        self.frames.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.frames.push(Frame {
            name: item.sig.ident.to_string(),
            installed: false,
        });
        visit::visit_impl_item_fn(self, item);
        self.frames.pop();
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        self.frames.push(Frame {
            name: item.sig.ident.to_string(),
            installed: false,
        });
        visit::visit_trait_item_fn(self, item);
        self.frames.pop();
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        let (first, methods) = chain_of(expr);
        // The chain a client is built by is judged whole, at its last call, and its parts are
        // walked in the order they are written: the first call's arguments, then each method's.
        if let Expr::Call(call) = first {
            if let Some(made) = callee(call).and_then(|path| self.made_by(&path)) {
                self.judge(made, &methods);
                for argument in &call.args {
                    self.visit_expr(argument);
                }
                let mut parts = Vec::new();
                let mut at = expr;
                loop {
                    match at {
                        Expr::MethodCall(method) => {
                            parts.push(method);
                            at = &method.receiver;
                        }
                        Expr::Paren(paren) => at = &paren.expr,
                        Expr::Group(group) => at = &group.expr,
                        _ => break,
                    }
                }
                for method in parts.into_iter().rev() {
                    for argument in &method.args {
                        self.visit_expr(argument);
                    }
                }
                return;
            }
        }
        match expr {
            Expr::Call(call) if installs(call) => self.install(),
            Expr::MethodCall(call) if call.method == INSTALLING_METHOD => self.install(),
            _ => {}
        }
        visit::visit_expr(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if let Ok(arguments) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
        {
            for argument in &arguments {
                self.visit_expr(argument);
            }
        } else {
            self.scan(mac.tokens.clone());
        }
    }
}

/// What this rule finds in one file of Rust, which `label` names.
fn findings(label: &str, source: &str) -> Vec<Finding> {
    // A file that names neither crate holds no construction this rule judges; one that does is
    // read whole, and one that cannot be read is no pass.
    if !source.contains("reqwest") && !source.contains("rustls") {
        return Vec::new();
    }
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(cause) => {
            return vec![Finding {
                kind: Kind::Client,
                text: format!("{label}: cannot be read as Rust ({cause}), so cannot be judged"),
            }]
        }
    };
    let mut names = Names::default();
    Collect {
        label,
        names: &mut names,
    }
    .visit_file(&file);
    let mut judge = Judge {
        label,
        names: &names,
        frames: Vec::new(),
        found: Vec::new(),
    };
    judge.visit_file(&file);
    let mut found = names.findings.clone();
    found.append(&mut judge.found);
    found
}

/// Every Rust file under `folder`, but for build folders.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !["target", "node_modules", ".git"].contains(&name.as_str()) {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The Rust sources of the workspace: its crates, its tools and the app's.
fn workspace() -> Vec<PathBuf> {
    let root = checkout();
    let mut found = Vec::new();
    for folder in [
        "crates",
        "tools",
        "app/src-tauri/src",
        "app/src-tauri/tests",
    ] {
        sources(&root.join(folder), &mut found);
    }
    found
}

/// The path from the checkout's root, with `/` between folders.
fn relative(path: &Path) -> String {
    path.strip_prefix(checkout())
        .unwrap()
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What the rule finds in the workspace, of one kind.
fn found_in_the_workspace(kind: Kind) -> Vec<String> {
    workspace()
        .iter()
        .flat_map(|path| {
            let source = fs::read_to_string(path).unwrap();
            findings(&relative(path), &source)
        })
        .filter(|finding| finding.kind == kind)
        .map(|finding| finding.text)
        .collect()
}

#[test]
fn every_client_the_product_builds_says_where_its_provider_comes_from() {
    let found = found_in_the_workspace(Kind::Client);
    assert!(
        found.is_empty(),
        "reqwest here brings no TLS provider (rustls-no-provider): a client panics at its build where the process has none:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_tls_config_is_built_on_the_default_provider() {
    let found = found_in_the_workspace(Kind::Config);
    assert!(
        found.is_empty(),
        "name the provider, builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_build_holds_ring_as_its_only_tls_provider() {
    let lock = fs::read_to_string(checkout().join("Cargo.lock")).expect("Cargo.lock");
    for name in ["aws-lc-rs", "aws-lc-sys"] {
        assert!(
            !lock.contains(&format!("name = \"{name}\"")),
            "Cargo.lock holds {name}: ring is the one TLS provider the app and the daemon build on"
        );
    }
    assert!(
        lock.contains("name = \"ring\""),
        "Cargo.lock holds no ring: the provider the app installs and the daemon hands its client"
    );
}

/// What the rule says of `source`, as the lines it would print.
fn said(source: &str) -> Vec<String> {
    findings("a.rs", source)
        .into_iter()
        .map(|finding| finding.text)
        .collect()
}

/// The shapes the product has: the daemon's chain, the app's install before the client.
#[test]
fn a_client_with_a_config_of_its_own_or_a_provider_installed_before_it_is_judged_whole() {
    let daemon = r#"
        fn client() -> Result<reqwest::Client, String> {
            let tls = ClientConfig::builder_with_provider(provider).with_no_client_auth();
            reqwest::Client::builder()
                .tls_backend_preconfigured(tls)
                .user_agent("x")
                .build()
                .map_err(|e| e.to_string())
        }
    "#;
    assert_eq!(said(daemon), Vec::<String>::new());
    let app = r#"
        fn client() -> Result<reqwest::Client, String> {
            crate::tls::install_ring_as_default();
            reqwest::Client::builder().https_only(true).build().map_err(|e| e.to_string())
        }
        fn another() {
            if true { tls::install_ring_as_default(); }
            let _ = reqwest::Client::new();
        }
        fn by_rustls() {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let _ = reqwest::Client::new();
        }
    "#;
    assert_eq!(said(app), Vec::<String>::new());
    // A `Client` that is not reqwest's is not judged.
    assert_eq!(
        said("use ureq::Client; use reqwest::Error; fn f() { let _ = Client::new(); }"),
        Vec::<String>::new()
    );
    assert_eq!(
        said("use reqwest::Error; struct Client; impl Client { fn new() -> Self { Client } } fn f() { Client::new(); }"),
        Vec::<String>::new()
    );
}

/// The escapes the review of T-1247 found: a file's grant covered every client in it.
#[test]
fn a_second_client_beside_a_configured_one_and_an_aliased_constructor_are_judged_on_their_own() {
    let second = r#"
        fn client() -> reqwest::Client {
            reqwest::Client::builder().tls_backend_preconfigured(tls).build().unwrap()
        }
        fn other() -> reqwest::Client {
            reqwest::Client::new()
        }
    "#;
    let found = said(second);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("fn other") && found[0].contains("reqwest::Client::new"),
        "{found:?}"
    );
    for aliased in [
        "use reqwest::Client as Http; fn f() { let _ = Http::new(); }",
        "use reqwest::{Client as Http, Error}; fn f() { let _ = Http::default(); }",
        "use reqwest as r; fn f() { let _ = r::Client::builder().build(); }",
        "use ::reqwest::ClientBuilder as B; fn f() { let _ = B::new().build(); }",
        "use reqwest::Client; type Http = Client; fn f() { let _ = Http::new(); }",
        "type Http = reqwest::Client; fn f() { let _ = Http::new(); }",
        "use reqwest::*; fn f() { let _ = Client::new(); }",
        "use reqwest::get as fetch; async fn f() { let _ = fetch(\"https://x\").await; }",
        "fn f() { let _ = <reqwest::Client>::new(); }",
        "fn f() { let _ = <reqwest::Client as Default>::default(); }",
        "fn f() { let _ = reqwest::ClientBuilder::build(b); }",
        "static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| reqwest::Client::new());",
    ] {
        let found = said(aliased);
        assert_eq!(found.len(), 1, "{aliased}: {found:?}");
    }
    // The name from a glob that cannot be told is no pass either.
    let unknown = said("use reqwest::Error; use super::*; fn f() { let _ = Client::new(); }");
    assert_eq!(unknown.len(), 1, "{unknown:?}");
    // An install after the client, or in another function, gives it no provider.
    for late in [
        "fn f() { let _ = reqwest::Client::new(); crate::tls::install_ring_as_default(); }",
        "fn f() { crate::tls::install_ring_as_default(); } fn g() { let _ = reqwest::Client::new(); }",
        "fn f() { let _ = reqwest::Client::builder().build(); rustls::crypto::ring::default_provider().install_default(); }",
    ] {
        assert_eq!(said(late).len(), 1, "{late}");
    }
    // A name that leaves a file is refused, so that no alias is out of sight.
    for out in [
        "pub use reqwest::Client;",
        "pub(crate) use reqwest::{Client as Http, Error};",
        "pub use reqwest::*;",
        "pub type Http = reqwest::Client;",
        "pub use rustls::ClientConfig;",
    ] {
        assert_eq!(said(out).len(), 1, "{out}");
    }
    assert_eq!(
        said("use reqwest::Client; type Http = Client;"),
        Vec::<String>::new()
    );
}

/// A client is found where the compiler would find it: in a macro's arguments, or in the tokens
/// of one that holds no list of expressions.
#[test]
fn a_client_in_a_macro_is_judged_as_one_out_of_it() {
    for inside in [
        "fn f() { assert!(reqwest::Client::new().get(\"https://x\").build().is_ok()); }",
        "fn f() { let _ = format!(\"{:?}\", reqwest::Client::new()); }",
        "fn f() { matches!(reqwest::Client::new(), c if c.get(\"x\").build().is_ok()); }",
        "macro_rules! make { () => { reqwest::Client::new() }; } ",
        "fn f() { make!(let c = reqwest::Client::builder().build();); }",
    ] {
        assert_eq!(said(inside).len(), 1, "{inside}: {:?}", said(inside));
    }
    // And installed in a macro's arguments before it.
    assert_eq!(
        said("fn f() { assert!(tls::install_ring_as_default() == ()); let _ = reqwest::Client::new(); }"),
        Vec::<String>::new()
    );
    assert_eq!(
        said("fn f() { make!(tls::install_ring_as_default(); let _ = reqwest::Client::builder().use_preconfigured_tls(c);); }"),
        Vec::<String>::new()
    );
}

/// Rustls configs: the provider is named, however the type was reached.
#[test]
fn a_tls_config_on_the_default_provider_is_found_however_its_type_was_reached() {
    for on_the_default in [
        "fn f() { let _ = rustls::ClientConfig::builder(); }",
        "use rustls::ClientConfig as Tls; fn f() { let _ = Tls::builder(); }",
        "use rustls::{ServerConfig}; fn f() { let _ = ServerConfig::builder(); }",
        "use rustls::*; fn f() { let _ = ClientConfig::builder_with_protocol_versions(&[]); }",
        "use rustls as tls; fn f() { let _ = tls::ServerConfig::builder(); }",
        "use rustls::client::WebPkiServerVerifier; fn f() { let _ = WebPkiServerVerifier::builder(roots); }",
        "fn f() { assert!(rustls::ClientConfig::builder().with_no_client_auth().alpn_protocols.is_empty()); }",
    ] {
        let found = findings("a.rs", on_the_default);
        assert_eq!(found.len(), 1, "{on_the_default}: {found:?}");
        assert_eq!(found[0].kind, Kind::Config, "{on_the_default}");
    }
    for named in [
        "fn f() { let _ = rustls::ClientConfig::builder_with_provider(p); }",
        "use rustls::ServerConfig; fn f() { let _ = ServerConfig::builder_with_provider(p); }",
        "fn f() { let _ = rustls_platform_verifier::Verifier::new(p); }",
    ] {
        assert_eq!(said(named), Vec::<String>::new(), "{named}");
    }
}

/// What is no code is no construction: a comment, a doc comment, a string, a raw string.
#[test]
fn a_comment_or_a_string_that_holds_the_example_of_a_construction_is_no_construction() {
    let harmless = r##"
        //! Docs: build it with reqwest::Client::new(), not with ClientConfig::builder().
        /* Avoid ClientConfig::builder() here, and reqwest::Client::new() there. */
        /// A doc comment of reqwest::Client::builder().build().
        fn f() {
            // let _ = rustls::ServerConfig::builder();
            /* let _ = reqwest::get("https://x"); /* nested: reqwest::Client::new() */ */
            let a = "reqwest::Client::new() and ClientConfig::builder()";
            let b = r#"use reqwest::Client as Http; Http::new(); rustls::ClientConfig::builder()"#;
            let c = b"reqwest::Client::new()";
            println!("{}", "rustls::ServerConfig::builder()");
            let _ = (a, b, c);
        }
        #[doc = "rustls::ClientConfig::builder()"]
        fn g() {}
        #[expect(clippy::all, reason = "reqwest::Client::new()")]
        fn h() {}
    "##;
    assert_eq!(said(harmless), Vec::<String>::new());
}

/// A file that cannot be read as Rust is no pass.
#[test]
fn a_file_that_names_reqwest_and_cannot_be_read_is_a_finding() {
    let found = said("fn f( { reqwest::Client::new() ");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("cannot be read"), "{found:?}");
    // One that names neither crate is not read at all.
    assert_eq!(said("fn f( {"), Vec::<String>::new());
}
