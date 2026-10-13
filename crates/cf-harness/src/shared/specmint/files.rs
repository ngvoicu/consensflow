//! The four Specmint variants, as this build holds them: each variant's shipped
//! files (`SKILL.md`, its licence, `references`, and `assets` for the HTML
//! ones), by their names under the variant's folder and their bytes. They are
//! vendored in the repository (`skill/specmint/<variant>`, with the upstream
//! commit and the hash of each file in `manifest.json`), so an installed app and
//! a standalone binary have them with no checkout and no download. A test holds
//! this table to that folder and to the manifest, file for file.

use cf_proto::specs::Variant;

/// A file of a variant, by its name under the variant's folder and its bytes.
pub(crate) type Shipped = (&'static str, &'static [u8]);

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
