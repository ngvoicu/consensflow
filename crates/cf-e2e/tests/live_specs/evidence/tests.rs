use cf_e2e::files;

use super::*;

const SPEC: &str =
    "---\nid: hello\n---\n\n## Phase 1: One [pending]\n\n- [ ] [HI-01] Say hello\n\n## Phase 2: Two [pending]\n\n- [ ] [HI-02] Say goodbye\n";

const REGISTRY: &str = "# Spec Registry\n\n| ID | Title | Status | Priority | Progress | Updated |\n|----|-------|--------|----------|----------|---------|\n| hello | Hello | active | medium | 0/2 | 2026-10-13 |\n";

fn project(written: &[(&str, &str)]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in written {
        files::write(&folder.path().join(path), text).unwrap();
    }
    folder
}

#[test]
fn the_specs_and_the_registry_in_the_specs_folder_are_read_from_where_specmint_keeps_them() {
    let folder = project(&[
        ("docs/specs/registry.md", REGISTRY),
        ("docs/specs/hello/SPEC.md", SPEC),
        ("README.md", "# demo\n"),
    ]);
    let seen = look(folder.path(), "docs/specs").unwrap();
    assert!(seen.folder_exists);
    assert_eq!(seen.registry.as_deref(), Some(REGISTRY));
    assert!(seen.elsewhere.is_empty());
    let found = seen.the_spec("docs/specs").unwrap();
    assert_eq!(found.path, "docs/specs/hello/SPEC.md");
    assert_eq!(found.file, "SPEC.md");
    assert_eq!(found.spec.as_ref().unwrap().id, "hello");
    assert_eq!(found.spec.as_ref().unwrap().phases.len(), 2);
    assert!(seen.nothing_of_specs().is_err());
}

#[test]
fn an_html_spec_is_read_as_html_and_a_file_that_is_no_spec_is_kept_with_what_is_wrong_with_it() {
    let folder = project(&[
        ("specs/a/SPEC.html", "<html></html>"),
        ("specs/b/SPEC.md", "no frontmatter"),
    ]);
    let seen = look(folder.path(), "specs").unwrap();
    let wrong: Vec<(&str, &str)> = seen
        .specs
        .iter()
        .map(|found| {
            (
                found.path.as_str(),
                found.spec.as_ref().unwrap_err().as_str(),
            )
        })
        .collect();
    assert_eq!(
        wrong,
        [
            ("specs/a/SPEC.html", "the spec has no spec-meta"),
            ("specs/b/SPEC.md", "the spec has no id in its frontmatter"),
        ]
    );
}

#[test]
fn a_spec_written_outside_the_specs_folder_is_no_spec_of_it_and_is_said_where_it_is() {
    // The chief took Specmint's `.specs` for the folder it was told to use instead.
    let folder = project(&[
        (".specs/registry.md", REGISTRY),
        (".specs/hello/SPEC.md", SPEC),
    ]);
    let seen = look(folder.path(), "docs/specs").unwrap();
    assert!(!seen.folder_exists);
    assert!(seen.specs.is_empty() && seen.registry.is_none());
    assert_eq!(
        seen.elsewhere,
        [".specs/hello/SPEC.md", ".specs/registry.md"]
    );
    assert_eq!(
        seen.the_spec("docs/specs").unwrap_err(),
        "docs/specs holds no spec, and these are elsewhere: .specs/hello/SPEC.md, .specs/registry.md"
    );
    assert!(seen
        .nothing_of_specs()
        .unwrap_err()
        .contains(".specs/hello/SPEC.md"));
}

#[test]
fn a_folder_with_no_spec_and_a_folder_with_several_are_each_said() {
    let none = project(&[("README.md", "x")]);
    let seen = look(none.path(), ".specs").unwrap();
    assert_eq!(seen.the_spec(".specs").unwrap_err(), ".specs holds no spec");
    assert!(seen.nothing_of_specs().is_ok());
    assert!(!seen.folder_exists);
    let two = project(&[(".specs/a/SPEC.md", SPEC), (".specs/b/SPEC.md", SPEC)]);
    let seen = look(two.path(), ".specs").unwrap();
    assert_eq!(
        seen.the_spec(".specs").unwrap_err(),
        ".specs holds 2 specs, not one: .specs/a/SPEC.md, .specs/b/SPEC.md"
    );
}

#[test]
fn the_registry_is_the_one_beside_the_specs_and_not_one_a_spec_folder_has_deeper() {
    let folder = project(&[
        (".specs/hello/registry.md", "deeper"),
        (".specs/hello/SPEC.md", SPEC),
    ]);
    let seen = look(folder.path(), ".specs").unwrap();
    assert_eq!(seen.registry, None);
    assert_eq!(seen.specs.len(), 1);
}

#[test]
fn the_git_folder_is_not_looked_into_and_a_trailing_slash_names_the_same_folder() {
    let folder = project(&[(".git/hooks/SPEC.md", SPEC), (".specs/hello/SPEC.md", SPEC)]);
    let seen = look(folder.path(), ".specs/").unwrap();
    assert!(seen.elsewhere.is_empty());
    assert_eq!(seen.specs.len(), 1);
}
