//! The app under test, as a bundle on disk: which `.app` the smoke runs on, what
//! it must hold to be run, and what it must not hold. The bundle ships the
//! native `cf` alone, so Node's files, which the releases before it carried, are
//! found anywhere in it and refused.
//!
//! `/Applications` is never a candidate: the installed app is whatever was last
//! installed, and a smoke that passes against it says nothing about the source
//! in this tree.

use std::io::Write;
use std::path::{Path, PathBuf};

use cf_e2e::process::Run;
use cf_e2e::shipped::{Answer, Shipped};
use cf_e2e::{checkout, Error, Result};

/// What builds the bundle the smoke runs on, for the messages that say there is none.
pub const BUILD_HINT: &str = "npm --prefix app run build -- --bundles app";

/// Where a build of this checkout leaves the app, from the checkout's root.
const BUILT: &str = "app/src-tauri/target/release/bundle/macos/ConsensFlow.app";

/// The names of the files of Node's era that a bundle must not hold, wherever
/// in it they are: the runtime a sidecar was, and the script Node's `cf` was.
const NODE_FILES: [&str; 3] = ["node", "node.exe", "cf.mjs"];

/// The folders of Node's `cf` that a bundle must not hold, from `Contents`.
const NODE_FOLDERS: [&[&str]; 2] = [&["Resources", "cli", "src"], &["Resources", "cli", "hosts"]];

/// A bundle that can be run: the app's own program and the `cf` a window runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub app: PathBuf,
    /// What the app is started as.
    pub binary: PathBuf,
    /// The command a window runs, which is also the daemon the app starts.
    pub cf: PathBuf,
    /// Leave to start it, where it is the bundle the release ships ([`to_start`]).
    pub shipped: Option<Shipped>,
}

/// The `cf` of the bundle at `app`.
pub fn cf_of(app: &Path) -> PathBuf {
    ["Contents", "Resources", "cli", "bin", "cf"]
        .iter()
        .fold(app.to_path_buf(), |path, part| path.join(part))
}

/// The bundle to test: the one `named` (a relative path is from the checkout's
/// root), else the one a build of this checkout leaves.
pub fn requested(named: Option<&str>) -> PathBuf {
    match named.filter(|name| !name.is_empty()) {
        Some(name) => checkout::root().join(name),
        None => checkout::path(BUILT),
    }
}

/// The bundle at `app`, or why it cannot be run. Its program is what its
/// `Info.plist` says it is: Tauri names it after the crate, not after the
/// product, and guessing the product's name would report a missing bundle for
/// one that is sitting right there.
pub fn locate(app: &Path) -> Result<Bundle> {
    if !app.exists() {
        return Err(Error::Build(format!(
            "no built bundle at {} — build it with `{BUILD_HINT}`",
            app.display()
        )));
    }
    let binary = app.join("Contents").join("MacOS").join(executable_of(app)?);
    let cf = cf_of(app);
    for (what, path) in [("executable", &binary), ("cf for a window to run", &cf)] {
        if !path.exists() {
            return Err(Error::Build(format!(
                "the bundle at {} has no {what} ({}) — rebuild with `{BUILD_HINT}`",
                app.display(),
                path.display()
            )));
        }
    }
    Ok(Bundle {
        app: app.to_path_buf(),
        binary,
        cf,
        shipped: None,
    })
}

/// The program the bundle's `Info.plist` names (`CFBundleExecutable`), asked of
/// the system's own `plutil`.
fn executable_of(app: &Path) -> Result<String> {
    let unreadable = |why: String| {
        Error::Build(format!(
            "the bundle at {} has no readable Info.plist: {why}",
            app.display()
        ))
    };
    let ran = Run::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(app.join("Contents").join("Info.plist"))
        .run()
        .map_err(|cause| unreadable(cause.to_string()))?;
    if ran.code != Some(0) {
        return Err(unreadable(ran.stderr.trim().to_owned()));
    }
    Ok(ran.stdout.trim().to_owned())
}

/// What of Node's the bundle at `app` holds, each as the path it is at: none,
/// for an app that ships the native `cf` alone.
pub fn node_in(app: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = checkout::files_below(app, &[])?
        .into_iter()
        .filter(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| NODE_FILES.contains(&name))
        })
        .collect();
    for parts in NODE_FOLDERS {
        let folder = parts
            .iter()
            .fold(app.join("Contents"), |path, part| path.join(part));
        if folder.exists() {
            found.push(folder);
        }
    }
    Ok(found)
}

