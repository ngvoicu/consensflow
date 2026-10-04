use super::*;

#[test]
fn decode_uri_decodes_what_node_decodes_and_fails_where_it_throws() {
    // Node 26's `decodeURI`, `None` where it threw a `URIError`.
    let cases = [
        ("a%20b", Some("a b")),
        // A reserved character's escape stays as written; `%25` is no such.
        ("%2f%2F%3B%23%25", Some("%2f%2F%3B%23%")),
        ("%E2%82%AC", Some("€")),
        ("%e2%82%ac", Some("€")),
        ("%F0%9F%98%80", Some("😀")),
        ("é%20ü", Some("é ü")),
        ("%C3", None),
        ("%C3%28", None),
        ("%ZZ", None),
        ("%", None),
        ("%4", None),
        ("100%", None),
        ("%E2%82", None),
        ("%E2%82%", None),
        // Overlong, a surrogate, past U+10FFFF, a lone continuation, five bytes.
        ("%C0%AF", None),
        ("%ED%A0%80", None),
        ("%F4%90%80%80", None),
        ("%80", None),
        ("%F8%80%80%80%80", None),
    ];
    for (value, decoded) in cases {
        assert_eq!(decode_uri(value).as_deref(), decoded, "{value}");
    }
}

#[test]
fn a_file_tag_and_a_file_link_are_read_as_their_paths_as_node_read_them() {
    // Node 26's `devinComparable`.
    let cases = [
        (
            r#"See <ref_file file="/Users/a/b.ts" /> now"#,
            "See /Users/a/b.ts now",
        ),
        (
            r#"See <ref_snippet file="/Users/a/b.ts" lines="1-3" /> now"#,
            "See /Users/a/b.ts now",
        ),
        (
            "See [b.ts](file:///Users/a/b.ts) now",
            "See /Users/a/b.ts now",
        ),
        (
            "See [b.ts:1-3](file:///Users/a/b.ts) now",
            "See /Users/a/b.ts now",
        ),
        (
            r#"[x](file:///C:/Users/a%20b/c.ts) and <ref_file file="C:\Users\a b\c.ts" />"#,
            "C:/Users/a b/c.ts and C:/Users/a b/c.ts",
        ),
        // A reserved escape kept, a broken one left as it was.
        (
            "[x](file:///Users/a%2Fb) [y](file:///%ZZ)",
            "/Users/a%2Fb /%ZZ",
        ),
        // `\w` is ASCII; `\s` holds U+00A0 and not U+0085.
        (
            "<ref_\u{e9} file=\"/x\" /> <ref_a\u{a0}file=\"/y\"/> <ref_a\u{85}file=\"/z\"/>",
            "<ref_\u{e9} file=\"/x\" /> /y <ref_a\u{85}file=\"/z\"/>",
        ),
        (
            "[a\nb](file:///p q) [c](file:///p\u{2028}q) [d](file:///p\u{85}q)",
            "[a\nb](file:///p q) [c](file:///p\u{2028}q) /p\u{85}q",
        ),
        // A tag's attributes may cross lines; its end is the first `/>` that ends it.
        (
            "<ref_file file=\"/a\"\n lines=\"2\" /> <ref_file file=\"/b\" x=\"/>\" />",
            "/a /b\" />",
        ),
        (r#"[](file://) <ref_x file="" />"#, " "),
    ];
    for (text, read) in cases {
        assert_eq!(comparable(text), read, "{text:?}");
    }
}
