//! What the Specmint this build bundles says of itself, for Settings to show:
//! each variant's version (the `version` of its `SKILL.md`'s frontmatter, under
//! `metadata:` or at the top), and the commit its manifest pins it at, short.
//! Both are read from the files the build embeds ([`files`]), so a Specmint
//! that is vendored again shows itself with no word of it written anywhere else.
//! What the files do not say (a `SKILL.md` that is not text or names no version,
//! a manifest that pins no commit) is unknown, and is said so: the page asks for
//! this with the human's settings for specs, which no metadata may keep from them.

use cf_proto::specs::{Bundled, BundledVariant, Variant};

use super::files;

/// How many digits of a commit Settings shows.
const SHORT_COMMIT: usize = 7;

/// The Specmint this build bundles.
pub fn bundled_specmint() -> Bundled {
    bundled_from(files::MANIFEST, |variant| {
        files::of(variant)
            .iter()
            .find(|(name, _)| *name == "SKILL.md")
            .map(|(_, bytes)| *bytes)
    })
}

/// [`bundled_specmint`] over `manifest` and the `SKILL.md` bytes `skill` gives
/// for each variant.
fn bundled_from<'a>(manifest: &str, skill: impl Fn(Variant) -> Option<&'a [u8]>) -> Bundled {
    let variants: Vec<BundledVariant> = Variant::ALL
        .into_iter()
        .map(|variant| BundledVariant {
            name: variant.as_str().to_owned(),
            version: skill(variant)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(frontmatter_version)
                .map(str::to_owned),
            commit: files::commit_in(manifest, variant)
                .ok()
                .map(|commit| commit.chars().take(SHORT_COMMIT).collect()),
            about: variant.about().to_owned(),
        })
        .collect();
    let mut versions: Vec<&str> = Vec::new();
    for version in variants.iter().filter_map(|row| row.version.as_deref()) {
        if !versions.contains(&version) {
            versions.push(version);
        }
    }
    Bundled {
        version: (!versions.is_empty()).then(|| versions.join(", ")),
        variants,
    }
}

/// The `version` the frontmatter at the start of `text` gives: a block between
/// `---` lines, at the very start, where the key is at its top or indented
/// under `metadata:`, each line read without the comment YAML lets it end with.
/// A `version:` that is a line of another key's text (a description that runs
/// on) is none.
fn frontmatter_version(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut in_metadata = false;
    for line in lines.map(uncommented) {
        if line.trim_end() == "---" {
            return None;
        }
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        if !indented {
            in_metadata = line.trim_end() == "metadata:";
        }
        if indented && !in_metadata {
            continue;
        }
        if let Some(version) = line.trim_start().strip_prefix("version:") {
            let version = version.trim().trim_matches(['"', '\'']);
            return (!version.is_empty()).then_some(version);
        }
    }
    None
}

/// `line` without its YAML comment: nothing where it is one, else what comes
/// before a `#` that follows a space or a tab.
fn uncommented(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    match line.find(" #").or_else(|| line.find("\t#")) {
        Some(at) => &line[..at],
        None => line,
    }
}

#[cfg(test)]
mod tests;
