//! The four Specmint variants, as this build holds them: each variant's shipped
//! files (`SKILL.md`, its licence, `references`, and `assets` for the HTML
//! ones), by their names under the variant's folder and their bytes. They are
//! vendored in the repository (`skill/specmint/<variant>`, with the upstream
//! commit and the hash of each file in `manifest.json`), so an installed app and
//! a standalone binary have them with no checkout and no download. A test holds
//! this table to that folder and to the manifest, file for file. The manifest
//! is held too, and says which upstream commit each variant is vendored at
//! ([`commit_of`]).

use cf_proto::specs::Variant;
use serde_json::Value;

/// A file of a variant, by its name under the variant's folder and its bytes.
pub(crate) type Shipped = (&'static str, &'static [u8]);

/// The manifest that pins the variants: each one's upstream commit and the hash
/// of each of its files.
const MANIFEST: &str = include_str!("../../../../../skill/specmint/manifest.json");

/// The files of the variant `$variant`, embedded from its folder.
macro_rules! shipped {
    ($variant:literal: $($file:literal),+ $(,)?) => {
        &[$((
            $file,
            include_bytes!(concat!("../../../../../skill/specmint/", $variant, "/", $file)),
        )),+]
    };
}

const CORE: &[Shipped] = shipped!("core":
    "LICENSE",
    "SKILL.md",
    "references/command-contracts.md",
    "references/researcher.md",
    "references/spec-format.md",
);

const CORE_HTML: &[Shipped] = shipped!("core-html":
    "LICENSE",
    "SKILL.md",
    "assets/spec-runtime.js",
    "assets/spec-styles.css",
    "references/command-contracts.md",
    "references/edit-recipes.md",
    "references/html-template.html",
    "references/mockup-library.md",
    "references/researcher.md",
    "references/spec-format.md",
    "references/validate.md",
    "references/wireframe-library.md",
);

const TDD: &[Shipped] = shipped!("tdd":
    "LICENSE",
    "SKILL.md",
    "references/command-contracts.md",
    "references/researcher.md",
    "references/spec-format.md",
    "references/testing-knowledge.md",
);

const TDD_HTML: &[Shipped] = shipped!("tdd-html":
    "LICENSE",
    "SKILL.md",
    "assets/spec-runtime.js",
    "assets/spec-styles.css",
    "references/command-contracts.md",
    "references/edit-recipes.md",
    "references/html-template.html",
    "references/mockup-library.md",
    "references/researcher.md",
    "references/spec-format.md",
    "references/testing-knowledge.md",
    "references/validate.md",
    "references/wireframe-library.md",
);

/// The shipped files of `variant`.
pub(crate) fn of(variant: Variant) -> &'static [Shipped] {
    match variant {
        Variant::Core => CORE,
        Variant::CoreHtml => CORE_HTML,
        Variant::Tdd => TDD,
        Variant::TddHtml => TDD_HTML,
    }
}

/// The upstream commit this build vendors `variant` at, as its manifest pins it.
pub(crate) fn commit_of(variant: Variant) -> Result<String, String> {
    commit_in(MANIFEST, variant)
}

/// The commit `manifest` pins `variant` at, or why it pins none. A commit names
/// a folder, so it is the hexadecimal digits of one and nothing else: a path is
/// never made of what a manifest says.
fn commit_in(manifest: &str, variant: Variant) -> Result<String, String> {
    let pinned: Value = serde_json::from_str(manifest)
        .map_err(|error| format!("the Specmint manifest is no JSON: {error}"))?;
    pinned[variant.as_str()]["commit"]
        .as_str()
        .filter(|commit| !commit.is_empty() && commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "the Specmint manifest pins no commit for {}",
                variant.as_str()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variant_is_pinned_at_the_commit_the_manifest_gives_it() {
        let manifest = r#"{ "core": { "commit": "c1c89498aa05", "files": {} }, "tdd": { "commit": "0FEDE124", "files": {} } }"#;
        assert_eq!(
            commit_in(manifest, Variant::Core),
            Ok("c1c89498aa05".to_owned())
        );
        assert_eq!(commit_in(manifest, Variant::Tdd), Ok("0FEDE124".to_owned()));
    }

    #[test]
    fn a_manifest_that_pins_no_commit_a_path_could_be_made_of_says_so() {
        let said = |variant: &str| format!("the Specmint manifest pins no commit for {variant}");
        let manifest = r#"{
            "core": { "commit": "../../elsewhere", "files": {} },
            "tdd": { "commit": "", "files": {} },
            "core-html": { "commit": 7, "files": {} },
            "tdd-html": { "files": {} }
        }"#;
        for variant in Variant::ALL {
            assert_eq!(
                commit_in(manifest, variant),
                Err(said(variant.as_str())),
                "{variant:?}"
            );
        }
        assert_eq!(
            commit_in("{}", Variant::Core),
            Err("the Specmint manifest pins no commit for core".to_owned())
        );
        assert!(commit_in("not json", Variant::Core)
            .unwrap_err()
            .starts_with("the Specmint manifest is no JSON: "));
    }
}
