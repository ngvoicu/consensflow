use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::*;

/// A manifest that pins each variant at a commit of `digits` hexadecimal digits.
fn manifest(digits: usize) -> String {
    let pinned: Vec<String> = Variant::ALL
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let commit = format!("{index:x}").repeat(digits);
            format!(
                r#""{}": {{ "commit": "{commit}", "files": {{}} }}"#,
                variant.as_str()
            )
        })
        .collect();
    format!("{{ {} }}", pinned.join(", "))
}

/// A `SKILL.md` that says it is `version`, as the vendored ones do.
fn skill(variant: Variant, version: &str) -> String {
    format!(
        "---\nname: {}\ndescription: >\n  Persistent spec management.\nlicense: MIT\nmetadata:\n  author: Someone\n  version: \"{version}\"\n---\n\n# Spec Mint\n",
        variant.skill_name()
    )
}

/// What the build bundles when each variant's `SKILL.md` says what `versions` give.
fn read(versions: [&str; 4], manifest: &str) -> Bundled {
    let skills: BTreeMap<&str, String> = Variant::ALL
        .iter()
        .zip(versions)
        .map(|(variant, version)| (variant.as_str(), skill(*variant, version)))
        .collect();
    bundled_from(manifest, |variant| {
        skills.get(variant.as_str()).map(|text| text.as_bytes())
    })
}

/// Each variant's version, as it was read.
fn versions(bundled: &Bundled) -> Vec<Option<&str>> {
    bundled
        .variants
        .iter()
        .map(|row| row.version.as_deref())
        .collect()
}

#[test]
fn each_variant_says_its_version_its_short_commit_and_what_it_is() {
    let bundled = read(["3.0.0"; 4], &manifest(40));
    let rows: Vec<_> = bundled
        .variants
        .iter()
        .map(|row| {
            (
                &*row.name,
                row.version.as_deref(),
                row.commit.as_deref(),
                &*row.about,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("core", Some("3.0.0"), Some("0000000"), "Markdown specs"),
            (
                "core-html",
                Some("3.0.0"),
                Some("1111111"),
                "HTML spec pages"
            ),
            (
                "tdd",
                Some("3.0.0"),
                Some("2222222"),
                "Test-first Markdown specs"
            ),
            (
                "tdd-html",
                Some("3.0.0"),
                Some("3333333"),
                "Test-first specs as HTML pages"
            ),
        ]
    );
    assert_eq!(bundled.version.as_deref(), Some("3.0.0"));
}

#[test]
fn a_commit_shorter_than_seven_digits_is_said_whole() {
    let bundled = read(["3.0.0"; 4], &manifest(3));
    assert_eq!(bundled.variants[0].commit.as_deref(), Some("000"));
    assert_eq!(bundled.variants[3].commit.as_deref(), Some("333"));
}

#[test]
fn a_version_a_fixture_changes_is_the_version_that_is_read() {
    let before = read(["3.0.0"; 4], &manifest(40));
    let after = read(["3.1.4"; 4], &manifest(40));
    assert_eq!(before.version.as_deref(), Some("3.0.0"));
    assert_eq!(after.version.as_deref(), Some("3.1.4"));
    assert_eq!(versions(&after), [Some("3.1.4"); 4]);
    // One variant ahead of the others: each says its own, and the headline says both.
    let mixed = read(["3.0.0", "3.0.0", "3.0.0", "3.2.0"], &manifest(40));
    assert_eq!(
        versions(&mixed),
        [Some("3.0.0"), Some("3.0.0"), Some("3.0.0"), Some("3.2.0")]
    );
    assert_eq!(mixed.version.as_deref(), Some("3.0.0, 3.2.0"));
}

#[test]
fn the_version_is_the_frontmatters_at_its_top_or_under_metadata_and_nothing_else_is() {
    for (text, version) in [
        ("---\nname: x\nversion: 1.2.3\n---\n", Some("1.2.3")),
        ("---\nname: x\nversion: \"1.2.3\"\n---\n", Some("1.2.3")),
        ("---\nname: x\nversion: '1.2.3'\n---\n", Some("1.2.3")),
        ("---\nname: x\nmetadata:\n  version: \"1.2.3\"\n---\n", Some("1.2.3")),
        ("---\nmetadata:\n\tversion: 4\n---\n", Some("4")),
        // An upstream commit vendored with CRLF reads as one with LF.
        (
            "---\r\nname: x\r\nmetadata:\r\n  version: \"1.2.3\"\r\n---\r\n",
            Some("1.2.3"),
        ),
        // A YAML comment is no part of a key or of its value, and a line of one is none.
        (
            "---\nmetadata: # release information\n  # pinned upstream\n\n  version: \"3.0.0\" # as tagged\n---\n",
            Some("3.0.0"),
        ),
        ("---\n# version: 9.9.9\nversion: 1.0.0\n---\n", Some("1.0.0")),
        // The text of another key that runs on is no version.
        (
            "---\ndescription: >\n  See the docs.\n  version: 9.9.9\nmetadata:\n  version: \"1.2.3\"\n---\n",
            Some("1.2.3"),
        ),
        ("---\ndescription: >\n  version: 9.9.9\n---\n", None),
        // Nothing after the block counts, nor does a block that is not first.
        ("---\nname: x\n---\nversion: 1.2.3\n", None),
        ("\n---\nversion: 1.2.3\n---\n", None),
        ("version: 1.2.3\n", None),
        ("---\nversion:\n---\n", None),
        ("---\nversion: \"\"\n---\n", None),
        ("---\nversion: # none yet\n---\n", None),
        ("", None),
    ] {
        assert_eq!(frontmatter_version(text), version, "{text:?}");
    }
}

/// The `SKILL.md`s of a build whose metadata went wrong in each way it can, but for
/// core's: core-html's is no text, tdd's names no version, and tdd-html has none.
fn broken(variant: Variant) -> Option<&'static [u8]> {
    match variant {
        Variant::Core => Some(b"---\nmetadata:\n  version: \"3.0.0\"\n---\n"),
        Variant::CoreHtml => Some(&[0xff, 0xfe]),
        Variant::Tdd => Some(b"---\nname: specmint-tdd\n---\n# no version\n"),
        Variant::TddHtml => None,
    }
}

