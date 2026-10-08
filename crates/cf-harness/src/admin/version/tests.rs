//! `versionOf` and `newer` as Node's admin had them: the answers below are Node
//! v26.8.1's, for each text and each pair; the recorded table
//! (`tests/goldens/admin/tables.json`) holds more, through the admin.

use super::*;

#[test]
fn a_version_is_found_where_a_cli_says_it() {
    for (text, version) in [
        ("codex-cli 99.1.0", "99.1.0"),
        ("99.1.0", "99.1.0"),
        ("99.1.0\n", "99.1.0"),
        ("v1.2.3", "1.2.3"),
        ("dev1.2.3", "1.2.3"),
        ("claude 2.1.3 (Claude Code)", "2.1.3"),
        ("1.2.3)", "1.2.3"),
        ("1.2.3-beta.1", "1.2.3-beta.1"),
        ("1.2.3-beta.1 build", "1.2.3-beta.1"),
        ("1.2.3-a_b", "1.2.3-a_b"),
        ("1.2.3-a b", "1.2.3-a"),
        ("v1.2.3-rc.1)", "1.2.3-rc.1"),
        ("01.02.03", "01.02.03"),
        ("Devin 3000.10.21 (abc)", "3000.10.21"),
        ("1.2.3 1.2.4", "1.2.3"),
    ] {
        assert_eq!(version_of(text), Some(version), "{text:?}");
    }
}

#[test]
fn a_number_that_is_followed_or_preceded_by_anything_else_is_no_version() {
    for text in [
        "(1.2.3)",
        "1.2.3-beta+meta",
        "1.2.3+meta",
        "1.2.3.4",
        "a1.2.3",
        "version=1.2.3",
        "1.2",
        "",
        "1.2.3-",
        "1.2.3-\u{e9}",
        // Digits JavaScript's `\d` does not take.
        "\u{FF11}.\u{FF12}.\u{FF13}",
    ] {
        assert_eq!(version_of(text), None, "{text:?}");
    }
}

#[test]
fn a_candidate_the_end_refuses_gives_way_to_the_next_one() {
    // The first number is followed by `x`, the second one by white space.
    assert_eq!(version_of("1.2.3x 4.5.6"), Some("4.5.6"));
    assert_eq!(version_of("1.2.3- 4.5.6"), Some("4.5.6"));
    assert_eq!(version_of("x 1.2.3.4 1.2.5"), Some("1.2.5"));
}

#[test]
fn white_space_is_javascripts_not_unicodes() {
    // JavaScript's `\s` takes U+00A0, U+2028 and U+FEFF, and not U+0085.
    for text in [
        "\u{a0}1.2.3",
        "\u{2028}1.2.3",
        "\u{feff}1.2.3",
        "1.2.3\u{2028}",
    ] {
        assert_eq!(version_of(text), Some("1.2.3"), "{text:?}");
    }
    for text in ["\u{85}1.2.3", "1.2.3\u{85}"] {
        assert_eq!(version_of(text), None, "{text:?}");
    }
}

#[test]
fn a_release_is_newer_a_number_at_a_time() {
    for (local, remote, answer) in [
        ("1.0.0", "1.0.1", true),
        ("1.0.1", "1.0.0", false),
        ("1.0.0", "1.0.0", false),
        ("1.9.0", "1.10.0", true),
        ("1.0.0", "2.0.0", true),
        ("007.0.0", "7.0.0", false),
        ("3000.6.14", "3000.10.21", true),
    ] {
        assert_eq!(newer(Some(local), remote), Some(answer), "{local} {remote}");
    }
}

#[test]
fn numbers_past_a_double_are_read_as_javascript_reads_them() {
    let huge = "9".repeat(400);
    assert_eq!(
        newer(Some(&format!("{huge}.0.0")), &format!("{huge}.0.1")),
        Some(true),
        "both infinity, so the first numbers do not differ"
    );
}

#[test]
fn a_comparison_of_anything_but_three_whole_numbers_is_none() {
    for (local, remote) in [
        (Some("1.0.0-beta"), "1.0.0"),
        (Some("1.0.0"), "v1.0.1"),
        (Some("1.0.0"), "1.0.0\n"),
        (None, "1.0.0"),
        (Some("1.0.0"), ""),
        (Some("1.0.0"), "\u{FF11}.0.1"),
        (Some("1.0"), "1.0.1"),
    ] {
        assert_eq!(newer(local, remote), None, "{local:?} {remote:?}");
    }
}
