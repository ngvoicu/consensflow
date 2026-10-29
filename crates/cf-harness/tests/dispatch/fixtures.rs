//! What the scan finds in code written to be found, and what it leaves alone. A rule that
//! finds nothing in a tree of today's shape must be shown to find a site where there is one:
//! each case here is a way to tell one harness from another that the list must hold, or an
//! innocent look-alike that it must not.

use std::fs;

use crate::scan::{self, Reading, Vocabulary};

/// The sites in `source` as `scope: kind` (once per count), sorted. `outside` is whether the
/// code is outside `cf-harness`, where paths into adapters count.
fn found_in(source: &str, outside: bool) -> Vec<String> {
    let reading = Reading {
        file: "fixture.rs",
        adapter_paths: outside,
    };
    let read = scan::read(source, reading, &Vocabulary::of_the_product())
        .unwrap_or_else(|failed| panic!("{failed}"));
    let mut sites: Vec<String> = read
        .sites
        .iter()
        .flat_map(|(site, count)| {
            std::iter::repeat_n(format!("{}: {:?}", site.scope, site.kind), *count)
        })
        .collect();
    sites.sort();
    sites
}

fn inside(source: &str) -> Vec<String> {
    found_in(source, false)
}

fn outside(source: &str) -> Vec<String> {
    found_in(source, true)
}

#[test]
fn a_match_on_the_enum_is_found_whatever_the_enum_is_called_in_the_file() {
    assert_eq!(
        inside(
            "use cf_proto::agents::Harness as Kind;
             fn tell(kind: Kind) -> u8 { match kind { Kind::Claude => 1, _ => 0 } }"
        ),
        ["tell: Match"]
    );
    assert_eq!(
        inside(
            "type Which = cf_proto::agents::Harness;
             fn tell(which: Which) -> u8 { match which { Which::Pi | Which::Codex => 1, _ => 0 } }"
        ),
        ["tell: Match"]
    );
    assert_eq!(
        inside("fn tell(h: cf_proto::agents::Harness) -> u8 { match h { cf_proto::agents::Harness::Devin => 1, _ => 0 } }"),
        ["tell: Match"]
    );
}

#[test]
fn a_match_on_variants_imported_bare_is_found_by_the_names_it_imports() {
    assert_eq!(
        inside(
            "use cf_proto::agents::Harness::{Claude, Codex as Gpt};
             fn tell(h: cf_proto::agents::Harness) -> u8 { match h { Gpt => 1, Claude => 2, _ => 0 } }"
        ),
        ["tell: Match"]
    );
    assert_eq!(
        inside(
            "use cf_proto::agents::Harness::*;
             fn tell(h: cf_proto::agents::Harness) -> u8 { match h { Pi => 1, _ => 0 } }"
        ),
        ["tell: Match"]
    );
}

#[test]
fn self_is_the_enum_inside_its_own_impl_and_nowhere_else() {
    assert_eq!(
        inside(
            "impl Harness { fn is_pi(self) -> bool { matches!(self, Self::Pi) } }
             impl Other { fn is_pi(self) -> bool { matches!(self, Self::Pi) } }"
        ),
        ["Harness::is_pi: Matches"]
    );
}

#[test]
fn a_matches_an_if_let_a_while_let_and_a_let_else_are_each_a_site() {
    assert_eq!(
        inside(
            "fn a(h: Harness) -> bool { matches!(h, Harness::Pi | Harness::Devin) }
             fn b(h: Option<Harness>) { if let Some(Harness::Codex) = h {} }
             fn c(mut h: Vec<Harness>) { while let Some(Harness::Claude) = h.pop() {} }
             fn d(h: Harness) { let Harness::Pi = h else { return }; }"
        ),
        ["a: Matches", "b: IfLet", "c: IfLet", "d: IfLet"]
    );
}

#[test]
fn a_comparison_with_a_variant_is_found_on_either_side_and_in_an_option() {
    assert_eq!(
        inside(
            "fn a(h: Harness) -> bool { h == Harness::Pi }
             fn b(h: Harness) -> bool { Harness::Pi != h }
             fn c(h: Option<Harness>) -> bool { h == Some(Harness::Codex) }
             fn d(h: &Harness) -> bool { *h == Harness::Claude }
             fn e(h: &Harness) -> bool { h == &Harness::Claude }"
        ),
        ["a: Eq", "b: Eq", "c: Eq", "d: Eq", "e: Eq"]
    );
}

#[test]
fn a_comparison_with_a_constant_of_the_file_that_holds_an_id_is_a_comparison_with_the_id() {
    assert_eq!(
        inside(
            "const CLAUDE: &str = \"claude-code\";
             const OTHER: &str = \"other\";
             fn a(h: Option<&str>) -> bool { h != Some(CLAUDE) }
             fn b(h: &str) -> u8 { match h { CLAUDE => 1, _ => 0 } }
             fn c(h: &str) -> bool { h == OTHER }"
        ),
        ["a: IdEq", "b: IdMatch"]
    );
}