/// Runs `program` with `args` to its end, or says what it said when it failed.
fn ran(program: &str, args: &[&std::ffi::OsStr]) -> Result {
    let ran = Run::new(program).args(args).run()?;
    if ran.code == Some(0) {
        return Ok(());
    }
    Err(Error::Build(format!(
        "{program} {} failed: {}",
        args.iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        ran.stderr.trim()
    )))
}

/// The copy of `bundle` the smoke starts, in `into`: the bundle whole, of the smoke's
/// identity and name, with no icon of the product's, and sealed again as it was (ad hoc, keeping its entitlements, its flags and
/// its hardened runtime). The bundle built is of the live app's identity, and to macOS a
/// copy started with it is the live app, which a test never starts ([`cf_base::bundle`]).
pub fn smoke_copy(bundle: &Bundle, into: &Path) -> Result<Bundle> {
    let name = bundle
        .app
        .file_name()
        .unwrap_or_else(|| "ConsensFlow.app".as_ref());
    // At the path with no link in it: Tauri finds no resources of an app started through one, and
    // macOS's temporary folder is under /var, a link to /private/var.
    let into = std::fs::canonicalize(into)
        .map_err(|source| Error::Build(format!("could not find {}: {source}", into.display())))?;
    let copy = into.join(name);
    ran(
        "/usr/bin/ditto",
        &[bundle.app.as_os_str(), copy.as_os_str()],
    )?;
    let plist = copy.join("Contents").join("Info.plist");
    ran(
        "/usr/bin/plutil",
        &["-convert".as_ref(), "xml1".as_ref(), plist.as_os_str()],
    )?;
    let text = std::fs::read_to_string(&plist)
        .map_err(|source| Error::Build(format!("could not read {}: {source}", plist.display())))?;
    let smoke = cf_base::bundle::with_identity(&text, cf_base::bundle::SMOKE)
        .ok_or_else(|| Error::Build(format!("{} names no identity to replace", plist.display())))?;
    std::fs::write(&plist, smoke)
        .map_err(|source| Error::Build(format!("could not write {}: {source}", plist.display())))?;
    // It goes by the smoke's name, and shows no icon of the product's.
    if let Some(icon) = cf_base::bundle::icon_of(&copy, &text) {
        if icon.exists() {
            std::fs::remove_file(&icon).map_err(|source| {
                Error::Build(format!("could not take out {}: {source}", icon.display()))
            })?;
        }
    }
    for edit in cf_base::bundle::smoke_name_edits(&text) {
        let mut words: Vec<&std::ffi::OsStr> = edit.iter().map(|word| word.as_ref()).collect();
        words.push(plist.as_os_str());
        ran("/usr/bin/plutil", &words)?;
    }
    ran(
        "/usr/bin/codesign",
        &[
            "--force".as_ref(),
            "--deep".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            "--preserve-metadata=entitlements,flags,runtime".as_ref(),
            copy.as_os_str(),
        ],
    )?;
    cf_base::bundle::startable_by_a_test(&copy).map_err(Error::LiveApp)?;
    locate(&copy)
}

/// What the smoke starts of `bundle`: the bundle as it ships, on GitHub's runner where the
/// release run asks for it (`CONSENSFLOW_SMOKE_SHIPPED=1`) and no live app runs; everywhere
/// else a copy of the smoke's identity in `into` ([`smoke_copy`]), and why, where the shipped
/// one was asked for.
pub fn to_start(bundle: &Bundle, into: &Path) -> Result<Bundle> {
    match cf_e2e::shipped::ask(&bundle.app)? {
        Answer::Shipped(leave) => Ok(Bundle {
            shipped: Some(leave),
            ..bundle.clone()
        }),
        Answer::Copy(why) => {
            if let Some(why) = why {
                let _ = writeln!(std::io::stderr(), "smoke: {why}");
            }
            smoke_copy(bundle, into)
        }
    }
}

/// The app's program as a run starts it: with leave for the bundle the release ships, where
/// it is that one.
pub fn run_of(bundle: &Bundle) -> Run {
    let run = Run::new(&bundle.binary);
    match &bundle.shipped {
        Some(leave) => run.shipped(leave),
        None => run,
    }
}

/// Says the bundle's seal holds (`codesign --verify --deep --strict`), as it must after a run.
pub fn assert_sealed(app: &Path) -> Result {
    ran(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            app.as_os_str(),
        ],
    )
}

