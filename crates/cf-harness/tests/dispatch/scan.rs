//! Finds where code tells one harness from another, reading Rust as syntax.
//!
//! A *site* is a place that branches on which harness it has: a `match` with an arm that
//! names a variant of `Harness`, a `matches!`, an `if let` or a `let … else` on one, an `==`
//! or `!=` against one, the same on a harness's id as a string (`"claude"`), and, outside
//! `cf-harness`, a path into a concrete adapter's module (`cf_harness::claude::…`, a `use`
//! of it among them). Which adapter a harness has is the registry's to say
//! (`cf_harness::launch`); every other site is one more place to change when a harness
//! comes or goes.
//!
//! Sites are found by name, not by type, as syntax allows: `Harness` itself, a name it is
//! imported as (`use … Harness as Kind`, `type Kind = Harness`), its variants imported bare
//! (`use Harness::{Claude, Codex}`, `use Harness::*`), and `Self` inside `impl Harness`. Code
//! that only builds with the test kit (`#[cfg(test)]`, `#[cfg(feature = "test-support")]`
//! and the modules they gate) is no part of the product and is not read.
//!
//! A site is named by the file, the item it is in (`Type::method`, `function`, or `use …`)
//! and its kind, and counted: a second `match` in the same function is a second site.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use cf_proto::agents::Harness;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{
    Attribute, BinOp, Expr, ExprBinary, ExprLet, ExprLit, ExprMatch, ExprMethodCall, ImplItem,
    Item, ItemConst, ItemFn, ItemImpl, ItemMod, ItemTrait, ItemType, ItemUse, Lit, Local, Macro,
    Meta, Pat, Token, TraitItem, Type, UseTree,
};

/// How a site tells one harness from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A `match` with an arm on a variant of `Harness`.
    Match,
    /// A `matches!` on one.
    Matches,
    /// An `if let`, a `while let` or a `let … else` on one.
    IfLet,
    /// `==`, `!=`, `.eq()`, `.ne()` or `.contains()` against one.
    Eq,
    /// A `match`, `matches!`, `if let` or `let … else` on a harness's id as a string.
    IdMatch,
    /// The same against a harness's id as a string.
    IdEq,
    /// A path into a concrete adapter's module, from outside `cf-harness`.
    Adapter,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Match => "match on a variant",
            Kind::Matches => "matches! on a variant",
            Kind::IfLet => "if let on a variant",
            Kind::Eq => "comparison with a variant",
            Kind::IdMatch => "match on an id",
            Kind::IdEq => "comparison with an id",
            Kind::Adapter => "path into an adapter",
        })
    }
}

/// Where a harness is told from another: a file (from the checkout's root, with `/`
/// between folders), the item in it, and how.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    pub file: String,
    pub scope: String,
    pub kind: Kind,
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}` ({})", self.file, self.scope, self.kind)
    }
}

/// The sites found, and how many of each.
pub type Sites = BTreeMap<Site, usize>;

/// What the product calls its harnesses.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    /// The variants of `Harness`.
    pub variants: BTreeSet<String>,
    /// The ids a harness goes by in text (`claude`), which are also the names of its
    /// adapter's module in `cf-harness`.
    pub ids: BTreeSet<String>,
    /// The words a harness goes by in text besides its id: its kind (`claude-code`).
    pub kinds: BTreeSet<String>,
}

impl Vocabulary {
    /// Whether `word` is what a harness goes by in text: its id or its kind.
    pub fn names(&self, word: &str) -> bool {
        self.ids.contains(word) || self.kinds.contains(word)
    }

    /// What `cf_proto::agents::Harness` has today.
    pub fn of_the_product() -> Self {
        let mut words = Self::default();
        for harness in Harness::ALL {
            words.ids.insert(harness.as_str().to_owned());
            words.kinds.insert(harness.kind().to_owned());
            words.variants.insert(format!("{harness:?}"));
        }
        words
    }
}

/// What reading one file found.
#[derive(Debug, Default)]
pub struct Read {
    pub sites: Sites,
    /// The modules it declares without a body, which are files of their own.
    pub modules: Vec<Module>,
}

/// A `mod name;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The inline modules it is in, outermost first.
    pub inside: Vec<String>,
    pub name: String,
}

/// What of a file is read: the checkout-relative name its sites carry, and whether paths
/// into adapters count (they do outside `cf-harness`).
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    pub file: &'a str,
    pub adapter_paths: bool,
}

