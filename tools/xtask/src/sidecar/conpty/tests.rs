use std::ffi::OsString;
use std::fs;

use super::*;
use crate::sidecar::testing::{
    checkout, console_host_package, files_under, read, sha256_of, write, zip_of, Fake, DLL_INSIDE,
    EXE_INSIDE,
};
use crate::sidecar::Platform;

/// The package every test but the first is about: version 9.9.9, whatever its
/// bytes are.
const ARCHIVE: &str = "microsoft.windows.console.conpty.9.9.9.nupkg";
const URL: &str = "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/9.9.9/microsoft.windows.console.conpty.9.9.9.nupkg";

fn words(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// A cache and a folder for the files, beside each other in a temporary folder.
struct Scene {
    _dir: tempfile::TempDir,
    cache: PathBuf,
    into: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            cache: dir.path().join("cache"),
            // Not there yet, and below a folder that is not either.
            into: dir.path().join("out").join("conpty"),
            _dir: dir,
        }
    }

    /// Where the package is kept.
    fn archive(&self) -> PathBuf {
        self.cache.join(ARCHIVE)
    }

    /// `prepare` for `package`, with what it said.
    fn prepare(&self, package: &Package, system: &mut Fake) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let result = prepare(package, &self.cache, &self.into, system, &mut out);
        (result, String::from_utf8(out).unwrap())
    }
}

/// A stand-in whose `curl` writes `bytes`.
fn serving(bytes: &[u8]) -> Fake {
    let mut system = Fake::on(Platform::Windows);
    system.package = bytes.to_vec();
    system
}

#[test]
fn the_pin_is_the_package_microsoft_published() {
    assert_eq!(PINNED.version, "1.25.260930003");
    assert_eq!(
        PINNED.sha256,
        "02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee"
    );
    assert_eq!(
        PINNED.name(),
        "microsoft.windows.console.conpty.1.25.260930003"
    );
    assert_eq!(
        PINNED.archive(),
        "microsoft.windows.console.conpty.1.25.260930003.nupkg"
    );
    assert_eq!(
        PINNED.url(),
        "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.25.260930003/microsoft.windows.console.conpty.1.25.260930003.nupkg"
    );
}

#[test]
fn a_package_is_hashed_as_the_sha256_of_its_bytes_in_lower_case_hex() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn fetches_a_package_that_is_not_kept_and_puts_its_two_files_in_the_folder() {
    let scene = Scene::new();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    let mut system = serving(&bytes);

    let (result, said) = scene.prepare(&package, &mut system);

    result.unwrap();
    // The fetching: curl, told to fail on an HTTP error, to follow redirects and to write the package where it is kept.
    assert_eq!(
        system.lines(),
        [format!("curl -fsSL -o {} {URL}", scene.archive().display())]
    );
    assert_eq!(system.ran[0].cwd, scene.cache);
    // Only the two files of the x64 package, under the names the app has them by.
    assert_eq!(files_under(&scene.into), ["OpenConsole.exe", "conpty.dll"]);
    assert_eq!(
        read(&scene.into.join("conpty.dll")),
        "the console host's dll"
    );
    assert_eq!(
        read(&scene.into.join("OpenConsole.exe")),
        "the console host's exe"
    );
    // Kept for the next time.
    assert_eq!(fs::read(scene.archive()).unwrap(), bytes);
    assert_eq!(
        said,
        format!(
            "fetching {URL}\nconpty: {}\nconpty: {}\n",
            scene.into.join("conpty.dll").display(),
            scene.into.join("OpenConsole.exe").display()
        )
    );
}

#[test]
fn takes_a_kept_package_that_is_the_pinned_one_without_fetching() {
    let scene = Scene::new();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    fs::create_dir_all(&scene.cache).unwrap();
    fs::write(scene.archive(), &bytes).unwrap();
    // Nothing to fetch: a curl that would fail is never started.
    let mut system = serving(b"not what is kept");
    system.curl_status = 22;

    let (result, said) = scene.prepare(&package, &mut system);

    result.unwrap();
    assert!(system.ran.is_empty(), "{:?}", system.lines());
    assert_eq!(files_under(&scene.into), ["OpenConsole.exe", "conpty.dll"]);
    assert!(!said.contains("fetching"), "{said}");
}

#[test]
fn fetches_again_a_kept_package_that_is_not_the_pinned_one() {
    let scene = Scene::new();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    fs::create_dir_all(&scene.cache).unwrap();
    fs::write(scene.archive(), b"a download cut short").unwrap();
    let mut system = serving(&bytes);

    let (result, said) = scene.prepare(&package, &mut system);

    result.unwrap();
    assert_eq!(system.ran.len(), 1, "{:?}", system.lines());
    assert!(said.starts_with(&format!("fetching {URL}\n")), "{said}");
    assert_eq!(fs::read(scene.archive()).unwrap(), bytes);
    assert_eq!(
        read(&scene.into.join("conpty.dll")),
        "the console host's dll"
    );
}

#[test]
fn refuses_a_fetched_package_that_is_not_the_pinned_one_and_deletes_it() {
    let scene = Scene::new();
    let hash = sha256_of(&console_host_package());
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    // What nuget.org served is another package: with the files in it, and not Microsoft's.
    let other = zip_of(&[
        (DLL_INSIDE, b"not Microsoft's dll"),
        (EXE_INSIDE, b"not its exe"),
    ]);
    let mut system = serving(&other);

    let (result, _) = scene.prepare(&package, &mut system);

    assert_eq!(
        result.unwrap_err().to_string(),
        format!(
            "{} is not the package Microsoft published (SHA-256 {}, not {hash}): deleted it; run again",
            scene.archive().display(),
            sha256_of(&other),
        )
    );
    assert!(!scene.archive().exists());
    assert!(!scene.into.exists(), "nothing of it was used");
}

