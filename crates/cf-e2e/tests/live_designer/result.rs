//! Whether the designer's result names the file it saved. The role text asks
//! for the file's whole path, one to a line; what a chief needs is to find the
//! file, so a result that names the path the task gave, from the project's
//! folder, is told apart from one that names the whole path and from one that
//! names neither.

use std::path::Path;

/// How a result names a saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    /// By its whole path, as the role text asks, in either spelling the system
    /// has for it (macOS's temporary folder is a link: `/var/…` is `/private/var/…`).
    Whole,
    /// By the path the task gave, from the project's folder.
    Relative,
    /// Not at all.
    Nowhere,
}

/// How `result` names the file `saved`, which the task asked for as `relative`
/// (with `/` between folders) from the project's folder.
pub fn named(result: &str, saved: &Path, relative: &str) -> Named {
    let text = plain(result);
    let canonical = std::fs::canonicalize(saved).ok();
    let whole = std::iter::once(saved)
        .chain(canonical.as_deref())
        .any(|spelling| text.contains(&plain(&spelling.to_string_lossy())));
    if whole {
        Named::Whole
    } else if text.contains(&plain(relative)) {
        Named::Relative
    } else {
        Named::Nowhere
    }
}

/// A path, or text that holds one, as a single spelling: `/` between folders,
/// without the prefix Windows gives a path it has made whole, and on Windows
/// without a difference of case.
fn plain(text: &str) -> String {
    let slashed = text.replace('\\', "/").replace("//?/", "");
    if cfg!(windows) {
        slashed.to_lowercase()
    } else {
        slashed
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const RELATIVE: &str = "images/honey.png";

    /// A project folder with the image in it.
    fn project() -> (tempfile::TempDir, std::path::PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let saved = folder.path().join("images").join("honey.png");
        fs::create_dir_all(saved.parent().unwrap()).unwrap();
        fs::write(&saved, "").unwrap();
        (folder, saved)
    }

    #[test]
    fn a_result_that_gives_the_whole_path_on_a_line_of_its_own_names_it_whole() {
        let (_folder, saved) = project();
        let result = format!(
            "{}\nA flat honey jar on a cream background.",
            saved.display()
        );
        assert_eq!(named(&result, &saved, RELATIVE), Named::Whole);
        // However it is dressed: in code marks, in a sentence, after a bullet.
        for dressed in [
            format!("`{}`", saved.display()),
            format!("Saved to {}.", saved.display()),
            format!("- {}", saved.display()),
        ] {
            assert_eq!(named(&dressed, &saved, RELATIVE), Named::Whole, "{dressed}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_spelled_as_the_system_does_is_the_same_path_when_the_folder_is_a_link() {
        let (real, _) = project();
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        // The test names the file through the link; the designer, whose folder
        // the system had made whole, names it as the system spells it.
        let saved = link.join("images").join("honey.png");
        let system = fs::canonicalize(&saved).unwrap();
        assert_ne!(system, saved);
        assert_eq!(
            named(&system.to_string_lossy(), &saved, RELATIVE),
            Named::Whole
        );
        assert_eq!(
            named(&saved.to_string_lossy(), &saved, RELATIVE),
            Named::Whole
        );
    }

    #[test]
    fn a_result_that_gives_the_path_the_task_gave_names_it_from_the_projects_folder() {
        let (_folder, saved) = project();
        // Whichever separator it is written with.
        for result in [
            "images/honey.png\nA honey jar.",
            "I saved it as `images/honey.png`.",
            r"images\honey.png",
        ] {
            assert_eq!(named(result, &saved, RELATIVE), Named::Relative, "{result}");
        }
    }

    #[test]
    fn a_result_with_no_path_in_it_names_nothing() {
        let (_folder, saved) = project();
        for result in [
            "",
            "Done. The image is a honey jar.",
            "/somewhere/else/honey.png",
            "images/other.png",
        ] {
            assert_eq!(
                named(result, &saved, RELATIVE),
                Named::Nowhere,
                "{result:?}"
            );
        }
    }

    #[test]
    fn a_path_has_one_spelling_for_a_slash_a_backslash_and_windows_verbatim_prefix() {
        assert_eq!(plain(r"a\b/c"), "a/b/c");
        assert_eq!(plain("/var/x"), "/var/x");
        // The prefix Windows gives a path it made whole, and the case it ignores there.
        let verbatim = plain(r"\\?\C:\Users\Zee");
        assert_eq!(
            verbatim,
            if cfg!(windows) {
                "c:/users/zee"
            } else {
                "C:/Users/Zee"
            }
        );
    }
}
