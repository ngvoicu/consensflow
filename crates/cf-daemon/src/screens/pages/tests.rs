//! The page is what Node serves, and the token is put in where Node put it.

use super::*;

/// The page as Node's generator wrote it (recorded, and fixed since).
const RECORDED_HARNESSES: &str = include_str!("../../../tests/goldens/pages/harnesses.html");

#[test]
fn the_page_is_the_page_node_recorded_to_the_byte() {
    assert!(
        HARNESSES == RECORDED_HARNESSES,
        "the harnesses page is not the recorded one: change src/screens/pages/harnesses.html and tests/goldens/pages/harnesses.html together"
    );
}

#[test]
fn the_token_is_named_once_in_the_page() {
    assert_eq!(HARNESSES.matches(r#""$TOKEN""#).count(), 1);
    assert_eq!(HARNESSES.matches("$TOKEN").count(), 1);
}

#[test]
fn the_token_goes_into_the_script_as_the_text_it_is() {
    let page = harnesses("abc123");
    assert!(page.contains("\nconst TOKEN = \"abc123\";\n"), "{page}");
    assert!(!page.contains("$TOKEN"));
}

#[test]
fn the_page_is_its_recording_and_nothing_else_once_the_token_is_put_back() {
    assert_eq!(
        harnesses("0f0f").replace(r#""0f0f""#, r#""$TOKEN""#),
        RECORDED_HARNESSES
    );
}