/// The sites in `source`, a Rust file, and the modules it declares.
pub fn read(source: &str, reading: Reading<'_>, words: &Vocabulary) -> Result<Read, String> {
    let mut file =
        syn::parse_file(source).map_err(|failed| format!("{}: {failed}", reading.file))?;
    let mut names = Names::new(words);
    names.visit_file_mut(&mut file);
    let mut finder = Finder {
        file: reading.file,
        words,
        names,
        adapter_paths: reading.adapter_paths,
        scope: Vec::new(),
        inline: Vec::new(),
        harness_impl: false,
        found: Read::default(),
        unsupported: Vec::new(),
    };
    finder.visit_file_mut(&mut file);
    match finder.unsupported.first() {
        Some(said) => Err(format!("{}: {said}", reading.file)),
        None => Ok(finder.found),
    }
}

/// The crate rooted at `root` (a `lib.rs` or `main.rs`), every file of it that is not
/// gated to the test kit, in the order its modules are declared. `exempt` are files
/// (checkout-relative) whose own sites are not counted: the registry. A file it lists
/// still has its modules read.
pub fn read_crate(
    checkout: &Path,
    root: &Path,
    adapter_paths: bool,
    exempt: &[&str],
    words: &Vocabulary,
) -> Result<Sites, String> {
    let mut sites = Sites::new();
    let children = root.parent().map(Path::to_path_buf).unwrap_or_default();
    read_file(
        checkout,
        root,
        &children,
        adapter_paths,
        exempt,
        words,
        &mut sites,
    )?;
    Ok(sites)
}

fn read_file(
    checkout: &Path,
    file: &Path,
    children: &Path,
    adapter_paths: bool,
    exempt: &[&str],
    words: &Vocabulary,
    sites: &mut Sites,
) -> Result<(), String> {
    let name = relative(checkout, file);
    let source = fs::read_to_string(file).map_err(|failed| format!("{name}: {failed}"))?;
    let read = read(
        &source,
        Reading {
            file: &name,
            adapter_paths,
        },
        words,
    )?;
    if !exempt.contains(&name.as_str()) {
        for (site, count) in read.sites {
            *sites.entry(site).or_default() += count;
        }
    }
    for module in &read.modules {
        let mut folder = children.to_path_buf();
        folder.extend(&module.inside);
        let flat = folder.join(format!("{}.rs", module.name));
        let nested = folder.join(&module.name).join("mod.rs");
        let (path, below) = if flat.is_file() {
            (flat, folder.join(&module.name))
        } else if nested.is_file() {
            (nested, folder.join(&module.name))
        } else {
            return Err(format!(
                "{name}: `mod {};` is in neither {} nor {}",
                module.name,
                relative(checkout, &flat),
                relative(checkout, &nested)
            ));
        };
        read_file(checkout, &path, &below, adapter_paths, exempt, words, sites)?;
    }
    Ok(())
}

/// `path` from `checkout`, with `/` between folders.
pub fn relative(checkout: &Path, path: &Path) -> String {
    let inside = path.strip_prefix(checkout).unwrap_or(path);
    inside
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a pattern or an operand names.
#[derive(Debug, Default, Clone, Copy)]
struct Hit {
    variant: bool,
    id: bool,
}

impl Hit {
    fn or(self, other: Hit) -> Hit {
        Hit {
            variant: self.variant || other.variant,
            id: self.id || other.id,
        }
    }
}

/// The names a file gives `Harness` and its variants.
struct Names<'a> {
    words: &'a Vocabulary,
    /// What the file calls the enum: `Harness`, and what it is imported or aliased as.
    harness: BTreeSet<String>,
    /// The variants it imports bare, by the name it has them as.
    variants: BTreeSet<String>,
    /// What `use` and `type` declared, to be resolved once all are seen: a rename can
    /// come before the import of what it renames.
    uses: Vec<(Vec<String>, Option<String>, bool)>,
    aliases: Vec<(String, Vec<String>)>,
    /// The strings the file gives a name (`const CLAUDE: &str = "claude-code"`), which it
    /// compares with by that name.
    strings: BTreeMap<String, String>,
}

impl<'a> Names<'a> {
    fn new(words: &'a Vocabulary) -> Self {
        Self {
            words,
            harness: BTreeSet::from(["Harness".to_owned()]),
            variants: BTreeSet::new(),
            uses: Vec::new(),
            aliases: Vec::new(),
            strings: BTreeMap::new(),
        }
    }

