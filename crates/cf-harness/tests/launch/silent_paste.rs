//! Which harnesses read a human's paste without drawing it until they have read it all
//! (`Adapter::silent_paste`, which the pane host holds the human's keys after a paste by):
//! Codex on Windows, measured on Codex 0.166.0 (T-1048), and Devin on Windows, measured on Devin
//! 3000.11.234 (T-1086). Every other harness, and both elsewhere, declare nothing, and the
//! human's keys go as they come: Claude Code and Pi on Windows were measured taking a paste and
//! its Enter as they came.

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
fn on_windows_codex_and_devin_read_a_paste_silently_at_their_measured_paces() {
    let windows = Env::from_vars([("OS", "Windows_NT")]);
    for harness in Harness::ALL {
        let declared = declares(harness, &windows);
        let measured = match harness {
            Harness::Codex => Some(SilentPaste {
                pace_us_per_character: 600,
                drawn_after_ms: 50,
                quiet_ms: 120,
                most_ms: 1_000,
                most_us_per_character: 1_000,
            }),
            // T-1086: Devin 3000.11.234 drew a line at ~10 ms and 7,775 characters at
            // ~140-160 ms, with no print before, and lost an Enter that came before its draw.
            Harness::Devin => Some(SilentPaste {
                pace_us_per_character: 20,
                drawn_after_ms: 0,
                quiet_ms: 80,
                most_ms: 1_000,
                most_us_per_character: 100,
            }),
            _ => None,
        };
        assert_eq!(declared, measured, "{}", harness.as_str());
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
