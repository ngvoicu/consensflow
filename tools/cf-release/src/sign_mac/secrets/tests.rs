//! The identity of the release, as the environment holds it and as the disk gets
//! it, and the blanking of every secret out of a text.

use std::fs;

use super::*;

/// Base64 as the system's tool writes it, padded.
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (place, byte)| {
            group | u32::from(*byte) << (16 - 8 * place)
        });
        for place in 0..4 {
            if place <= chunk.len() {
                let six = usize::try_from((group >> (18 - 6 * place)) & 63).unwrap();
                text.push(char::from(ALPHABET[six]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

fn credentials(vars: &[(&str, &str)]) -> Result<Credentials, Failure> {
    Credentials::from_env(&Env::from_vars(vars.iter().copied()))
}

const ALL: [(&str, &str); 5] = [
    (CERTIFICATE, "cGtjczEy"),
    (CERTIFICATE_PASSWORD, "p12 password"),
    (
        API_KEY,
        "-----BEGIN PRIVATE KEY-----\nMIGTAgEA\n-----END PRIVATE KEY-----\n",
    ),
    (API_KEY_ID, "KEYID12345"),
    (API_ISSUER, "69a6de70-1111-47e3"),
];

#[test]
fn base64_is_read_as_node_reads_it_padded_or_not_with_white_space_anywhere() {
    for (text, bytes) in [
        ("", ""),
        ("Zg==", "f"),
        ("Zg", "f"),
        ("Zm8=", "fo"),
        ("Zm8", "fo"),
        ("Zm9v", "foo"),
        ("Zm9vYg==", "foob"),
        ("Zm9vYmE=", "fooba"),
        ("Zm9vYmFy", "foobar"),
        (" Zm9v\r\nYmFy\n", "foobar"),
        ("Z m 9 v", "foo"),
    ] {
        assert_eq!(
            decode_base64(text),
            Some(bytes.as_bytes().to_vec()),
            "{text:?}"
        );
    }
}

#[test]
fn base64_is_read_whatever_its_length_and_wherever_it_is_wrapped() {
    let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    let text = encode(&bytes);
    assert_eq!(decode_base64(&text), Some(bytes.clone()));
    // As `base64` wraps it at 76 columns, and as the web's alphabet spells it.
    let wrapped = text
        .as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(decode_base64(&wrapped), Some(bytes.clone()));
    assert_eq!(
        decode_base64(&text.replace('+', "-").replace('/', "_")),
        Some(bytes)
    );
}

#[test]
fn what_is_not_base64_is_refused_and_so_is_a_last_character_with_no_byte_to_make() {
    for text in ["Zm9v!", "Zm9v=Zm9v", "Zm9vY", "Zm9v\u{e9}", "=Z", "Z"] {
        assert_eq!(decode_base64(text), None, "{text:?}");
    }
}

#[test]
fn the_identity_is_taken_from_the_environment_and_lacking_a_variable_names_it() {
    let found = credentials(&ALL).unwrap();
    assert_eq!(found.certificate_password(), "p12 password");
    assert_eq!(found.key_id(), "KEYID12345");
    assert_eq!(found.issuer(), "69a6de70-1111-47e3");

    let lacking = |names: &[&str]| -> Vec<(&str, &str)> {
        ALL.iter()
            .copied()
            .filter(|(name, _)| !names.contains(name))
            .collect()
    };
    let refused = |names: &[&str]| credentials(&lacking(names)).err().unwrap().to_string();
    let tail = "not set; --adhoc signs with no identity";
    assert_eq!(refused(&[API_KEY]), format!("{API_KEY} {tail}"));
    assert_eq!(
        refused(&[API_ISSUER, CERTIFICATE]),
        format!("{CERTIFICATE}, {API_ISSUER} {tail}")
    );
    // Set to nothing is not set.
    let mut empty = ALL;
    empty[3] = (API_KEY_ID, "");
    assert_eq!(
        credentials(&empty).err().unwrap().to_string(),
        format!("{API_KEY_ID} {tail}")
    );
}

#[test]
fn the_certificate_is_written_decoded_and_the_key_as_it_is_each_to_a_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let found = credentials(&ALL).unwrap();
    let (certificate, key) = (
        dir.path().join("certificate.p12"),
        dir.path().join("notary.p8"),
    );
    found.write_certificate(&certificate).unwrap();
    found.write_key(&key).unwrap();
    assert_eq!(fs::read(&certificate).unwrap(), b"pkcs12");
    assert_eq!(fs::read_to_string(&key).unwrap(), ALL[2].1);
    // A file that is there is not written over: the folder is the run's own, and new.
    let told = found.write_key(&key).unwrap_err().to_string();
    assert!(
        told.starts_with(&format!("could not write {}: ", key.display())),
        "{told}"
    );
}

#[test]
fn a_certificate_that_is_not_base64_is_refused_without_saying_a_character_of_it() {
    let mut vars = ALL;
    vars[0] = (CERTIFICATE, "not base64 at all !");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("certificate.p12");
    let told = credentials(&vars)
        .unwrap()
        .write_certificate(&path)
        .unwrap_err()
        .to_string();
    assert_eq!(told, "APPLE_CERTIFICATE is not base64");
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn what_is_written_is_readable_by_this_user_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("notary.p8");
    credentials(&ALL).unwrap().write_key(&key).unwrap();
    assert_eq!(
        fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn every_secret_is_blanked_wherever_it_is_said_and_the_rest_of_the_text_is_kept() {
    let secrets = Secrets::new(["hunter2", "KEYID12345"]);
    assert_eq!(
        secrets.redact("password hunter2 for KEYID12345, hunter2 again"),
        "password [redacted] for [redacted], [redacted] again"
    );
    assert_eq!(secrets.redact("nothing to hide"), "nothing to hide");
    assert_eq!(secrets.redact(""), "");
    // Multi-byte text round the blanks is kept whole.
    let secrets = Secrets::new(["é"]);
    assert_eq!(
        secrets.redact("café é — ok é"),
        "caf[redacted] [redacted] — ok [redacted]"
    );
}

#[test]
fn secrets_that_overlap_are_blanked_as_one_and_a_longer_one_wins_over_one_inside_it() {
    let overlapping = Secrets::new(["abcdef", "defghi"]);
    assert_eq!(overlapping.redact("xxabcdefghixx"), "xx[redacted]xx");
    let inside = Secrets::new(["secret", "secret-and-more"]);
    assert_eq!(inside.redact("a secret-and-more b"), "a [redacted] b");
    assert_eq!(inside.redact("a secret b"), "a [redacted] b");
    // The blank is not itself read as a secret: one of them, `act`, is in it.
    let in_the_blank = Secrets::new(["hunter2", "act"]);
    assert_eq!(
        in_the_blank.redact("hunter2 and act"),
        "[redacted] and [redacted]"
    );
}

#[test]
fn nothing_is_blanked_where_there_is_nothing_to_blank() {
    assert_eq!(Secrets::default().redact("a text"), "a text");
    assert_eq!(Secrets::new([""]).redact("a text"), "a text");
    assert_eq!(Secrets::new(["  \n "]).redact("a text"), "a text");
}

#[test]
fn a_value_is_blanked_whole_trimmed_and_line_by_line() {
    let key = "-----BEGIN PRIVATE KEY-----\nMIGTAgEA\nbWVhbnM=\n-----END PRIVATE KEY-----\n";
    let secrets = Secrets::new([key]);
    assert_eq!(secrets.redact(key), "[redacted]");
    assert_eq!(secrets.redact(key.trim_end()), "[redacted]");
    // A tool that says one line of it.
    assert_eq!(
        secrets.redact("bad key MIGTAgEA in a file"),
        "bad key [redacted] in a file"
    );
    // A value with a stray newline, said without it.
    assert_eq!(
        Secrets::new(["p4ss\n"]).redact("wrong p4ss given"),
        "wrong [redacted] given"
    );
}

#[test]
fn the_secrets_of_a_run_are_the_five_values_and_the_keychains_password() {
    let secrets = Secrets::of(&credentials(&ALL).unwrap(), "keychain-pw");
    for secret in [
        "cGtjczEy",
        "p12 password",
        "MIGTAgEA",
        "KEYID12345",
        "69a6de70-1111-47e3",
        "keychain-pw",
    ] {
        assert_eq!(
            secrets.redact(&format!("[{secret}]")),
            "[[redacted]]",
            "{secret}"
        );
    }
}
