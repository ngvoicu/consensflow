//! How a program that does not do what it was asked is told: by its name and the
//! first of its arguments, and by what it said. The rest of its arguments are
//! never in it, since one of them may be a password.

use super::fake::refused;
use super::{credentials, Fake, Trial, CERTIFICATE_PASSWORD};

fn failure_of(fake: Fake) -> String {
    Trial::new(fake)
        .sign(Some(credentials()))
        .unwrap_err()
        .to_string()
}

#[test]
fn a_program_that_fails_is_told_by_its_name_and_first_argument_and_by_what_it_said() {
    let said = failure_of(
        Fake::new()
            .refusing("security import", 1)
            .saying("no key in it"),
    );
    assert_eq!(said, "security import failed: no key in it");
    // The password and the file it was given are not in it.
    assert!(!said.contains(CERTIFICATE_PASSWORD) && !said.contains("certificate.p12"));
}

#[test]
fn what_a_program_wrote_on_its_error_stream_comes_first_then_its_output_then_its_code() {
    let on = |code, stdout, stderr| {
        failure_of(Fake::new().answering("security create-keychain", refused(code, stdout, stderr)))
    };
    assert_eq!(on(1, "out", "err"), "security create-keychain failed: err");
    assert_eq!(on(1, "out", ""), "security create-keychain failed: out");
    assert_eq!(
        on(65, "", ""),
        "security create-keychain failed: exited with 65"
    );
    // White space at either end is not part of what it said.
    assert_eq!(
        on(1, "", "  err \n\n"),
        "security create-keychain failed: err"
    );
}

#[test]
fn a_program_that_is_not_there_is_told_by_its_name_alone() {
    let said = failure_of(Fake::new().without("security"));
    assert_eq!(
        said,
        "security was not found: is it installed, and on the PATH?"
    );
}
