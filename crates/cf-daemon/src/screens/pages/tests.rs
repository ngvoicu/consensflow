//! The pages are what Node serves, and the token and the version are put in
//! where Node put them.

use super::*;

/// The pages as Node's generator wrote them (recorded, and fixed since).
const RECORDED_AGENTS: &str = include_str!("../../../tests/goldens/pages/agents.html");
const RECORDED_HARNESSES: &str = include_str!("../../../tests/goldens/pages/harnesses.html");

#[test]
fn each_page_is_the_page_node_recorded_to_the_byte() {
    assert!(
        AGENTS == RECORDED_AGENTS,
        "the agents page is not the recorded one: change src/screens/pages/agents.html and tests/goldens/pages/agents.html together"
    );
    assert!(
        HARNESSES == RECORDED_HARNESSES,
        "the harnesses page is not the recorded one: change src/screens/pages/harnesses.html and tests/goldens/pages/harnesses.html together"
    );
}

#[test]
fn the_token_is_named_once_in_each_page_and_the_version_once_in_the_agents_page() {
    for (page, template) in [("agents", AGENTS), ("harnesses", HARNESSES)] {
        assert_eq!(template.matches(r#""$TOKEN""#).count(), 1, "{page}");
        assert_eq!(template.matches("$TOKEN").count(), 1, "{page}");
    }
    assert_eq!(AGENTS.matches("v$VERSION</span>").count(), 1);
    assert_eq!(AGENTS.matches("$VERSION").count(), 1);
    assert_eq!(HARNESSES.matches("$VERSION").count(), 0);
}

#[test]
fn the_token_goes_into_the_script_as_the_text_it_is_and_the_version_into_the_heading() {
    let page = agents("abc123");
    assert!(page.contains("\nconst TOKEN = \"abc123\";\n"), "{page}");
    assert!(
        page.contains(&format!(
            "<p class=\"mark\"><span>consensflow</span> <span>v{VERSION}</span></p>"
        )),
        "the version of this build"
    );
    assert!(!page.contains("$TOKEN") && !page.contains("$VERSION"));

    let page = harnesses("abc123");
    assert!(page.contains("\nconst TOKEN = \"abc123\";\n"), "{page}");
    assert!(!page.contains("$TOKEN"));
}

#[test]
fn a_page_is_its_recording_and_nothing_else_once_the_two_are_put_back() {
    let (token, version) = ("0f0f", VERSION);
    assert_eq!(
        agents(token)
            .replace(r#""0f0f""#, r#""$TOKEN""#)
            .replace(&format!("v{version}</span>"), "v$VERSION</span>"),
        RECORDED_AGENTS
    );
    assert_eq!(
        harnesses(token).replace(r#""0f0f""#, r#""$TOKEN""#),
        RECORDED_HARNESSES
    );
}

#[test]
fn a_token_that_needs_escaping_is_written_as_json_stringify_writes_it() {
    // A UI token is hex, but the page is made for any: the script must still parse.
    let token = "a\"b\\c\n\u{e9}\u{2028}";
    let page = agents(token);
    assert!(
        page.contains("\nconst TOKEN = \"a\\\"b\\\\c\\n\u{e9}\u{2028}\";\n"),
        "{page}"
    );
    // What the token holds is no placeholder for the version: the version is only where Node had it.
    assert_eq!(agents("$VERSION").matches(VERSION).count(), 1);
}
