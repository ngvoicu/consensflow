//! The owner's config as Devin reads it: what Node's two regular expressions
//! make of a text (each expectation probed on Node v26.8.1), and what JSON
//! this reads that Node read otherwise.

use std::fs;

use super::*;

/// What Devin's installer reads of `text`, left as the owner's config in a
/// config folder of its own.
fn read(text: &str) -> Result<Map<String, Value>, String> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    fs::create_dir_all(dir.path().join("devin")).unwrap();
    fs::write(dir.path().join("devin").join("config.json"), text).unwrap();
    native(&Env::from_vars([
        ("XDG_CONFIG_HOME", root.as_str()),
        ("APPDATA", root.as_str()),
    ]))
}

#[test]
fn comments_and_trailing_commas_are_taken_out_and_nothing_inside_a_string() {
    for (source, stripped) in [
        ("// the owner's own\n{}", " \n{}"),
        ("{/* where */ \"a\": 1}", "{  \"a\": 1}"),
        (
            r#"{"url": "https://example.com/a//b"}"#,
            r#"{"url": "https://example.com/a//b"}"#,
        ),
        (r#"{"a": "x /* y */ z"}"#, r#"{"a": "x /* y */ z"}"#),
        (r#"{"a": "q \" // c"}"#, r#"{"a": "q \" // c"}"#),
        (r#"{"a": "x,}"}"#, r#"{"a": "x,}"}"#),
        (r"[1, 2, 3,]", "[1, 2, 3]"),
        (r#"{"a": [{"b": 1,},], }"#, r#"{"a": [{"b": 1}]}"#),
        ("{\"a\": 1, // one\n\"b\": 2}", "{\"a\": 1,  \n\"b\": 2}"),
        (
            "{\"a\": 1 /* \"q */, \"b\": 2 // it's \"q\n}",
            "{\"a\": 1  , \"b\": 2  \n}",
        ),
        ("{} /**/ /* a */", "{}    "),
        // Not a comment inside a comment: the first block closes at the first close.
        ("{/* a /* b */}", "{ }"),
        // What is no string, no block and no line comment is left as it is.
        ("{\"a\": 1} \"open /* gone */ ", "{\"a\": 1} \"open   "),
        ("[1,\n]", "[1]"),
    ] {
        assert_eq!(strip(source), stripped, "{source}");
    }
}

#[test]
fn a_comma_goes_with_the_white_space_javascript_holds_between_it_and_its_closer() {
    // JavaScript's `\s`: a no-break space, a separator and the byte order mark are white space there.
    for gap in [
        "",
        " ",
        "\t\n\r",
        "\u{a0}",
        "\u{2028}\u{2029}",
        "\u{feff}",
        "\u{3000}",
    ] {
        assert_eq!(strip(&format!("[1,{gap}]")), "[1]", "{gap:?}");
        assert_eq!(
            strip(&format!("{{\"a\": 1,{gap}}}")),
            "{\"a\": 1}",
            "{gap:?}"
        );
    }
    // Unicode's next-line mark is none: the comma stays, and JSON refuses it.
    assert_eq!(strip("[1,\u{85}]"), "[1,\u{85}]");
}

#[test]
fn a_block_comment_and_an_escape_take_any_character_after_them_even_those_no_white_space_matches() {
    for character in ["\u{85}", "\u{2028}", "\u{feff}", "\n", "\r", "\u{1F600}"] {
        assert_eq!(
            strip(&format!("{{/* {character} */}}")),
            "{ }",
            "{character:?}"
        );
        let escaped = format!("{{\"a\": \"\\{character}\"}}");
        assert_eq!(
            strip(&escaped),
            escaped,
            "an escape of {character:?} is in its string"
        );
    }
    // A line comment ends at CR or LF alone: a line separator is part of it.
    assert_eq!(strip("{} // a\u{2028}b\n[]"), "{}  \n[]");
    assert_eq!(strip("{} // a\u{85}b\r\n[]"), "{}  \r\n[]");
}

#[test]
fn json_this_cannot_hold_is_refused_as_no_config_where_node_read_it() {
    // Kept from Node on purpose: JSON nested past 127 levels, or holding a
    // number past a double's range, is more than a value here can hold.
    let deep = format!("{{\"a\": {}{}}}", "[".repeat(200), "]".repeat(200));
    for owner in [deep.as_str(), r#"{"big": 1e400}"#] {
        assert_eq!(read(owner).err().as_deref(), Some(UNREADABLE), "{owner}");
    }
}

#[test]
fn a_lone_surrogate_escape_is_read_as_a_replacement_character_where_node_kept_it_whole() {
    let configuration = read(r#"{"a": "\ud800"}"#).unwrap();
    assert_eq!(configuration["a"], "\u{fffd}");
}

#[test]
fn a_config_of_an_object_with_hooks_of_an_object_or_none_is_read_whole() {
    for owner in [
        "{}",
        r#"{"hooks": {}}"#,
        r#"{"hooks": null}"#,
        r#"{"hooks": 0}"#,
        r#"{"hooks": ""}"#,
    ] {
        assert!(read(owner).is_ok(), "{owner}");
    }
    assert_eq!(read(r#"{"theme": "dark"}"#).unwrap()["theme"], "dark");
}
