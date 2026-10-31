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

/// The name every app a smoke builds or starts goes by, in the menu bar, the Dock and the
/// switcher: never the live app's.
pub const SMOKE_NAME: &str = "ConsensFlow Smoke";

const KEY: &str = "<key>CFBundleIdentifier</key>";

/// Where the value of the identity sits in the text of an XML `Info.plist`.
fn identity_span(plist: &str) -> Option<(usize, usize)> {
    value_span(plist, KEY)
}

/// The value of the text field `key` of an XML `Info.plist` (`CFBundleIconFile`), if it has
/// one.
pub fn value_in<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
    value_span(plist, &format!("<key>{key}</key>")).map(|(start, end)| &plist[start..end])
}

/// The words `plutil` is given, before the plist, to have a bundle whose `Info.plist` is
/// `plist` go by the smoke's name ([`SMOKE_NAME`]) and show no icon of the product's: its
/// names replaced, and its icon's field taken out where it has one (the icon's file is
/// [`value_in`] `CFBundleIconFile`, under `Contents/Resources`, to be taken out too).
pub fn smoke_name_edits(plist: &str) -> Vec<Vec<&'static str>> {
    let mut edits = vec![
        vec!["-replace", "CFBundleName", "-string", SMOKE_NAME],
        vec!["-replace", "CFBundleDisplayName", "-string", SMOKE_NAME],
    ];
    if value_in(plist, "CFBundleIconFile").is_some() {
        edits.push(vec!["-remove", "CFBundleIconFile"]);
    }
    edits
}

/// The icon's file of the bundle `app` whose `Info.plist` is `plist`, where it names one.
pub fn icon_of(app: &Path, plist: &str) -> Option<PathBuf> {
    let icon = value_in(plist, "CFBundleIconFile")?;
    let file = if Path::new(icon).extension().is_some() {
        icon.to_owned()
    } else {
        format!("{icon}.icns")
    };
    Some(app.join("Contents").join("Resources").join(file))
}

