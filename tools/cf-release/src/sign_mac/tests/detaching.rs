//! The DMG's volume: mounted to put the signed app in it, and let go again
//! whatever came of that.

use std::time::Duration;

use super::{Fake, Trial};

fn detaches(trial: &Trial) -> Vec<String> {
    trial
        .transcript()
        .into_iter()
        .filter(|call| call.starts_with("hdiutil detach"))
        .collect()
}

#[test]
fn a_volume_that_stays_busy_is_asked_three_times_two_seconds_apart_and_then_forced() {
    let mut trial = Trial::new(Fake::new().refusing("hdiutil detach", 3));
    trial.sign(None).unwrap();
    let plain = "hdiutil detach $SCRATCH/volume";
    assert_eq!(
        detaches(&trial),
        [plain, plain, plain, &format!("{plain} -force")]
    );
    assert_eq!(trial.fake.waits(), [Duration::from_secs(2); 3]);
    // The run went on to make the DMG again.
    assert!(trial
        .transcript()
        .iter()
        .any(|call| call.contains("-format UDZO")));
}

#[test]
fn a_volume_that_cannot_be_let_go_ends_the_run_naming_it_and_the_dmg_is_not_made() {
    let mut trial = Trial::new(Fake::new().refusing("hdiutil detach", 4));
    let failure = trial.sign(None).unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!(
            "hdiutil detach failed: {} stays mounted",
            trial.scratch().join("volume").display()
        )
    );
    assert_eq!(detaches(&trial).len(), 4);
    assert_eq!(trial.fake.waits(), [Duration::from_secs(2); 3]);
    assert!(!trial
        .transcript()
        .iter()
        .any(|call| call.contains("-format UDZO")));
    assert_eq!(trial.left_behind(), Vec::<String>::new());
}

#[test]
fn a_volume_that_copying_into_failed_is_let_go_and_the_failure_is_the_copys() {
    for failing in ["ditto", "chmod"] {
        let mut trial = Trial::new(Fake::new().refusing(failing, 1).saying("disk is full"));
        let failure = trial.sign(None).unwrap_err().to_string();
        assert!(
            failure.starts_with(&format!("{failing} "))
                && failure.ends_with(" failed: disk is full"),
            "{failure}"
        );
        // Nothing is done in the volume after the failure but to let it go.
        let calls = trial.transcript();
        let at = calls
            .iter()
            .position(|call| call.starts_with(failing))
            .unwrap();
        assert_eq!(&calls[at + 1..], ["hdiutil detach $SCRATCH/volume"]);
    }
}

#[test]
fn a_copy_and_a_detach_that_both_fail_are_both_told() {
    let fake = Fake::new()
        .refusing("ditto", 1)
        .refusing("hdiutil detach", 4)
        .saying("no luck");
    let mut trial = Trial::new(fake);
    let failure = trial.sign(None).unwrap_err().to_string();
    assert!(
        failure.contains(" failed: no luck; and hdiutil detach failed: "),
        "{failure}"
    );
    assert!(failure.ends_with(" stays mounted"), "{failure}");
}

#[test]
fn a_volume_that_was_never_mounted_is_not_let_go() {
    let mut trial = Trial::new(Fake::new().refusing("hdiutil attach", 1));
    trial.sign(None).unwrap_err();
    assert_eq!(detaches(&trial), Vec::<String>::new());
}
