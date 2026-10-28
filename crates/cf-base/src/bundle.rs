//! The identities of ConsensFlow's macOS app bundles, and which of them a test may
//! start. The release's identity is the live app's, the board everyone works on: to
//! macOS a copy of the app started with it is that app (its preferences, its web
//! storage, its permissions, its place among the running apps), and on 2026-10-28 an
//! updater smoke's copy of it came to the front over the live app, and the Quit a
//! person gave the one in front reached the live app next. So a test, a smoke or a
//! plant starts only a bundle of another identity: the smoke's own, which every app
//! they build is given, and every copy they make of a built one.

use std::fs;
use std::path::{Path, PathBuf};

/// The identity of the released app: the live app's.
pub const RELEASE: &str = "dev.ngvoicu.consensflow";

/// The identity of every app a smoke builds or starts.
pub const SMOKE: &str = "dev.ngvoicu.consensflow.smoke";

const KEY: &str = "<key>CFBundleIdentifier</key>";

/// Where the value of the identity sits in the text of an XML `Info.plist`: the
/// span of the text between `<string>` and `</string>` after its key.
fn identity_span(plist: &str) -> Option<(usize, usize)> {
    let after_key = plist.find(KEY)? + KEY.len();
    let rest = &plist[after_key..];
    let value = after_key + (rest.len() - rest.trim_start().len());
    let start = value + plist[value..].strip_prefix("<string>").map(|_| 8)?;
    let end = start + plist[start..].find("</string>")?;
    Some((start, end))
}

/// The identity the text of an XML `Info.plist` names, if it names one.
pub fn identity_in(plist: &str) -> Option<&str> {
    identity_span(plist).map(|(start, end)| &plist[start..end])
}

/// The text of an XML `Info.plist` with its identity made `identity`: none where it
/// names none.
pub fn with_identity(plist: &str, identity: &str) -> Option<String> {
    let (start, end) = identity_span(plist)?;
    Some(format!("{}{identity}{}", &plist[..start], &plist[end..]))
}

/// The bundle a program is the executable of: `X.app` for `X.app/Contents/MacOS/x`.
pub fn bundle_of(binary: &Path) -> Option<PathBuf> {
    let macos = binary.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|it| it == name);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && app.extension().is_some_and(|it| it == "app"))
    .then(|| app.to_path_buf())
}

/// Whether a test may start the bundle `app`: the identity it has, or why not. The
/// release's identity is refused, and so is a bundle whose identity cannot be read,
/// which may be the release's.
pub fn startable_by_a_test(app: &Path) -> Result<String, String> {
    let plist = app.join("Contents").join("Info.plist");
    let text = fs::read_to_string(&plist).map_err(|cause| {
        format!(
            "{} is not started: its identity cannot be read ({}: {cause}), and it may be the \
             live app's",
            app.display(),
            plist.display()
        )
    })?;
    let identity = identity_in(&text).ok_or_else(|| {
        format!(
            "{} is not started: {} names no identity a test can read, and it may be the live \
             app's",
            app.display(),
            plist.display()
        )
    })?;
    if identity == RELEASE {
        return Err(format!(
            "{} is not started: it is {RELEASE}, the live app's identity. A test starts only \
             an app of its own identity ({SMOKE}): one a smoke built, or a copy it made",
            app.display()
        ));
    }
    Ok(identity.to_owned())
}

/// Whether a test may start the program `binary`: where it is a program of an app bundle
/// that has an `Info.plist`, the bundle must be one a test may start
/// ([`startable_by_a_test`]). A bundle with none has no identity, and is no app's.
pub fn program_startable_by_a_test(binary: &Path) -> Result<(), String> {
    match bundle_of(binary) {
        Some(app) if app.join("Contents").join("Info.plist").exists() => {
            startable_by_a_test(&app).map(|_| ())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(identity: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n  \
             <key>CFBundleExecutable</key>\n  <string>app</string>\n  {KEY}\n  \
             <string>{identity}</string>\n</dict>\n</plist>\n"
        )
    }

    fn bundle(root: &Path, identity: Option<&str>) -> PathBuf {
        let app = root.join("ConsensFlow.app");
        fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        if let Some(identity) = identity {
            fs::write(app.join("Contents").join("Info.plist"), plist(identity)).unwrap();
        }
        app
    }

    #[test]
    fn the_identity_of_a_plist_is_read_and_replaced_where_it_is() {
        let text = plist(RELEASE);
        assert_eq!(identity_in(&text), Some(RELEASE));
        let smoke = with_identity(&text, SMOKE).unwrap();
        assert_eq!(identity_in(&smoke), Some(SMOKE));
        assert_eq!(smoke.replace(SMOKE, RELEASE), text);
        assert_eq!(identity_in("<plist><dict></dict></plist>"), None);
        assert_eq!(with_identity("<plist><dict></dict></plist>", SMOKE), None);
        assert_eq!(identity_in(&format!("{KEY}<integer>1</integer>")), None);
    }

    /// The live app's identity is never started by a test, nor a bundle that does not
    /// say which it is; any other is, the smoke's and the Candidate's.
    #[test]
    fn a_test_starts_no_bundle_of_the_live_apps_identity_nor_one_that_names_none() {
        let root = tempfile::tempdir().unwrap();
        let release = bundle(&root.path().join("release"), Some(RELEASE));
        let said = startable_by_a_test(&release).unwrap_err();
        assert!(said.contains("the live app's identity"), "{said}");
        let binary = release.join("Contents").join("MacOS").join("app");
        assert_eq!(program_startable_by_a_test(&binary), Err(said));
        let unnamed = bundle(&root.path().join("unnamed"), None);
        let said = startable_by_a_test(&unnamed).unwrap_err();
        assert!(said.contains("cannot be read"), "{said}");
        // A program of a bundle with no Info.plist is no app's: it has no identity.
        let helper = unnamed.join("Contents").join("MacOS").join("node");
        assert_eq!(program_startable_by_a_test(&helper), Ok(()));
        let unread = bundle(&root.path().join("unread"), None);
        fs::write(unread.join("Contents").join("Info.plist"), "bplist00").unwrap();
        let binary = unread.join("Contents").join("MacOS").join("app");
        let said = program_startable_by_a_test(&binary).unwrap_err();
        assert!(said.contains("names no identity"), "{said}");
        for identity in [SMOKE, "dev.ngvoicu.consensflow.candidate"] {
            let app = bundle(&root.path().join(identity), Some(identity));
            assert_eq!(startable_by_a_test(&app).as_deref(), Ok(identity));
            let binary = app.join("Contents").join("MacOS").join("app");
            assert_eq!(program_startable_by_a_test(&binary), Ok(()));
        }
        assert_eq!(
            program_startable_by_a_test(Path::new("/usr/bin/true")),
            Ok(())
        );
    }

    #[test]
    fn the_bundle_of_a_program_is_the_app_it_is_the_executable_of() {
        assert_eq!(
            bundle_of(Path::new("/x/ConsensFlow.app/Contents/MacOS/app")),
            Some(PathBuf::from("/x/ConsensFlow.app"))
        );
        assert_eq!(
            bundle_of(Path::new(
                "/x/ConsensFlow.app/Contents/Resources/cli/bin/cf"
            )),
            None
        );
        assert_eq!(bundle_of(Path::new("/x/folder/Contents/MacOS/app")), None);
        assert_eq!(bundle_of(Path::new("app")), None);
    }
}
