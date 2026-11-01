//! Which harnesses read a human's paste without drawing it until they have read it all
//! (`Adapter::silent_paste`, which the pane host holds the human's keys after a paste by):
//! Codex on Windows, measured on Codex 0.166.0 (T-1048). Every other harness, and Codex
//! elsewhere, declares nothing, and the human's keys go as they come.

use cf_base::env::Env;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;
use cf_proto::panes::SilentPaste;

/// What `harness`'s adapter declares of how its windows read a paste, run with `env`.
fn declares(harness: Harness, env: &Env) -> Option<SilentPaste> {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).silent_paste()
}

#[test]
fn on_windows_codex_alone_reads_a_paste_silently_at_its_measured_pace() {
    let windows = Env::from_vars([("OS", "Windows_NT")]);
    for harness in Harness::ALL {
        let declared = declares(harness, &windows);
        if harness == Harness::Codex {
            assert_eq!(
                declared,
                Some(SilentPaste {
                    pace_us_per_character: 600,
                    drawn_after_ms: 50,
                    quiet_ms: 120,
                    most_ms: 1_000,
                    most_us_per_character: 1_000,
                })
            );
        } else {
            assert_eq!(declared, None, "{}", harness.as_str());
        }
    }
}

#[test]
fn elsewhere_no_harness_reads_a_paste_silently() {
    let unix = Env::from_vars([("OS", "Darwin")]);
    for harness in Harness::ALL {
        let expected =
            declares(harness, &Env::from_vars([("OS", "Windows_NT")])).filter(|_| cfg!(windows));
        assert_eq!(declares(harness, &unix), expected, "{}", harness.as_str());
    }
}
