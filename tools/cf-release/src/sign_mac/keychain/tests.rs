//! What is read off `security`'s answers: the user's search list, and the one
//! identity a keychain holds.

use super::*;

const SHA1: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

#[test]
fn the_search_list_is_the_paths_in_quotes_one_to_a_line_in_their_order() {
    let listed = "    \"/Users/runner/Library/Keychains/login.keychain-db\"\n    \"/Library/Keychains/System.keychain\"\n";
    assert_eq!(
        search_list(listed),
        [
            "/Users/runner/Library/Keychains/login.keychain-db",
            "/Library/Keychains/System.keychain"
        ]
    );
    assert_eq!(search_list(""), Vec::<String>::new());
}

#[test]
fn a_line_is_trimmed_and_unquoted_only_where_it_is_quoted_at_both_ends() {
    let listed = "\n  \n\"\"\n  /plain/path  \n\"half\n\"\n\"a \"quoted\" one\"\n";
    assert_eq!(
        search_list(listed),
        ["/plain/path", "\"half", "\"", "a \"quoted\" one"]
    );
}

fn listing(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

#[test]
fn the_one_developer_id_identity_is_found_among_the_lines_the_keychain_lists() {
    let listed = listing(&[
        &format!("  1) {SHA1} \"Developer ID Application: ConsensFlow (TEAMID1234)\""),
        "     1 valid identities found",
    ]);
    assert_eq!(developer_id(&listed).unwrap(), SHA1);
    // Numbered in two digits, with no indent.
    let listed = format!("12) {SHA1} \"Developer ID Application: A B (X)\"");
    assert_eq!(developer_id(&listed).unwrap(), SHA1);
}

#[test]
fn a_certificate_with_no_such_identity_or_with_several_is_refused_by_the_count() {
    let told = |listed: &str| developer_id(listed).unwrap_err().to_string();
    let none = "     0 valid identities found\n";
    assert_eq!(
        told(none),
        "the certificate holds 0 Developer ID Application identities"
    );
    let other = SHA1.replace('0', "A");
    let two = listing(&[
        &format!("  1) {SHA1} \"Developer ID Application: One (TEAM)\""),
        &format!("  2) {other} \"Developer ID Application: Two (TEAM)\""),
    ]);
    assert_eq!(
        told(&two),
        "the certificate holds 2 Developer ID Application identities"
    );
}

#[test]
fn what_is_not_a_developer_id_application_identity_is_not_counted() {
    let lowercase = SHA1.to_lowercase();
    let short = &SHA1[1..];
    let not_counted = listing(&[
        &format!("  1) {SHA1} \"Developer ID Installer: Someone (TEAM)\""),
        &format!("  2) {SHA1} \"Apple Development: Someone (TEAM)\""),
        &format!("  3) {SHA1} \"3rd Party Mac Developer Application: Someone (TEAM)\""),
        &format!("  4) {lowercase} \"Developer ID Application: Someone (TEAM)\""),
        &format!("  5) {short} \"Developer ID Application: Someone (TEAM)\""),
        &format!("  {SHA1} \"Developer ID Application: Someone (TEAM)\""),
        &format!("  x) {SHA1} \"Developer ID Application: Someone (TEAM)\""),
        &format!(") {SHA1} \"Developer ID Application: Someone (TEAM)\""),
        &format!("  6) {SHA1}"),
        &format!("  7) {SHA1}  \"Developer ID Application: Someone (TEAM)\""),
    ]);
    assert_eq!(
        developer_id(&not_counted).unwrap_err().to_string(),
        "the certificate holds 0 Developer ID Application identities"
    );
    // One that is among them is the one.
    let among = format!("{not_counted}  8) {SHA1} \"Developer ID Application: Me (TEAM)\"\n");
    assert_eq!(developer_id(&among).unwrap(), SHA1);
}