    /// Settles the names, once the file's `use` and `type` are all seen.
    fn settle(&mut self) {
        loop {
            let before = (self.harness.len(), self.variants.len());
            for (path, rename, glob) in self.uses.clone() {
                let Some(last) = path.last() else {
                    continue;
                };
                let parent = path.len().checked_sub(2).map(|at| &path[at]);
                if glob {
                    if self.harness.contains(last) {
                        self.variants.extend(self.words.variants.iter().cloned());
                    }
                } else if parent.is_some_and(|parent| self.harness.contains(parent))
                    && self.words.variants.contains(last)
                {
                    self.variants.insert(rename.unwrap_or_else(|| last.clone()));
                } else if self.harness.contains(last) {
                    self.harness.insert(rename.unwrap_or_else(|| last.clone()));
                }
            }
            for (alias, target) in self.aliases.clone() {
                if target
                    .last()
                    .is_some_and(|last| self.harness.contains(last))
                {
                    self.harness.insert(alias);
                }
            }
            if before == (self.harness.len(), self.variants.len()) {
                break;
            }
        }
    }
}

impl VisitMut for Names<'_> {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        visit_mut::visit_file_mut(self, file);
        self.settle();
    }

    fn visit_item_use_mut(&mut self, item: &mut ItemUse) {
        for leaf in leaves(&item.tree) {
            self.uses.push((leaf.path, leaf.rename, leaf.glob));
        }
    }

    fn visit_item_const_mut(&mut self, item: &mut ItemConst) {
        if let Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        }) = &*item.expr
        {
            self.strings.insert(item.ident.to_string(), text.value());
        }
        visit_mut::visit_item_const_mut(self, item);
    }

    fn visit_item_type_mut(&mut self, item: &mut ItemType) {
        if let Type::Path(target) = &*item.ty {
            let path = target.path.segments.iter().map(|s| s.ident.to_string());
            self.aliases.push((item.ident.to_string(), path.collect()));
        }
        visit_mut::visit_item_type_mut(self, item);
    }
}

/// One name a `use` brings in.
struct Leaf {
    path: Vec<String>,
    rename: Option<String>,
    glob: bool,
}

