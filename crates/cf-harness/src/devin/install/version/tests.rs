//! Which Devin records complete worker replies, as Node's Devin adapter suite
//! held it: a version judged a number at a time.

use super::*;

#[test]
fn a_devin_is_supported_from_the_minimum_up_a_number_at_a_time() {
    let high = format!("devin {}.0.0", "9".repeat(400));
    for version in [
        "3000.10.21",
        "3000.10.22",
        "3000.11.0",
        "3001.0.0",
        "4000.0.0",
        "devin 3000.11.3 (9c803229faa4)",
        "3000.10.21.5",
        "\u{e9}3000.10.21",
        &high,
    ] {
        assert!(supported(version), "{version}");
    }
    for version in [
        "",
        "devin",
        "3000.10",
        "3000.10.20",
        "3000.9.99",
        "2999.99.99",
        "devin 1.2.3 3000.11.3",
    ] {
        assert!(!supported(version), "{version}");
    }
}

#[test]
fn a_version_is_three_numbers_between_ascii_word_boundaries() {
    for version in [
        "v3000.10.21",
        "3000.10.21abc",
        "3000.10.21_",
        "3000.10.2_1",
        // Digits JavaScript's `\d` does not take.
        "\u{FF13}\u{FF10}\u{FF10}\u{FF10}.10.21",
    ] {
        assert!(!supported(version), "{version}");
    }
}