/// Says the bundle holds no Node, or which of it it does.
pub fn assert_no_node(app: &Path) -> Result {
    let found = node_in(app)?;
    if found.is_empty() {
        return Ok(());
    }
    let paths: Vec<String> = found
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    Err(Error::Build(format!(
        "the bundle at {} holds Node: {} — the app ships the native cf alone",
        app.display(),
        paths.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    use cf_e2e::files;

    /// The `Info.plist` of an app whose program is `executable`.
    fn plist(executable: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict><key>CFBundleExecutable</key>\
             <string>{executable}</string></dict></plist>\n"
        )
    }

    /// A bundle with everything it needs to be run, its program named `app`.
    fn bundle() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("ConsensFlow Candidate.app");
        files::write(&app.join("Contents").join("Info.plist"), plist("app")).unwrap();
        files::write(&app.join("Contents").join("MacOS").join("app"), "").unwrap();
        files::write(&cf_of(&app), "").unwrap();
        (root, app)
    }

    #[test]
    fn the_bundle_is_the_one_named_else_the_one_a_build_leaves_and_a_relative_name_is_from_the_root(
    ) {
        assert_eq!(requested(None), checkout::path(BUILT));
        assert_eq!(requested(Some("")), checkout::path(BUILT));
        assert_eq!(
            requested(Some("dist/ConsensFlow.app")),
            checkout::path("dist/ConsensFlow.app")
        );
        let absolute = std::env::temp_dir().join("ConsensFlow.app");
        assert_eq!(requested(absolute.to_str()), absolute);
    }

    #[test]
    fn a_bundle_is_found_by_the_program_its_plist_names_and_the_cf_a_window_runs() {
        let (_root, app) = bundle();
        let found = locate(&app).unwrap();
        assert_eq!(found.binary, app.join("Contents").join("MacOS").join("app"));
        assert_eq!(found.cf, cf_of(&app));
        assert_eq!(found.app, app);
        // The program is whatever the plist says, not the name of the product.
        files::write(
            &app.join("Contents").join("Info.plist"),
            plist("ConsensFlow"),
        )
        .unwrap();
        let said = locate(&app).unwrap_err().to_string();
        assert!(said.contains("has no executable"), "{said}");
        assert!(said.contains("ConsensFlow"), "{said}");
    }

    #[test]
    fn a_bundle_that_is_not_there_or_cannot_be_run_says_what_is_missing_and_what_builds_it() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("ConsensFlow.app");
        let said = locate(&missing).unwrap_err().to_string();
        assert_eq!(
            said,
            format!(
                "no built bundle at {} — build it with `{BUILD_HINT}`",
                missing.display()
            )
        );

        let (_root, app) = bundle();
        std::fs::remove_file(cf_of(&app)).unwrap();
        let said = locate(&app).unwrap_err().to_string();
        assert!(said.contains("has no cf for a window to run"), "{said}");
        assert!(said.contains(&cf_of(&app).display().to_string()), "{said}");
        assert!(
            said.ends_with(&format!("rebuild with `{BUILD_HINT}`")),
            "{said}"
        );

        let (_root, app) = bundle();
        std::fs::remove_file(app.join("Contents").join("Info.plist")).unwrap();
        let said = locate(&app).unwrap_err().to_string();
        assert!(said.contains("has no readable Info.plist"), "{said}");
    }

    #[test]
    fn a_bundle_that_holds_no_node_is_taken() {
        let (_root, app) = bundle();
        files::write(
            &app.join("Contents").join("Resources").join("icon.icns"),
            "",
        )
        .unwrap();
        assert_eq!(node_in(&app).unwrap(), Vec::<PathBuf>::new());
        assert_no_node(&app).unwrap();
    }

    #[test]
    fn node_is_found_wherever_it_is_in_the_bundle_and_named_with_where() {
        for held in [
            ["Contents", "MacOS", "node"].as_slice(),
            ["Contents", "Resources", "binaries", "node"].as_slice(),
            ["Contents", "Resources", "cli", "bin", "cf.mjs"].as_slice(),
            ["Contents", "Resources", "somewhere", "deeper", "node.exe"].as_slice(),
        ] {
            let (_root, app) = bundle();
            let file = held.iter().fold(app.clone(), |path, part| path.join(part));
            files::write(&file, "").unwrap();
            assert_eq!(
                node_in(&app).unwrap(),
                std::slice::from_ref(&file),
                "{held:?}"
            );
            let said = assert_no_node(&app).unwrap_err().to_string();
            assert!(said.contains(&file.display().to_string()), "{said}");
            assert!(
                said.ends_with("the app ships the native cf alone"),
                "{said}"
            );
        }
    }

    #[test]
    fn the_folders_of_nodes_cf_are_found_and_a_file_that_only_contains_the_name_is_not() {
        let (_root, app) = bundle();
        let src = app
            .join("Contents")
            .join("Resources")
            .join("cli")
            .join("src");
        files::make_dir(&src).unwrap();
        let hosts = app
            .join("Contents")
            .join("Resources")
            .join("cli")
            .join("hosts");
        files::make_dir(&hosts).unwrap();
        assert_eq!(node_in(&app).unwrap(), [src, hosts]);

        let (_root, app) = bundle();
        for name in ["nodes", "node-gyp", "not-node", "cf.mjs.map", "Node"] {
            files::write(&app.join("Contents").join("Resources").join(name), "").unwrap();
        }
        assert_eq!(node_in(&app).unwrap(), Vec::<PathBuf>::new());
    }

    /// The smoke starts a copy of the bundle built, of its own identity, never the bundle:
    /// the copy is whole, sealed, and the bundle is left as it was.
    #[test]
    fn the_smoke_starts_a_sealed_copy_of_its_own_identity_and_leaves_the_bundle_as_it_was() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("built").join("ConsensFlow.app");
        let program = app.join("Contents").join("MacOS").join("app");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::copy("/bin/echo", &program).unwrap();
        std::fs::create_dir_all(cf_of(&app).parent().unwrap()).unwrap();
        std::fs::copy("/bin/echo", cf_of(&app)).unwrap();
        let release = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleExecutable</key><string>app</string>\
             <key>CFBundleIdentifier</key><string>{}</string>\
             <key>CFBundleName</key><string>ConsensFlow</string>\
             <key>CFBundleDisplayName</key><string>ConsensFlow</string>\
             <key>CFBundleIconFile</key><string>icon.icns</string>\
             <key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n",
            cf_base::bundle::RELEASE
        );
        files::write(&app.join("Contents").join("Info.plist"), &release).unwrap();
        files::write(
            &app.join("Contents/Resources/icon.icns"),
            "the product's icon",
        )
        .unwrap();
        ran(
            "/usr/bin/codesign",
            &[
                "--force".as_ref(),
                "--deep".as_ref(),
                "--sign".as_ref(),
                "-".as_ref(),
                app.as_os_str(),
            ],
        )
        .unwrap();
        let built = locate(&app).unwrap();
        assert!(cf_base::bundle::startable_by_a_test(&app).is_err());

        let into = root.path().join("machine");
        std::fs::create_dir_all(&into).unwrap();
        let copy = smoke_copy(&built, &into).unwrap();
        // At the path with no link in it: Tauri finds no resources of an app started through one
        // (macOS's temporary folder is under /var, a link to /private/var).
        assert_eq!(
            copy.app,
            std::fs::canonicalize(&into)
                .unwrap()
                .join("ConsensFlow.app")
        );
        assert_eq!(std::fs::canonicalize(&copy.binary).unwrap(), copy.binary);
        assert_eq!(
            copy.binary,
            copy.app.join("Contents").join("MacOS").join("app")
        );
        assert_eq!(
            cf_base::bundle::startable_by_a_test(&copy.app).as_deref(),
            Ok(cf_base::bundle::SMOKE)
        );
        ran(
            "/usr/bin/codesign",
            &[
                "--verify".as_ref(),
                "--deep".as_ref(),
                "--strict".as_ref(),
                copy.app.as_os_str(),
            ],
        )
        .unwrap();
        // It goes by the smoke's name, and has no icon of the product's.
        let copied = std::fs::read_to_string(copy.app.join("Contents").join("Info.plist")).unwrap();
        assert!(
            copied.matches("<string>ConsensFlow Smoke</string>").count() == 2
                && !copied.contains("CFBundleIconFile"),
            "{copied}"
        );
        assert!(!copy.app.join("Contents/Resources/icon.icns").exists());
        assert_eq!(
            std::fs::read_to_string(app.join("Contents").join("Info.plist")).unwrap(),
            release,
            "the bundle built is left as it was"
        );
        assert!(app.join("Contents/Resources/icon.icns").exists());
    }
}
