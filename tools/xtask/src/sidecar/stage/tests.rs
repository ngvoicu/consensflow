use std::ffi::OsString;

use super::*;
use crate::sidecar::testing::{
    checkout, console_host_package, files_under, read, sha256_of, write, Fake,
};

fn said(out: Vec<u8>) -> String {
    String::from_utf8(out).unwrap()
}

/// A stand-in for Windows whose `curl` serves the console host's package, and a
/// pin to it: the real package is a download.
fn windows_serving(bytes: &[u8]) -> Fake {
    let mut windows = Fake::on(Platform::Windows);
    windows.package = bytes.to_vec();
    windows
}

#[test]
fn stages_the_cf_as_the_bundles_cli_bin_cf_and_nothing_else_off_windows() {
    for platform in [Platform::MacOs, Platform::Other] {
        let (_dir, context) = checkout();
        let mut system = Fake::on(platform);
        let mut out = Vec::new();

        stage(&context, &conpty::PINNED, &mut system, &mut out).unwrap();

        let resources = context.path("app/src-tauri/resources");
        assert_eq!(files_under(&resources), ["cli/bin/cf"], "{platform:?}");
        let staged = resources.join("cli").join("bin").join("cf");
        assert_eq!(said(out), format!("cf → {}\n", staged.display()));
        // No console host: nothing is fetched where it is not used.
        assert!(
            system.lines().iter().all(|line| !line.starts_with("curl")),
            "{:?}",
            system.lines()
        );
    }
}

#[test]
fn stages_the_console_host_beside_it_on_windows_and_says_each_file() {
    let (_dir, context) = checkout();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = conpty::Package {
        version: "9.9.9",
        sha256: &hash,
    };
    let mut system = windows_serving(&bytes);
    let mut out = Vec::new();

    stage(&context, &package, &mut system, &mut out).unwrap();

    let resources = context.path("app/src-tauri/resources");
    // Exactly what the bundle is made of: the cf, and the two files of the console host.
    assert_eq!(
        files_under(&resources),
        [
            "cli/bin/cf.exe",
            "conpty/OpenConsole.exe",
            "conpty/conpty.dll"
        ]
    );
    assert_eq!(read(&resources.join("cli/bin/cf.exe")), "the new cf");
    assert_eq!(
        read(&resources.join("conpty/conpty.dll")),
        "the console host's dll"
    );
    assert_eq!(
        read(&resources.join("conpty/OpenConsole.exe")),
        "the console host's exe"
    );
    // Said in the order they were staged: the console host's files, then where the cf is.
    let url = "https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/9.9.9/microsoft.windows.console.conpty.9.9.9.nupkg";
    assert_eq!(
        said(out),
        format!(
            "fetching {url}\nconpty: {}\nconpty: {}\ncf → {}\n",
            resources.join("conpty").join("conpty.dll").display(),
            resources.join("conpty").join("OpenConsole.exe").display(),
            resources.join("cli").join("bin").join("cf.exe").display(),
        )
    );
    // The package is kept where the scripts kept it, app/.cache.
    assert!(context
        .path("app/.cache")
        .join("microsoft.windows.console.conpty.9.9.9.nupkg")
        .is_file());
}

#[test]
fn stages_the_copy_in_bin_after_it_was_signed_and_not_the_one_cargo_built() {
    let (_dir, context) = checkout();
    let mut mac = Fake::on(Platform::MacOs);

    stage(&context, &conpty::PINNED, &mut mac, &mut Vec::new()).unwrap();

    let resources = context.path("app/src-tauri/resources");
    // The stand-in's signing adds to the file: the bundle's cf is the signed one.
    assert_eq!(read(&resources.join("cli/bin/cf")), "the new cf signed");
    assert_eq!(read(&context.path("bin/cf")), "the new cf signed");
    // Staged without building again: one build, one signing.
    assert_eq!(mac.lines().len(), 2, "{:?}", mac.lines());
}

