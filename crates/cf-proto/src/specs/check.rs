//! What a person types of specs, checked the same wherever it is said: a
//! project's own folder and variant (the ledger keeps them), and the folder and
//! variant every project uses unless it names its own (the agents file keeps
//! them). Each check answers the words of why it refuses, which the page shows
//! as they are; the one who asked gives them their code.

use super::Variant;

/// The most characters a folder's name may have: far past any a person types.
const FOLDER_MAX: usize = 200;

/// A refusal of a folder, in the words every place that keeps one says it in.
pub fn folder_refused(why: &str) -> String {
    format!("the specs folder is a folder inside the project: {why}")
}

/// `text` as the folder it names: its parts in slashes, with the `.` of a
/// `./` gone. It is a folder under the project's own (a path that is
/// absolute, on a drive or climbs out with `..` is refused, and so is the
/// project's folder itself), and none where it is blank.
pub fn folder(text: &str) -> Result<Option<String>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.encode_utf16().count() > FOLDER_MAX {
        return Err(folder_refused(&format!(
            "its name is at most {FOLDER_MAX} characters"
        )));
    }
    if text.chars().any(char::is_control) {
        return Err(folder_refused("its name has no control characters"));
    }
    let slashed = text.replace('\\', "/");
    let bytes = slashed.as_bytes();
    let on_a_drive =
        bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':');
    if slashed.starts_with('/') || on_a_drive {
        return Err(folder_refused(
            "it is relative to the project, not absolute",
        ));
    }
    if slashed.contains(':') {
        return Err(folder_refused("its name has no `:`"));
    }
    let mut parts = Vec::new();
    for part in slashed.split('/').filter(|part| !matches!(*part, "" | ".")) {
        if part == ".." {
            return Err(folder_refused("it stays inside it, with no `..`"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(folder_refused(
            "it is a folder of the project's, not its own",
        ));
    }
    Ok(Some(parts.join("/")))
}

/// The variant `name` says, or the words of why it is none.
pub fn variant(name: &str) -> Result<Variant, String> {
    Variant::from_name(name).ok_or_else(|| {
        format!(
            "the specs variant is one of {}, not {name:?}",
            Variant::ALL.map(Variant::as_str).join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(text: &str) -> Result<Option<String>, String> {
        folder(text)
    }

    #[test]
    fn a_folder_is_kept_as_the_project_s_own_in_slashes() {
        for (given, kept) in [
            (".specs", ".specs"),
            ("docs/specs", "docs/specs"),
            ("./docs/specs/", "docs/specs"),
            ("docs//specs", "docs/specs"),
            (r"docs\specs", "docs/specs"),
            ("  plans  ", "plans"),
            ("a/./b", "a/b"),
            ("..specs", "..specs"),
            ("café/планы", "café/планы"),
        ] {
            assert_eq!(checked(given), Ok(Some(kept.to_owned())), "{given:?}");
        }
    }

    #[test]
    fn a_blank_folder_is_none_and_the_default_stands() {
        for blank in ["", "   ", "\t "] {
            assert_eq!(checked(blank), Ok(None), "{blank:?}");
        }
    }

    #[test]
    fn a_folder_that_leaves_the_project_is_refused() {
        for (given, why) in [
            ("/etc", "it is relative to the project, not absolute"),
            (r"\etc", "it is relative to the project, not absolute"),
            (
                "//server/share",
                "it is relative to the project, not absolute",
            ),
            (
                r"\\server\share",
                "it is relative to the project, not absolute",
            ),
            ("C:/specs", "it is relative to the project, not absolute"),
            (r"c:\specs", "it is relative to the project, not absolute"),
            ("C:specs", "it is relative to the project, not absolute"),
            ("docs/a:b", "its name has no `:`"),
            ("x:y", "it is relative to the project, not absolute"),
            ("1:y", "its name has no `:`"),
            ("..", "it stays inside it, with no `..`"),
            ("../specs", "it stays inside it, with no `..`"),
            ("docs/../../specs", "it stays inside it, with no `..`"),
            (r"docs\..\specs", "it stays inside it, with no `..`"),
            ("docs/..", "it stays inside it, with no `..`"),
            (".", "it is a folder of the project's, not its own"),
            ("./", "it is a folder of the project's, not its own"),
            ("././", "it is a folder of the project's, not its own"),
            ("a\nb", "its name has no control characters"),
            ("a\0b", "its name has no control characters"),
        ] {
            assert_eq!(
                checked(given),
                Err(format!(
                    "the specs folder is a folder inside the project: {why}"
                )),
                "{given:?}"
            );
        }
        let long = "x".repeat(FOLDER_MAX + 1);
        assert!(checked(&long)
            .unwrap_err()
            .ends_with("at most 200 characters"));
        assert_eq!(
            checked(&"x".repeat(FOLDER_MAX)),
            Ok(Some("x".repeat(FOLDER_MAX)))
        );
    }

    #[test]
    fn a_variant_is_one_of_the_four_names_and_any_other_word_says_which_are() {
        for variant in Variant::ALL {
            assert_eq!(super::variant(variant.as_str()), Ok(variant));
        }
        for word in ["Core", "core-html ", "specmint-core", "all", "", "html"] {
            assert_eq!(
                super::variant(word),
                Err(format!(
                    "the specs variant is one of core, core-html, tdd, tdd-html, not {word:?}"
                )),
                "{word:?}"
            );
        }
    }
}
