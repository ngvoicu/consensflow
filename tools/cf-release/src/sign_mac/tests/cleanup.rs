//! What a run does about its keychain and its folder when something fails: from
//! whichever call it is, the user's search list is back as it was, the keychain
//! is deleted, nothing is left, and the run ends there.

use super::{credentials, secrets, Fake, Trial};

/// What `security list-keychains` is told to set the search list to when it is
/// put back.
fn restore() -> String {
    format!(
        "security list-keychains -d user -s {}",
        Fake::SEARCHED.join(" ")
    )
}

/// A text with every secret in it, for a failing tool to say.
fn chatter() -> String {
    secrets().join(" | ")
}

/// The calls a release makes under the Developer ID when nothing fails, and the
/// place of the one that makes the keychain.
fn calls_of_a_release() -> (usize, usize) {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    let calls = trial.fake.calls();
    let made = calls
        .iter()
        .position(|call| call.starts_with("security create-keychain"))
        .unwrap();
    (calls.len(), made)
}

/// Whether a failure of this call is one the run goes on from, or ends in.
fn is_gone_on_from(call: &str) -> bool {
    // A volume is detached again, and the last two calls are the tidying: told, never raised.
    call.starts_with("hdiutil detach")
        || *call == restore()
        || call.starts_with("security delete-keychain")
}

#[test]
fn whichever_call_fails_the_search_list_is_back_the_keychain_is_gone_and_the_run_ends_there() {
    let (total, made) = calls_of_a_release();
    for index in 0..total {
        let mut trial = Trial::new(Fake::new().failing_at(index).saying(&chatter()));
        let result = trial.sign(Some(credentials()));
        let calls = trial.fake.calls();
        let failed = &calls[index];
        let said = format!("call {index}, {failed}, fails: {calls:#?}");

        // The tidying: once the keychain is made, the list is set back and the keychain deleted.
        if index > made {
            let [restored, deleted] = &calls[calls.len() - 2..] else {
                unreachable!()
            };
            assert_eq!(*restored, restore(), "{said}");
            assert!(
                deleted.starts_with("security delete-keychain ")
                    && deleted.ends_with(".keychain-db"),
                "{said}"
            );
        } else {
            assert!(
                !calls.iter().any(|call| call.contains("delete-keychain")),
                "{said}"
            );
            assert!(
                !calls
                    .iter()
                    .any(|call| call.starts_with("security list-keychains -d user -s")),
                "{said}"
            );
        }

        // The certificate, the key and what the keychain held are not left behind.
        assert_eq!(trial.left_behind(), Vec::<String>::new(), "{said}");

        // A failure ends the run: what follows it is only the tidying.
        if is_gone_on_from(failed) {
            assert!(result.is_ok(), "{said}");
        } else {
            assert!(result.is_err(), "{said}");
            let after = &calls[index + 1..];
            assert!(after.iter().all(|call| is_gone_on_from(call)), "{said}");
        }
    }
}

#[test]
fn an_ad_hoc_run_that_fails_leaves_nothing_and_never_meets_a_keychain() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(None).unwrap();
    let total = trial.fake.calls().len();
    for index in 0..total {
        let mut trial = Trial::new(Fake::new().failing_at(index));
        let result = trial.sign(None);
        let calls = trial.fake.calls();
        let said = format!("call {index}, {}, fails: {calls:#?}", calls[index]);
        assert!(
            calls.iter().all(|call| !call.starts_with("security")),
            "{said}"
        );
        assert_eq!(trial.left_behind(), Vec::<String>::new(), "{said}");
        if calls[index].starts_with("hdiutil detach") {
            assert!(result.is_ok(), "{said}");
        } else {
            assert!(result.is_err(), "{said}");
            assert!(
                calls[index + 1..]
                    .iter()
                    .all(|call| call.starts_with("hdiutil detach")),
                "{said}"
            );
        }
    }
}

#[test]
fn a_tidying_call_that_fails_is_told_by_its_name_and_the_run_still_ends_well() {
    let fake = Fake::new()
        .refusing("security delete-keychain", 1)
        .saying("it is busy");
    let mut trial = Trial::new(fake);
    trial.sign(Some(credentials())).unwrap();
    assert!(
        trial
            .err
            .contains("sign-mac: security delete-keychain failed: it is busy\n"),
        "{}",
        trial.err
    );

    let fake = Fake::new()
        .refusing("security list-keychains -d user -s /Users", 1)
        .saying("no way");
    let mut trial = Trial::new(fake);
    trial.sign(Some(credentials())).unwrap();
    assert!(
        trial
            .err
            .contains("sign-mac: security list-keychains failed: no way\n"),
        "{}",
        trial.err
    );
    // The keychain is deleted all the same.
    let calls = trial.fake.calls();
    assert!(calls[calls.len() - 1].starts_with("security delete-keychain"));
}

#[test]
fn a_failure_in_the_tidying_does_not_hide_the_one_that_ended_the_run() {
    let fake = Fake::new()
        .answering(
            "xcrun notarytool submit",
            super::fake::refused(1, "", "queue is gone"),
        )
        .refusing("security delete-keychain", 1);
    let mut trial = Trial::new(fake);
    let failure = trial.sign(Some(credentials())).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "notarytool submit failed: queue is gone"
    );
    assert!(trial.err.contains("security delete-keychain failed"));
}