/// Every name `tree` brings in, with the whole path to it (`use a::{b, c as d, e::*}` is
/// `a::b`, `a::c` renamed `d`, and `a::e` as a glob).
fn leaves(tree: &UseTree) -> Vec<Leaf> {
    fn walk(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<Leaf>) {
        match tree {
            UseTree::Path(step) => {
                prefix.push(step.ident.to_string());
                walk(&step.tree, prefix, out);
                prefix.pop();
            }
            UseTree::Name(name) => {
                let mut path = prefix.clone();
                if name.ident != "self" {
                    path.push(name.ident.to_string());
                }
                out.push(Leaf {
                    path,
                    rename: None,
                    glob: false,
                });
            }
            UseTree::Rename(name) => {
                let mut path = prefix.clone();
                if name.ident != "self" {
                    path.push(name.ident.to_string());
                }
                out.push(Leaf {
                    path,
                    rename: Some(name.rename.to_string()),
                    glob: false,
                });
            }
            UseTree::Glob(_) => out.push(Leaf {
                path: prefix.clone(),
                rename: None,
                glob: true,
            }),
            UseTree::Group(group) => {
                for item in &group.items {
                    walk(item, prefix, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), &mut out);
    out
}

struct Finder<'a> {
    file: &'a str,
    words: &'a Vocabulary,
    names: Names<'a>,
    adapter_paths: bool,
    /// The items the visit is in, outermost first.
    scope: Vec<String>,
    /// The inline modules it is in, outermost first.
    inline: Vec<String>,
    /// Whether `Self` is `Harness`.
    harness_impl: bool,
    found: Read,
    unsupported: Vec<String>,
}

impl Finder<'_> {
    fn record(&mut self, kind: Kind) {
        let scope = self.scope.join("::");
        self.record_in(scope, kind);
    }

    fn record_in(&mut self, scope: String, kind: Kind) {
        let site = Site {
            file: self.file.to_owned(),
            scope,
            kind,
        };
        *self.found.sites.entry(site).or_default() += 1;
    }

    /// Whether `segments` name a variant of `Harness`.
    fn is_variant(&self, segments: &[String]) -> bool {
        match segments {
            [only] => self.names.variants.contains(only),
            [.., parent, last] => {
                self.words.variants.contains(last)
                    && (self.names.harness.contains(parent)
                        || (self.harness_impl && parent == "Self"))
            }
            [] => false,
        }
    }

    fn pat(&self, pat: &Pat) -> Hit {
        let named = |path: &syn::Path| {
            let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            Hit {
                variant: self.is_variant(&segments),
                id: false,
            }
        };
        match pat {
            Pat::Path(path) => named(&path.path),
            Pat::Ident(ident) => {
                let bare = Hit {
                    variant: self.names.variants.contains(&ident.ident.to_string()),
                    id: self.is_named_id(&[ident.ident.to_string()]),
                };
                match &ident.subpat {
                    Some((_, inner)) => bare.or(self.pat(inner)),
                    None => bare,
                }
            }
            Pat::Lit(lit) => self.operand(&Expr::Lit(lit.clone())),
            Pat::Or(or) => or
                .cases
                .iter()
                .fold(Hit::default(), |h, p| h.or(self.pat(p))),
            Pat::Paren(paren) => self.pat(&paren.pat),
            Pat::Reference(reference) => self.pat(&reference.pat),
            Pat::Type(typed) => self.pat(&typed.pat),
            Pat::Slice(slice) => slice
                .elems
                .iter()
                .fold(Hit::default(), |h, p| h.or(self.pat(p))),
            Pat::Tuple(tuple) => tuple
                .elems
                .iter()
                .fold(Hit::default(), |h, p| h.or(self.pat(p))),
            Pat::TupleStruct(tuple) => tuple
                .elems
                .iter()
                .fold(named(&tuple.path), |h, p| h.or(self.pat(p))),
            Pat::Struct(strukt) => strukt
                .fields
                .iter()
                .fold(named(&strukt.path), |h, f| h.or(self.pat(&f.pat))),
            _ => Hit::default(),
        }
    }

    /// Whether `segments` are the name of a string constant of this file that is a harness's
    /// id or kind.
    fn is_named_id(&self, segments: &[String]) -> bool {
        matches!(segments, [name]
            if self.names.strings.get(name).is_some_and(|word| self.words.names(word)))
    }

    /// What `expr` names, as an operand of a comparison: a variant of `Harness`, or a
    /// string that is a harness's id, bare, held by `Some`/`Ok`/`Err`, behind `&` or `*`
    /// or in parentheses, or among the elements of an array.
    fn operand(&self, expr: &Expr) -> Hit {
        match expr {
            Expr::Path(path) => {
                let segments: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                Hit {
                    variant: self.is_variant(&segments),
                    id: self.is_named_id(&segments),
                }
            }
            Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) => Hit {
                variant: false,
                id: self.words.names(&text.value()),
            },
            Expr::Paren(inner) => self.operand(&inner.expr),
            Expr::Group(inner) => self.operand(&inner.expr),
            Expr::Reference(inner) => self.operand(&inner.expr),
            Expr::Unary(inner) => self.operand(&inner.expr),
            Expr::Array(array) => array
                .elems
                .iter()
                .fold(Hit::default(), |hit, element| hit.or(self.operand(element))),
            Expr::Call(call)
                if matches!(&*call.func, Expr::Path(f)
                    if ["Some", "Ok", "Err"].iter().any(|word| f.path.is_ident(word))) =>
            {
                call.args.iter().fold(Hit::default(), |hit, argument| {
                    hit.or(self.operand(argument))
                })
            }
            _ => Hit::default(),
        }
    }

    /// Records a comparison of `operands`, as one of a variant or of an id.
    fn record_comparison<'e>(&mut self, operands: impl IntoIterator<Item = &'e Expr>) {
        let hit = operands
            .into_iter()
            .fold(Hit::default(), |hit, operand| hit.or(self.operand(operand)));
        if hit.variant {
            self.record(Kind::Eq);
        } else if hit.id {
            self.record(Kind::IdEq);
        }
    }

    fn record_pattern(&mut self, hit: Hit, variant: Kind) {
        if hit.variant {
            self.record(variant);
        } else if hit.id {
            self.record(Kind::IdMatch);
        }
    }

    fn adapter_in(&self, path: &[String]) -> bool {
        self.adapter_paths
            && matches!(path, [krate, adapter, ..]
                if krate == "cf_harness" && self.words.ids.contains(adapter))
    }
}

