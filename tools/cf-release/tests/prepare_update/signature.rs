//! The signature and the notes, as the command takes them: the shape of what the
//! signer writes, and the real signer to say what that is.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cf_base::env::Env;
use cf_release::process;

use super::fixture::{signature_for, Release};

const OUTER: &str = "signature is not outer-base64 minisign text\n";
const ENVELOPE: &str = "signature is not a Tauri minisign envelope\n";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn rejects_blank_and_malformed_signatures() {
    let release = Release::new();
    for text in [
        "",
        "   \n",
        "not a signature!!!\n",
        "short\n",
        "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkK\ndW50cnVzdGVk\n",
    ] {
        fs::write(&release.signature, text).unwrap();
        release.run(&[]).refused(OUTER);
    }
}

#[test]
fn rejects_text_that_is_base64_and_not_what_the_signer_writes() {
    let release = Release::new();
    // The signer's text with its last line gone.
    let signed =
        String::from_utf8(STANDARD.decode(signature_for("a.app.tar.gz")).unwrap()).unwrap();
    let three: Vec<&str> = signed.lines().take(3).collect();
    for text in [
        STANDARD.encode("x".repeat(120)),
        STANDARD.encode(format!("{}\n", three.join("\n"))),
        STANDARD.encode("a\nb\nc\nd\n".repeat(20)),
    ] {
        fs::write(&release.signature, text).unwrap();
        release.run(&[]).refused(ENVELOPE);
    }
}

#[test]
fn rejects_a_signature_that_cannot_be_read() {
    let release = Release::new();
    fs::remove_file(&release.signature).unwrap();
    release.run(&[]).refused(&format!(
        "could not read signature: {}: No such file or directory",
        release.signature.display()
    ));
}

#[test]
fn carries_the_signature_without_the_blank_the_file_has_around_it() {
    let release = Release::new();
    let signature = fs::read_to_string(&release.signature).unwrap();
    fs::write(
        &release.signature,
        format!("\u{feff}\n\n  {}\r\n\t\n", signature.trim()),
    )
    .unwrap();
    release.run(&[]).finished();
    assert_eq!(
        release.entry()["platforms"]["darwin-aarch64"]["signature"],
        signature.trim()
    );
}

#[test]
fn takes_what_the_real_tauri_signer_writes() {
    let signer = root()
        .join("app")
        .join("node_modules")
        .join(".bin")
        .join("tauri");
    assert!(
        signer.exists(),
        "the signer is {}: `npm ci --prefix app` puts it there",
        signer.display()
    );
    let release = Release::new();
    let key = release.dir.path().join("test-signing-key");
    // The signer takes the key it is given in its environment before the one it is
    // told of: this run is to sign with its own.
    let env = Env::from_vars(
        Env::from_process()
            .iter()
            .filter(|(name, _)| {
                !name
                    .to_string_lossy()
                    .starts_with("TAURI_SIGNING_PRIVATE_KEY")
            })
            .map(|(name, value)| (name.to_os_string(), value.to_os_string())),
    );
    let sign = |words: &[&str], file: &Path| {
        let mut args: Vec<OsString> = words.iter().map(OsString::from).collect();
        args.push(file.into());
        let ran = process::capture(signer.as_os_str(), &args, &env).unwrap();
        assert_eq!(ran.code, 0, "{}", ran.stderr);
    };
    sign(
        &[
            "signer",
            "generate",
            "--ci",
            "--password",
            "",
            "--write-keys",
        ],
        &key,
    );
    let sign_with_key = ["signer", "sign", "--password", "", "--private-key-path"];
    let mut args: Vec<OsString> = sign_with_key.iter().map(OsString::from).collect();
    args.extend([
        key.into_os_string(),
        release.archive.clone().into_os_string(),
    ]);
    let signed = process::capture(signer.as_os_str(), &args, &env).unwrap();
    assert_eq!(signed.code, 0, "{}", signed.stderr);

    // It wrote `<archive>.sig`, where the release keeps the signature.
    assert_eq!(
        release.signature,
        PathBuf::from(format!("{}.sig", release.archive.display()))
    );
    release.run(&[]).finished();
    assert_eq!(
        release.entry()["platforms"]["darwin-aarch64"]["signature"],
        fs::read_to_string(&release.signature).unwrap().trim()
    );
}