#[test]
fn a_membership_test_is_a_comparison_with_each_of_its_members() {
    assert_eq!(
        inside(
            "fn a(h: Harness) -> bool { [Harness::Pi, Harness::Devin].contains(&h) }
             fn b(h: Vec<Harness>) -> bool { h.contains(&Harness::Pi) }
             fn c(h: Harness) -> bool { h.eq(&Harness::Pi) }
             fn d(id: &str) -> bool { [\"claude\", \"devin\"].contains(&id) }
             fn e(v: Vec<u8>) -> bool { v.contains(&1) }"
        ),
        ["a: Eq", "b: Eq", "c: Eq", "d: IdEq"]
    );
}

#[test]
fn a_string_comparison_with_a_harness_id_is_found() {
    assert_eq!(
        inside(
            "fn a(id: &str) -> bool { id == \"codex\" }
             fn b(id: &str) -> bool { \"pi\" != id }
             fn c(id: Option<&str>) -> bool { id == Some(\"devin\") }
             fn d(kind: &str) -> bool { kind == \"claude-code\" }"
        ),
        ["a: IdEq", "b: IdEq", "c: IdEq", "d: IdEq"]
    );
}

#[test]
fn a_match_on_harness_ids_as_strings_is_found_in_any_form_a_pattern_takes() {
    assert_eq!(
        inside(
            "fn a(id: &str) -> u8 { match id { \"claude\" => 1, _ => 0 } }
             fn b(id: Option<&str>) -> u8 { match id { Some(\"pi\" | \"devin\") => 1, _ => 0 } }
             fn c(id: Option<&str>) -> u8 { match id { Some(tool @ (\"claude\" | \"devin\")) => tool.len() as u8, _ => 0 } }
             fn d(id: &str) -> bool { matches!(id, \"opencode\") }
             fn e(id: &str) { if let \"codex\" = id {} }"
        ),
        [
            "a: IdMatch",
            "b: IdMatch",
            "c: IdMatch",
            "d: IdMatch",
            "e: IdMatch"
        ]
    );
}

#[test]
fn a_site_inside_a_macro_a_closure_or_a_guard_is_found() {
    assert_eq!(
        inside(
            "fn a(h: Harness) -> String { format!(\"{}\", match h { Harness::Pi => 1, _ => 0 }) }
             fn b(h: Vec<Harness>) -> usize { h.iter().filter(|h| **h == Harness::Pi).count() }
             fn c(h: Harness, on: bool) -> bool { matches!(h, _ if h == Harness::Pi) && on }"
        ),
        ["a: Match", "b: Eq", "c: Eq"]
    );
}

#[test]
fn a_second_site_in_the_same_item_is_a_second_site() {
    let found = inside(
        "fn a(h: Harness) { match h { Harness::Pi => {}, _ => {} } match h { Harness::Codex => {}, _ => {} } }",
    );
    assert_eq!(found, ["a: Match", "a: Match"]);
}

#[test]
fn a_site_is_named_by_the_type_and_the_function_it_is_in() {
    assert_eq!(
        inside(
            "mod inner { impl Adapter for Mine { fn launch(h: Harness) { fn deeper(h: Harness) { let _ = h == Harness::Pi; } } } }"
        ),
        ["inner::Mine::launch::deeper: Eq"]
    );
}

#[test]
fn a_concrete_import_is_found_outside_the_harnesses_crate_only() {
    let source = "use cf_harness::claude::stale_hooks;
                  use cf_harness::{devin, pi::Window};
                  use cf_harness::detect::locations;
                  use cf_harness::codex::*;";
    assert_eq!(
        outside(source),
        [
            "use cf_harness::claude::stale_hooks: Adapter",
            "use cf_harness::codex::*: Adapter",
            "use cf_harness::devin: Adapter",
            "use cf_harness::pi::Window: Adapter",
        ]
    );
    assert_eq!(inside(source), Vec::<String>::new());
}

#[test]
fn a_path_into_an_adapter_is_found_in_a_type_or_a_call_with_no_import() {
    assert_eq!(
        outside(
            "trait Seams { fn claude(&self) -> cf_harness::claude::computer_use::ComputerUse; }
             fn at() { let _ = cf_harness::codex::computer_use::computer_use(); }"
        ),
        ["Seams::claude: Adapter", "at: Adapter"]
    );
}

#[test]
fn what_looks_alike_but_is_no_harness_is_left_alone() {
    assert_eq!(
        outside(
            "enum Mode { Claude, Pi }
             fn a(m: Mode) -> u8 { match m { Mode::Claude => 1, Mode::Pi => 2 } }
             fn b(word: &str) -> bool { word == \"claude-codes\" || word == \"\" }
             fn c(n: u8) -> bool { n == 1 }
             fn d(id: &str) -> u8 { match id { \"other\" => 1, _ => 0 } }
             use cf_harness::detect::locations;
             use cf_harness::contract::Adapter;
             fn e() { let _ = cf_harness::launch::adapter; }
             fn f(h: Harness) -> &'static str { h.as_str() }
             fn g(h: Harness) -> Harness { Harness::Pi }"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn code_built_only_for_the_tests_or_the_test_kit_is_not_read() {
    assert_eq!(
        inside(
            "#[cfg(test)] mod tests { fn a(h: Harness) { let _ = h == Harness::Pi; } }
             #[cfg(feature = \"test-support\")] fn b(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(any(test, feature = \"test-support\"))] fn c(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(all(unix, test))] fn d(h: Harness) { let _ = h == Harness::Pi; }
             #[test] fn e(h: Harness) { let _ = h == Harness::Pi; }
             struct S; impl S { #[cfg(test)] fn f(h: Harness) { let _ = h == Harness::Pi; } }
             trait T { #[cfg(test)] fn g(h: Harness) { let _ = h == Harness::Pi; } }"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn code_built_for_one_system_or_without_the_tests_is_read() {
    assert_eq!(
        inside(
            "#[cfg(unix)] fn a(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(windows)] fn b(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(not(test))] fn c(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(any(test, unix))] fn d(h: Harness) { let _ = h == Harness::Pi; }
             #[cfg(feature = \"local\")] fn e(h: Harness) { let _ = h == Harness::Pi; }"
        ),
        ["a: Eq", "b: Eq", "c: Eq", "d: Eq", "e: Eq"]
    );
}

#[test]
fn a_module_in_its_own_file_is_read_where_rust_looks_for_it_and_the_registry_is_not_counted() {
    let folder = tempfile::tempdir().unwrap();
    let src = folder.path().join("src");
    fs::create_dir_all(src.join("a/deeper")).unwrap();
    fs::create_dir_all(src.join("b")).unwrap();
    let site = "fn f(h: Harness) -> bool { h == Harness::Pi }";
    for (name, text) in [
        ("lib.rs", format!("mod a; mod b; mod registry; #[cfg(test)] mod tests; mod inline_ {{ mod c; }}\n{site}")),
        ("a.rs", format!("mod deeper;\n{site}")),
        ("a/deeper.rs", site.to_owned()),
        ("b/mod.rs", format!("mod d;\n{site}")),
        ("b/d.rs", site.to_owned()),
        ("registry.rs", format!("mod behind;\n{site}")),
        ("registry/behind.rs", site.to_owned()),
        ("inline_/c.rs", site.to_owned()),
    ] {
        let path = src.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // `tests.rs` is not there, as the module is gated to the tests.
    let words = Vocabulary::of_the_product();
    let sites = scan::read_crate(
        folder.path(),
        &src.join("lib.rs"),
        false,
        &["src/registry.rs"],
        &words,
    )
    .unwrap_or_else(|failed| panic!("{failed}"));
    let files: Vec<_> = sites
        .iter()
        .map(|(site, count)| format!("{} {:?} x{count}", site.file, site.kind))
        .collect();
    assert_eq!(
        files,
        [
            "src/a.rs Eq x1",
            "src/a/deeper.rs Eq x1",
            "src/b/d.rs Eq x1",
            "src/b/mod.rs Eq x1",
            "src/inline_/c.rs Eq x1",
            "src/lib.rs Eq x1",
            "src/registry/behind.rs Eq x1",
        ]
    );
}

#[test]
fn a_module_that_is_not_where_it_is_declared_is_an_error_and_never_skipped() {
    let folder = tempfile::tempdir().unwrap();
    let src = folder.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "mod missing;").unwrap();
    let failed = scan::read_crate(
        folder.path(),
        &src.join("lib.rs"),
        false,
        &[],
        &Vocabulary::of_the_product(),
    )
    .unwrap_err();
    assert!(failed.contains("`mod missing;`"), "{failed}");
    // A path attribute moves a module where this does not look: it is refused.
    let refused = scan::read(
        "#[path = \"x.rs\"] mod other;",
        Reading {
            file: "fixture.rs",
            adapter_paths: false,
        },
        &Vocabulary::of_the_product(),
    )
    .unwrap_err();
    assert!(refused.contains("#[path]"), "{refused}");
}

#[test]
fn the_vocabulary_is_the_enum_s_own() {
    let words = Vocabulary::of_the_product();
    assert_eq!(
        words.ids.iter().map(String::as_str).collect::<Vec<_>>(),
        ["claude", "codex", "devin", "opencode", "pi"]
    );
    // Claude's kind is the one word besides an id: the roster and the pages say `claude-code`.
    assert_eq!(
        words.kinds.iter().map(String::as_str).collect::<Vec<_>>(),
        ["claude-code", "codex", "devin", "opencode", "pi"]
    );
    assert!(words.variants.contains("Opencode") && words.variants.contains("Claude"));
}