impl VisitMut for Finder<'_> {
    fn visit_item_mut(&mut self, item: &mut Item) {
        if !gated_to_tests(item_attributes(item)) {
            visit_mut::visit_item_mut(self, item);
        }
    }

    fn visit_impl_item_mut(&mut self, item: &mut ImplItem) {
        let attributes: &[Attribute] = match &*item {
            ImplItem::Const(inner) => &inner.attrs,
            ImplItem::Fn(inner) => &inner.attrs,
            ImplItem::Type(inner) => &inner.attrs,
            ImplItem::Macro(inner) => &inner.attrs,
            _ => &[],
        };
        if !gated_to_tests(attributes) {
            visit_mut::visit_impl_item_mut(self, item);
        }
    }

    fn visit_trait_item_mut(&mut self, item: &mut TraitItem) {
        let attributes: &[Attribute] = match &*item {
            TraitItem::Const(inner) => &inner.attrs,
            TraitItem::Fn(inner) => &inner.attrs,
            TraitItem::Type(inner) => &inner.attrs,
            TraitItem::Macro(inner) => &inner.attrs,
            _ => &[],
        };
        if !gated_to_tests(attributes) {
            visit_mut::visit_trait_item_mut(self, item);
        }
    }

    fn visit_item_mod_mut(&mut self, item: &mut ItemMod) {
        if item.attrs.iter().any(|a| a.path().is_ident("path")) {
            self.unsupported.push(format!(
                "`mod {}` has a #[path], which is not read",
                item.ident
            ));
        }
        let name = item.ident.to_string();
        if item.content.is_none() {
            self.found.modules.push(Module {
                inside: self.inline.clone(),
                name,
            });
            return;
        }
        self.inline.push(name.clone());
        self.scope.push(name);
        visit_mut::visit_item_mod_mut(self, item);
        self.scope.pop();
        self.inline.pop();
    }

    fn visit_item_fn_mut(&mut self, item: &mut ItemFn) {
        self.scope.push(item.sig.ident.to_string());
        visit_mut::visit_item_fn_mut(self, item);
        self.scope.pop();
    }

    fn visit_impl_item_fn_mut(&mut self, item: &mut syn::ImplItemFn) {
        self.scope.push(item.sig.ident.to_string());
        visit_mut::visit_impl_item_fn_mut(self, item);
        self.scope.pop();
    }

    fn visit_trait_item_fn_mut(&mut self, item: &mut syn::TraitItemFn) {
        self.scope.push(item.sig.ident.to_string());
        visit_mut::visit_trait_item_fn_mut(self, item);
        self.scope.pop();
    }

    fn visit_item_impl_mut(&mut self, item: &mut ItemImpl) {
        let target = match &*item.self_ty {
            Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        };
        let before = self.harness_impl;
        self.harness_impl = target
            .as_ref()
            .is_some_and(|name| self.names.harness.contains(name));
        self.scope.push(target.unwrap_or_else(|| "_".to_owned()));
        visit_mut::visit_item_impl_mut(self, item);
        self.scope.pop();
        self.harness_impl = before;
    }

    fn visit_item_trait_mut(&mut self, item: &mut ItemTrait) {
        self.scope.push(item.ident.to_string());
        visit_mut::visit_item_trait_mut(self, item);
        self.scope.pop();
    }

    fn visit_item_use_mut(&mut self, item: &mut ItemUse) {
        for leaf in leaves(&item.tree) {
            if self.adapter_in(&leaf.path) {
                let tail = if leaf.glob { "::*" } else { "" };
                self.record_in(format!("use {}{tail}", leaf.path.join("::")), Kind::Adapter);
            }
        }
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if self.adapter_in(&segments) {
            self.record(Kind::Adapter);
        }
        visit_mut::visit_path_mut(self, path);
    }

    fn visit_expr_match_mut(&mut self, node: &mut ExprMatch) {
        let hit = node
            .arms
            .iter()
            .fold(Hit::default(), |hit, arm| hit.or(self.pat(&arm.pat)));
        self.record_pattern(hit, Kind::Match);
        visit_mut::visit_expr_match_mut(self, node);
    }

    fn visit_expr_let_mut(&mut self, node: &mut ExprLet) {
        let hit = self.pat(&node.pat);
        self.record_pattern(hit, Kind::IfLet);
        visit_mut::visit_expr_let_mut(self, node);
    }

    fn visit_local_mut(&mut self, node: &mut Local) {
        if node
            .init
            .as_ref()
            .is_some_and(|init| init.diverge.is_some())
        {
            let hit = self.pat(&node.pat);
            self.record_pattern(hit, Kind::IfLet);
        }
        visit_mut::visit_local_mut(self, node);
    }

    fn visit_expr_binary_mut(&mut self, node: &mut ExprBinary) {
        if matches!(node.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            self.record_comparison([&*node.left, &*node.right]);
        }
        visit_mut::visit_expr_binary_mut(self, node);
    }

    fn visit_expr_method_call_mut(&mut self, node: &mut ExprMethodCall) {
        if ["eq", "ne", "contains"]
            .iter()
            .any(|word| node.method == word)
        {
            self.record_comparison(std::iter::once(&*node.receiver).chain(&node.args));
        }
        visit_mut::visit_expr_method_call_mut(self, node);
    }

    fn visit_macro_mut(&mut self, node: &mut Macro) {
        let name = node.path.segments.last().map(|s| s.ident.to_string());
        if name.as_deref() == Some("matches") {
            if let Ok((mut scrutinee, pat, guard)) = node.parse_body_with(matches_arguments) {
                let hit = self.pat(&pat);
                self.record_pattern(hit, Kind::Matches);
                self.visit_expr_mut(&mut scrutinee);
                if let Some(mut guard) = guard {
                    self.visit_expr_mut(&mut guard);
                }
            }
        } else if let Ok(mut arguments) =
            node.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
        {
            for argument in &mut arguments {
                self.visit_expr_mut(argument);
            }
        }
    }
}

