use super::*;

use crate::context::Context;

/// The checkout these tests are of.
fn checkout() -> PathBuf {
    Context::new(&Env::default()).unwrap().root
}

#[test]
fn a_build_inherits_no_updater_key_apple_certificate_or_identity_and_is_offline() {
    let base = Env::from_vars([
        ("PATH", "/usr/bin"),
        ("HOME", "/home/someone"),
        ("TAURI_SIGNING_PRIVATE_KEY", "production"),
        ("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "password"),
        (
            "TAURI_SIGNING_PRIVATE_KEY_PATH",
            "/home/someone/.tauri/consensflow.key",
        ),
        ("TAURI_PRIVATE_KEY", "older production"),
        ("APPLE_CERTIFICATE", "certificate"),
        (
            "APPLE_SIGNING_IDENTITY",
            "Developer ID Application: Someone",
        ),
        ("APPLE_ID", "someone@example.com"),
        ("APPLE_API_KEY", "key"),
        ("CSC_LINK", "certificate"),
    ]);
    let env = clean_env(&base);
    let names: Vec<_> = env.iter().map(|(name, _)| name.to_os_string()).collect();
    assert_eq!(names, ["CARGO_NET_OFFLINE", "HOME", "PATH"]);
    assert_eq!(env.text("CARGO_NET_OFFLINE"), Some("true"));
    assert_eq!(env.text("PATH"), Some("/usr/bin"));
    assert_eq!(
        base.text("TAURI_SIGNING_PRIVATE_KEY"),
        Some("production"),
        "the caller's own is not touched"
    );
}

#[test]
fn the_tauri_cli_is_the_one_npm_links_in_the_bin_of_the_checkouts_app() {
    let expected: PathBuf = [
        "a",
        "app",
        "node_modules",
        ".bin",
        if cfg!(windows) { "tauri.cmd" } else { "tauri" },
    ]
    .iter()
    .collect();
    assert_eq!(tauri_bin(Path::new("a")), expected);
}

#[test]
fn names_no_key_of_the_products_and_no_home_folder_to_look_for_one() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("updater_smoke");
    let mut files = vec![folder.with_extension("rs")];
    let mut folders = vec![folder];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.file_name().is_some_and(|name| name != "tests.rs") {
                files.push(path);
            }
        }
    }
    assert!(
        files.len() > 10,
        "the sources of the smoke are read: {files:?}"
    );
    // The sources, not the tests that say what they look for.
    let looks = [
        "home_dir",
        ".tauri/",
        ".tauri'",
        ".tauri\"",
        "TAURI_SIGNING_PRIVATE_KEY",
    ];
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for look in looks {
            assert!(
                !text.contains(look),
                "{} looks where a key lives: {look}",
                file.display()
            );
        }
    }
}

#[test]
fn the_runs_updater_key_is_made_for_the_run_signs_what_it_is_asked_to_and_stays_in_its_folder() {
    let root = checkout();
    if !tauri_bin(&root).exists() {
        eprintln!("skipped: the Tauri CLI is not installed");
        return;
    }
    let env = Env::from_process();
    let folder = tempfile::tempdir().unwrap();
    let first = generate_key(&root, &folder.path().join("first"), &env).unwrap();
    let second = generate_key(&root, &folder.path().join("second"), &env).unwrap();
    assert!(
        first.private_key.starts_with(folder.path()),
        "the key is made where it was told"
    );
    let base64 = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+/=".contains(&byte))
    };
    assert!(
        base64(&first.public_key),
        "a public key is one line of base64"
    );
    assert_ne!(first.public_key, second.public_key, "a new pair each time");

    let archive = folder.path().join("archive.tar.gz");
    fs::write(&archive, "bytes").unwrap();
    let signature = sign_file(&root, &first.private_key, &archive, &env).unwrap();
    assert!(base64(&signature), "a signature is one line of base64");
    assert_eq!(
        signature,
        fs::read_to_string(beside(&archive, ".sig")).unwrap().trim()
    );
    assert_ne!(
        sign_file(&root, &second.private_key, &archive, &env).unwrap(),
        signature,
        "another key, another signature"
    );
}

#[test]
fn a_signer_that_fails_says_what_it_was_asked_and_what_it_said() {
    let root = checkout();
    if !tauri_bin(&root).exists() {
        eprintln!("skipped: the Tauri CLI is not installed");
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let said = sign_file(
        &root,
        &folder.path().join("no-such.key"),
        &folder.path().join("no-such-file"),
        &Env::from_process(),
    )
    .unwrap_err()
    .to_string();
    assert!(said.starts_with("tauri signer sign failed: "), "{said}");
}