/// Where the text value of the field whose key is `key` (written whole) sits in the text of
/// an XML `Info.plist`: the span between `<string>` and `</string>` after its key.
fn value_span(plist: &str, key: &str) -> Option<(usize, usize)> {
    let after_key = plist.find(key)? + key.len();
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

/// Whether a test may start the bundle `app`: the identity it has, or why not. Only the
/// smoke's identity is taken: the release's is the live app's, the Candidate's is the second
/// live app's, and a bundle whose identity cannot be read may be either.
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
             an app of the smoke's identity ({SMOKE}): one a smoke built, or a copy it made",
            app.display()
        ));
    }
    if identity != SMOKE {
        return Err(format!(
            "{} is not started: it is {identity}, and a test starts only an app of the smoke's \
             identity ({SMOKE}). The Candidate is a live app too, with a home, a lock and a \
             roster of its own",
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

/// Whether a test may start `program` with `args`: the program must be one a test may start
/// ([`program_startable_by_a_test`]), and a program that reaches other programs by what its
/// words name (`open`, `osascript`, `launchctl` and the shells) must name no app but the
/// smoke's: not the product by its name, not an identity of ConsensFlow's but the smoke's, and
/// no `.app` whose identity is another's. `killall` and `pkill`, which end processes by name and
/// never the one a test started, are never started.
pub fn command_startable_by_a_test<S: AsRef<std::ffi::OsStr>>(
    program: &Path,
    args: &[S],
) -> Result<(), String> {
    program_startable_by_a_test(program)?;
    words_startable_by_a_test(program, args)
}

/// Whether `program`'s words reach no app a test may not start: the half of
/// [`command_startable_by_a_test`] that is not about the program itself.
pub fn words_startable_by_a_test<S: AsRef<std::ffi::OsStr>>(
    program: &Path,
    args: &[S],
) -> Result<(), String> {
    let name = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if BY_NAME.contains(&name) {
        return Err(format!(
            "{} is not started: it ends processes by their name, never the one a test started, \
             and the live app's are among them",
            program.display()
        ));
    }
    if !BY_WORDS.contains(&name) {
        return Ok(());
    }
    for word in args {
        let word = word.as_ref().to_string_lossy();
        if let Some(why) = reaching(&word) {
            return Err(format!(
                "{} {word:?} is not started: it {why}, and a test reaches no app but the \
                 smoke's ({SMOKE})",
                program.display()
            ));
        }
    }
    Ok(())
}

/// The programs that end processes by name.
const BY_NAME: [&str; 2] = ["killall", "pkill"];

/// The programs that reach apps by what their words name: Launch Services' opener,
/// AppleScript's, launchd's, and the shells, which run any of them.
const BY_WORDS: [&str; 12] = [
    "open",
    "osascript",
    "launchctl",
    "env",
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
];

/// What `word` names that a test may not reach, if anything: an identity of ConsensFlow's
/// but the smoke's, the product by its name, or an app that is not the smoke's (any app in
/// a folder of the system's applications).
fn reaching(word: &str) -> Option<String> {
    let part = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    for (at, _) in word.match_indices(RELEASE) {
        let named: String = word[at..].chars().take_while(|c| part(*c)).collect();
        let named = named.trim_end_matches('.');
        if named != SMOKE {
            return Some(format!("names {named}"));
        }
    }
    const NAME: &str = "ConsensFlow";
    for (at, _) in word.match_indices(NAME) {
        let before = word[..at].chars().next_back();
        let rest = &word[at + NAME.len()..];
        let alone = before.is_none_or(|c| !c.is_alphanumeric())
            && rest.chars().next().is_none_or(|c| !c.is_alphanumeric());
        if alone && !rest.starts_with(".app") {
            return Some("names the product".to_owned());
        }
    }
    for token in word.split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '`')) {
        let Some(at) = token
            .match_indices(".app")
            .map(|(at, _)| at + 4)
            .find(|end| token[*end..].is_empty() || token[*end..].starts_with('/'))
        else {
            continue;
        };
        let app = Path::new(&token[..at]);
        // The system's folders of applications: a smoke's machine has one of its own, which
        // its plist answers for.
        let applications = ["/Applications/", "~/Applications/"]
            .iter()
            .any(|folder| token.starts_with(folder));
        if applications {
            return Some(format!("names {}, an installed app", app.display()));
        }
        if app.join("Contents").join("Info.plist").exists() {
            if let Err(why) = startable_by_a_test(app) {
                return Some(format!("names an app a test may not start ({why})"));
            }
        }
    }
    None
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
        let app = bundle(&root.path().join(SMOKE), Some(SMOKE));
        assert_eq!(startable_by_a_test(&app).as_deref(), Ok(SMOKE));
        let binary = app.join("Contents").join("MacOS").join("app");
        assert_eq!(program_startable_by_a_test(&binary), Ok(()));
        // The installed Candidate is a live app too, with its own home, lock and roster: a test
        // starts no identity but the smoke's.
        for identity in ["dev.ngvoicu.consensflow.candidate", "dev.example.other"] {
            let app = bundle(&root.path().join(identity), Some(identity));
            let said = startable_by_a_test(&app).unwrap_err();
            assert!(
                said.contains("only an app of the smoke's identity"),
                "{said}"
            );
            let binary = app.join("Contents").join("MacOS").join("app");
            assert_eq!(program_startable_by_a_test(&binary), Err(said));
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

    /// The programs that reach others by what their words name are refused where a word
    /// names an app a test may not start; one that names the smoke's, or none, is not.
    #[test]
    fn a_program_that_reaches_apps_by_its_words_names_none_but_the_smokes() {
        let root = tempfile::tempdir().unwrap();
        let release = bundle(&root.path().join("release"), Some(RELEASE));
        let smoke = bundle(&root.path().join("smoke"), Some(SMOKE));
        let refused = |program: &str, args: &[&str]| {
            command_startable_by_a_test(Path::new(program), args).is_err()
        };
        let live = release.to_string_lossy().into_owned();
        let own = smoke.to_string_lossy().into_owned();
        for (program, args) in [
            (
                "/usr/bin/osascript",
                vec![
                    "-e",
                    "tell application id \"dev.ngvoicu.consensflow\" to quit",
                ],
            ),
            (
                "/usr/bin/osascript",
                vec!["-e", "tell app \"ConsensFlow\" to quit"],
            ),
            ("/usr/bin/open", vec!["-b", RELEASE]),
            ("/usr/bin/open", vec!["-a", "ConsensFlow"]),
            ("/usr/bin/open", vec![live.as_str()]),
            ("open", vec!["-g", "/Applications/ConsensFlow.app"]),
            (
                "/bin/launchctl",
                vec!["kickstart", "gui/501/dev.ngvoicu.consensflow.candidate"],
            ),
            (
                "/bin/sh",
                vec!["-c", "osascript -e 'quit app \"ConsensFlow\"'"],
            ),
            ("/bin/zsh", vec!["-c", &format!("open '{live}'")]),
            ("/usr/bin/killall", vec!["sleep"]),
            ("/usr/bin/pkill", vec!["-f", "anything"]),
        ] {
            assert!(refused(program, &args), "{program} {args:?}");
        }
        let machine = root
            .path()
            .join("machine")
            .join("Applications")
            .join("ConsensFlow.app");
        let machine = bundle(machine.parent().unwrap(), Some(SMOKE));
        let machine = machine
            .join("Contents/Resources/cli/bin/cf")
            .to_string_lossy()
            .into_owned();
        for (program, args) in [
            ("/usr/bin/open", vec![own.as_str()]),
            ("/bin/sh", vec!["-c", &format!("'{machine}' setup")]),
            (
                "/usr/bin/open",
                vec!["https://github.com/ngvoicu/consensflow/releases"],
            ),
            (
                "/bin/sh",
                vec![
                    "-c",
                    "cd ~/Projects/ngvoicu/consensflow-trees/x && cargo test",
                ],
            ),
            ("/bin/sh", vec!["-c", &format!("ditto '{own}' /tmp/copy")]),
            (
                "/usr/bin/osascript",
                vec![
                    "-l",
                    "JavaScript",
                    "-e",
                    "$.NSRunningApplication.runningApplicationWithProcessIdentifier(42).hide",
                ],
            ),
            ("/usr/bin/codesign", vec!["--verify", live.as_str()]),
            ("/bin/launchctl", vec!["print", "gui/501"]),
        ] {
            assert!(!refused(program, &args), "{program} {args:?}");
        }
    }
}