/// `matches!`'s arguments: the value, the pattern, and the guard if it has one.
fn matches_arguments(input: syn::parse::ParseStream<'_>) -> syn::Result<(Expr, Pat, Option<Expr>)> {
    let scrutinee: Expr = input.parse()?;
    input.parse::<Token![,]>()?;
    let pat = Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.peek(Token![if]) {
        input.parse::<Token![if]>()?;
        Some(input.parse::<Expr>()?)
    } else {
        None
    };
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok((scrutinee, pat, guard))
}

fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(inner) => &inner.attrs,
        Item::Enum(inner) => &inner.attrs,
        Item::ExternCrate(inner) => &inner.attrs,
        Item::Fn(inner) => &inner.attrs,
        Item::ForeignMod(inner) => &inner.attrs,
        Item::Impl(inner) => &inner.attrs,
        Item::Macro(inner) => &inner.attrs,
        Item::Mod(inner) => &inner.attrs,
        Item::Static(inner) => &inner.attrs,
        Item::Struct(inner) => &inner.attrs,
        Item::Trait(inner) => &inner.attrs,
        Item::TraitAlias(inner) => &inner.attrs,
        Item::Type(inner) => &inner.attrs,
        Item::Union(inner) => &inner.attrs,
        Item::Use(inner) => &inner.attrs,
        _ => &[],
    }
}

/// Whether `attributes` build the item only for the tests or the test kit: `#[test]`, or
/// a `#[cfg]` that needs `test` or `feature = "test-support"`.
fn gated_to_tests(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("test")
            || (attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<Meta>()
                    .is_ok_and(|condition| needs_the_tests(&condition)))
    })
}

fn needs_the_tests(condition: &Meta) -> bool {
    match condition {
        Meta::Path(path) => path.is_ident("test"),
        Meta::NameValue(pair) => {
            pair.path.is_ident("feature")
                && matches!(&pair.value, Expr::Lit(ExprLit { lit: Lit::Str(text), .. })
                    if text.value() == "test-support")
        }
        Meta::List(list) => {
            let Ok(inner) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                inner.iter().any(needs_the_tests)
            } else if list.path.is_ident("any") {
                !inner.is_empty() && inner.iter().all(needs_the_tests)
            } else {
                false
            }
        }
    }
}

/// The root files of the crates of the product under `checkout`, with the name of each
/// crate's folder: every crate in `crates` that is not one of `left_out`, and the app.
pub fn crate_roots(checkout: &Path, left_out: &[&str]) -> Result<Vec<(String, PathBuf)>, String> {
    let mut roots = Vec::new();
    let mut folders = vec![("app".to_owned(), checkout.join("app/src-tauri"))];
    let listing = fs::read_dir(checkout.join("crates")).map_err(|failed| failed.to_string())?;
    for entry in listing {
        let entry = entry.map_err(|failed| failed.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !left_out.contains(&name.as_str()) {
            folders.push((name, entry.path()));
        }
    }
    folders.sort();
    for (name, folder) in folders {
        for root in ["lib.rs", "main.rs"] {
            let path = folder.join("src").join(root);
            if path.is_file() {
                roots.push((name.clone(), path));
            }
        }
    }
    Ok(roots)
}