#[test]
fn what_the_files_do_not_say_is_unknown_and_every_variant_is_listed_with_what_they_do_say() {
    let bundled = bundled_from(
        r#"{ "core": { "commit": "abcdef0123", "files": {} } }"#,
        broken,
    );
    let names: Vec<_> = bundled.variants.iter().map(|row| &*row.name).collect();
    assert_eq!(names, Variant::ALL.map(Variant::as_str));
    assert_eq!(versions(&bundled), [Some("3.0.0"), None, None, None]);
    let commits: Vec<_> = bundled
        .variants
        .iter()
        .map(|row| row.commit.as_deref())
        .collect();
    assert_eq!(commits, [Some("abcdef0"), None, None, None]);
    // The headline is what the variants that say one say.
    assert_eq!(bundled.version.as_deref(), Some("3.0.0"));
    // The page is told it as nothing: `null`, never a word of the daemon's.
    let told = serde_json::to_value(&bundled).unwrap();
    assert_eq!(
        told["variants"][2],
        json!({ "name": "tdd", "version": null, "commit": null, "about": "Test-first Markdown specs" })
    );
    // None says one, and the manifest is no JSON: nothing is known, and still every variant is there.
    let nothing = bundled_from("not json", |_| None);
    assert_eq!(nothing.version, None);
    assert_eq!(versions(&nothing), [None; 4]);
    assert_eq!(nothing.variants.len(), 4);
    assert_eq!(
        serde_json::to_value(&nothing).unwrap()["version"],
        Value::Null
    );
}

/// The folder the variants are vendored in, from the checkout.
fn vendored() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skill")
        .join("specmint")
}

#[test]
fn what_the_build_bundles_is_what_the_vendored_files_say() {
    let bundled = bundled_specmint();
    let pinned: Value =
        serde_json::from_slice(&fs::read(vendored().join("manifest.json")).unwrap()).unwrap();
    let names: Vec<_> = bundled.variants.iter().map(|row| &*row.name).collect();
    assert_eq!(names, Variant::ALL.map(Variant::as_str));
    for row in &bundled.variants {
        // Read here as a person would: the first `version:` of the file's frontmatter.
        let skill = fs::read_to_string(vendored().join(&row.name).join("SKILL.md")).unwrap();
        let written = skill
            .lines()
            .find_map(|line| line.trim_start().strip_prefix("version:"))
            .map(|version| version.trim().trim_matches('"'))
            .unwrap();
        assert_eq!(row.version.as_deref(), Some(written), "{}", row.name);
        let commit = pinned[&row.name]["commit"].as_str().unwrap();
        assert_eq!(row.commit.as_deref(), Some(&commit[..7]), "{}", row.name);
        assert!(!row.about.is_empty());
    }
    assert!(bundled.version.is_some_and(|version| !version.is_empty()));
}