#[test]
fn refuses_a_package_fetched_again_that_is_still_not_the_pinned_one() {
    let scene = Scene::new();
    let hash = sha256_of(&console_host_package());
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    fs::create_dir_all(&scene.cache).unwrap();
    fs::write(scene.archive(), b"a package changed since").unwrap();
    let mut system = serving(b"another one again");

    let (result, _) = scene.prepare(&package, &mut system);

    let said = result.unwrap_err();
    assert!(matches!(said, Error::NotThePackage { .. }), "{said:?}");
    assert_eq!(system.ran.len(), 1, "fetched once, then refused");
    assert!(!scene.archive().exists());
    assert!(!scene.into.exists());
}

#[test]
fn refuses_a_package_that_lacks_a_file_naming_it_and_writes_none_of_them() {
    for (lacking, kept) in [(DLL_INSIDE, EXE_INSIDE), (EXE_INSIDE, DLL_INSIDE)] {
        let scene = Scene::new();
        // The pinned package, by its hash, and without one of the two.
        let bytes = zip_of(&[(kept, b"the one that is there")]);
        let hash = sha256_of(&bytes);
        let package = Package {
            version: "9.9.9",
            sha256: &hash,
        };

        let (result, said) = scene.prepare(&package, &mut serving(&bytes));

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("{} has no {lacking}", scene.archive().display())
        );
        assert!(!scene.into.exists(), "without {lacking}: {said}");
    }
}

#[test]
fn refuses_a_package_that_is_no_zip_saying_so() {
    let scene = Scene::new();
    let bytes = b"not a zip at all";
    let hash = sha256_of(bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };

    let (result, _) = scene.prepare(&package, &mut serving(bytes));

    let said = result.unwrap_err().to_string();
    let start = format!("could not read {} as a zip: ", scene.archive().display());
    assert!(said.starts_with(&start), "{said}");
    assert!(!scene.into.exists());
}

#[test]
fn a_fetch_that_fails_is_the_status_curl_ended_with_and_nothing_is_written() {
    let scene = Scene::new();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    let mut system = serving(&bytes);
    system.curl_status = 22;

    let (result, _) = scene.prepare(&package, &mut system);

    let said = result.unwrap_err();
    assert!(matches!(said, Error::Ended { status: 22, .. }), "{said:?}");
    assert_eq!(
        said.to_string(),
        format!(
            "curl -fsSL -o {} {URL} ended with status 22",
            scene.archive().display()
        )
    );
    assert!(!scene.into.exists());
}

#[test]
fn curl_that_is_not_installed_is_one_error() {
    let scene = Scene::new();
    let hash = sha256_of(&console_host_package());
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    let mut system = Fake::on(Platform::Windows);
    system.missing = vec!["curl".into()];

    let (result, _) = scene.prepare(&package, &mut system);

    assert_eq!(
        result.unwrap_err().to_string(),
        "`curl` was not found: is it installed, and on the PATH?"
    );
}

#[test]
fn replaces_the_files_that_are_in_the_folder_and_leaves_the_others() {
    let scene = Scene::new();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = Package {
        version: "9.9.9",
        sha256: &hash,
    };
    write(&scene.into.join("conpty.dll"), "an older console host");
    write(&scene.into.join("OpenConsole.exe"), "an older one");
    write(&scene.into.join("other.txt"), "not the console host's");

    let (result, _) = scene.prepare(&package, &mut serving(&bytes));

    result.unwrap();
    assert_eq!(
        read(&scene.into.join("conpty.dll")),
        "the console host's dll"
    );
    assert_eq!(
        read(&scene.into.join("OpenConsole.exe")),
        "the console host's exe"
    );
    assert_eq!(
        files_under(&scene.into),
        ["OpenConsole.exe", "conpty.dll", "other.txt"]
    );
}

#[test]
fn into_is_the_one_folder_conpty_takes_and_a_relative_one_is_from_the_root() {
    let (_dir, context) = checkout();
    let root = &context.root;
    let at = |args: &[&str]| target(&context, &words(args));

    assert_eq!(at(&["--into", "out"]).unwrap(), root.join("out"));
    assert_eq!(at(&["--into=out"]).unwrap(), root.join("out"));
    assert_eq!(
        at(&["--into", "app/src-tauri/target/release"]).unwrap(),
        root.join("app/src-tauri/target/release")
    );
    // A folder that is already a whole path is that path.
    let elsewhere = root.join("elsewhere");
    let given = elsewhere.to_str().unwrap();
    assert_eq!(at(&["--into", given]).unwrap(), elsewhere);

    for refused in [
        vec![],
        vec!["--into"],
        vec!["--into", ""],
        vec!["--into="],
        vec!["out"],
        vec!["--out", "x"],
        vec!["--into", "a", "b"],
        vec!["--into=a", "--into=b"],
        vec!["--into", "a", "--into", "b"],
    ] {
        let said = at(&refused).unwrap_err();
        assert!(matches!(said, Failure::Usage(_)), "{refused:?}");
        assert_eq!(
            said.to_string(),
            "conpty takes --into DIR, the folder for the console host's files",
            "{refused:?}"
        );
    }
}

#[test]
fn a_command_line_that_does_not_name_the_folder_is_refused_before_anything_is_done() {
    let (_dir, context) = checkout();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };

    let refused = run(&context, &[], &mut console).unwrap_err();

    assert!(matches!(refused, Failure::Usage(_)));
    assert!(out.is_empty() && err.is_empty());
    assert!(!context.path("app").exists());
}