#[test]
fn builds_the_cf_for_the_staging_the_way_build_cf_does_and_without_offline() {
    let (_dir, context) = checkout();
    let mut system = Fake::on(Platform::Other);

    stage(&context, &conpty::PINNED, &mut system, &mut Vec::new()).unwrap();

    assert_eq!(
        system.lines(),
        ["cargo build --release --locked -p cf --bin cf"]
    );
}

#[test]
fn replaces_what_was_staged_in_the_cli_folder_and_leaves_the_other_folders() {
    let (_dir, context) = checkout();
    let resources = context.path("app/src-tauri/resources");
    write(&resources.join("cli/bin/cf"), "the cf staged before");
    // What an older staging left beside it, in the folder and below it.
    write(&resources.join("cli/bin/old-tool"), "a leftover");
    write(&resources.join("cli/lib/old.txt"), "a leftover");
    write(&resources.join("other/kept.txt"), "not the cf's");

    stage(
        &context,
        &conpty::PINNED,
        &mut Fake::on(Platform::Other),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(files_under(&resources), ["cli/bin/cf", "other/kept.txt"]);
    assert_eq!(read(&resources.join("cli/bin/cf")), "the new cf");
}

#[test]
fn a_build_that_fails_stages_nothing_and_leaves_what_was_staged_before() {
    for refuse in ["fails", "leaves no cf"] {
        let (_dir, context) = checkout();
        let staged = context.path("app/src-tauri/resources/cli/bin/cf");
        write(&staged, "the cf staged before");
        let mut system = Fake::on(Platform::Other);
        match refuse {
            "fails" => system.cargo_status = 101,
            _ => system.built = None,
        }
        let mut out = Vec::new();

        let result = stage(&context, &conpty::PINNED, &mut system, &mut out);

        assert!(result.is_err(), "a build that {refuse}");
        assert_eq!(
            read(&staged),
            "the cf staged before",
            "a build that {refuse}"
        );
        assert!(out.is_empty(), "a build that {refuse}");
    }
}

#[test]
fn a_console_host_that_cannot_be_fetched_ends_the_staging_before_it_says_where_the_cf_is() {
    let (_dir, context) = checkout();
    let bytes = console_host_package();
    let hash = sha256_of(&bytes);
    let package = conpty::Package {
        version: "9.9.9",
        sha256: &hash,
    };
    let mut system = windows_serving(&bytes);
    system.curl_status = 6;
    let mut out = Vec::new();

    let failed = stage(&context, &package, &mut system, &mut out).unwrap_err();

    assert!(
        matches!(failed, Error::Ended { status: 6, .. }),
        "{failed:?}"
    );
    assert!(!said(out).contains("cf →"));
}

#[test]
fn a_cli_folder_that_cannot_be_removed_is_an_error_naming_it() {
    let (_dir, context) = checkout();
    // A file where the folder goes is not one that can be taken away as a folder.
    let cli = context.path("app/src-tauri/resources/cli");
    write(&cli, "a file where the folder goes");

    let failed = stage(
        &context,
        &conpty::PINNED,
        &mut Fake::on(Platform::Other),
        &mut Vec::new(),
    )
    .unwrap_err();

    let said = failed.to_string();
    assert!(
        said.starts_with(&format!("could not remove {}: ", cli.display())),
        "{said}"
    );
    assert_eq!(read(&cli), "a file where the folder goes");
}

#[test]
fn stage_takes_no_arguments() {
    let (_dir, context) = checkout();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut console = Console {
        out: &mut out,
        err: &mut err,
    };

    let refused = run(&context, &[OsString::from("--offline")], &mut console).unwrap_err();

    assert_eq!(refused.to_string(), "stage takes no arguments");
    assert!(matches!(refused, Failure::Usage(_)));
    assert!(out.is_empty() && err.is_empty());
    assert!(!context.path("bin").exists());
}
