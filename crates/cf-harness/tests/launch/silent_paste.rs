//! Which harnesses read a human's paste without drawing it until they have read it all
//! (`Adapter::silent_paste`, which the pane host holds the human's keys after a paste by):
//! Codex on Windows, measured on Codex 0.166.0 (T-1048), Devin on every platform, measured on
//! Devin 3000.11.234 on Windows and the Mac (T-1086), and Muse on Windows, measured on Muse Code
//! 1.4.14 on zeewin2 (T-954). Every other harness, and Codex and Muse elsewhere, declare nothing,
//! and the human's keys go as they come: Claude Code and Pi on Windows, and Muse on the Mac, were
//! measured taking a paste and its Enter as they came.

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
fn on_windows_codex_devin_and_muse_read_a_paste_silently_at_their_measured_paces() {
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
            // T-954: Muse 1.4.14 read a paste's keys at 0.6-0.75 ms a character, redrawing its
            // box every 50-65 ms (7,773 characters in 4.5-5.8 s), and took an Enter 42 ms after
            // a line was drawn whole for part of it, one 157 ms after it as its own; the live
            // run drew the long paste at about 1 ms a character, so the limit is 2 (T-1121).
            Harness::Muse => Some(SilentPaste {
                pace_us_per_character: 500,
                drawn_after_ms: 0,
                quiet_ms: 200,
                most_ms: 1_000,
                most_us_per_character: 2_000,
            }),
            _ => None,
        };
        assert_eq!(declared, measured, "{}", harness.as_str());
    }
}

#[test]
fn elsewhere_devin_alone_reads_a_paste_silently_at_the_same_pace() {
    let unix = Env::from_vars([("OS", "Darwin")]);
    let windows = Env::from_vars([("OS", "Windows_NT")]);
    for harness in Harness::ALL {
        let expected = match harness {
            // T-1086: on the Mac Devin also lost an Enter in the paste's own write.
            Harness::Devin => declares(harness, &windows),
            _ => declares(harness, &windows).filter(|_| cfg!(windows)),
        };
        assert_eq!(declares(harness, &unix), expected, "{}", harness.as_str());
    }
    assert!(declares(Harness::Devin, &unix).is_some());
}
