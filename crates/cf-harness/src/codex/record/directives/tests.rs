//! The directives of Codex's own screen, read out of the model's words. Every
//! path and every name here is made up.

use super::readable;

/// What `text` reads as.
fn read(text: &str) -> String {
    readable(text).into_owned()
}

#[test]
fn a_file_citation_reads_as_its_path_wherever_it_stands_in_a_sentence() {
    assert_eq!(
        read(r#":codex-file-citation{path="/srv/demo/docs/guide.pdf" purpose="source"}"#),
        "/srv/demo/docs/guide.pdf"
    );
    assert_eq!(
        read(
            r#"The figures come from :codex-file-citation{path="/srv/demo/data/sales.csv" purpose="source"}, which I read."#
        ),
        "The figures come from /srv/demo/data/sales.csv, which I read."
    );
    // Whatever else the directive carries, and in whatever order.
    assert_eq!(
        read(r#":codex-file-citation{purpose="source" line="12" path="/srv/demo/a.rs"}"#),
        "/srv/demo/a.rs"
    );
    assert_eq!(
        read(
            r#"See :codex-file-citation{path='/srv/demo/b.rs'} and :codex-file-citation{path=/srv/demo/c.rs}."#
        ),
        "See /srv/demo/b.rs and /srv/demo/c.rs."
    );
}

#[test]
fn a_citation_whose_path_holds_a_brace_a_quote_or_a_line_break_is_read_whole() {
    assert_eq!(
        read(r#"x :codex-file-citation{path="/srv/demo/odd}name.txt" purpose="source"} y"#),
        "x /srv/demo/odd}name.txt y"
    );
    assert_eq!(
        read(r#":codex-file-citation{path="/srv/demo/say \"hi\".txt"}"#),
        r#"/srv/demo/say "hi".txt"#
    );
    assert_eq!(
        read(":codex-file-citation{path=\"/srv/demo/a\nb.txt\"} done"),
        "/srv/demo/a\nb.txt done"
    );
}

#[test]
fn any_other_directive_reads_as_its_label_and_as_nothing_where_it_has_none() {
    assert_eq!(
        read(
            r#":codex-followup[Run the tests again]{prompt="Run the whole suite again and show me what fails"}"#
        ),
        "Run the tests again"
    );
    assert_eq!(
        read("Done.\n\n:codex-followup[Short action]{prompt=\"Do it\"}\n"),
        "Done.\n\nShort action\n"
    );
    // A label may hold brackets that match, and a bracket that is escaped.
    assert_eq!(
        read(r#":codex-followup[Check [the] plan \] now]{prompt="x"}"#),
        r"Check [the] plan \] now"
    );
    // No label: nothing to read, and no gap where it was.
    assert_eq!(
        read(r#"Next, :codex-suggestion{id="a1"} and then more."#),
        "Next, and then more."
    );
    assert_eq!(read(r#"Done. :codex-suggestion{id="a1"}"#), "Done.");
    assert_eq!(read(":codex-suggestion"), "");
    // A file citation that names no path reads as its label.
    assert_eq!(
        read(r#":codex-file-citation[the guide]{purpose="source"}"#),
        "the guide"
    );
    assert_eq!(read(r#":codex-file-citation{path="" purpose="x"}"#), "");
}

#[test]
fn several_directives_in_one_answer_each_read_as_what_they_are() {
    let answer = r#"I changed :codex-file-citation{path="/srv/demo/src/lib.rs"} and :codex-file-citation{path="/srv/demo/src/main.rs" purpose="source"}.

:codex-followup[Add a test]{prompt="Add a test for it"}
:codex-followup[Update the docs]{prompt="Update the docs"}"#;
    assert_eq!(
        read(answer),
        "I changed /srv/demo/src/lib.rs and /srv/demo/src/main.rs.\n\nAdd a test\nUpdate the docs"
    );
}

#[test]
fn text_in_any_language_is_read_whole_and_no_scan_stops_in_the_middle_of_a_character() {
    assert_eq!(
        read(r#"é :codex-file-citation{path="/srv/demo/a.rs"} · done …"#),
        "é /srv/demo/a.rs · done …"
    );
    assert_eq!(
        read(r#"漢字 :codex-followup[再試行]{prompt="もう一度"} 🙂"#),
        "漢字 再試行 🙂"
    );
    assert_eq!(
        read(r#":codex-file-citation{path="/srv/demo/日本/a.rs"}"#),
        "/srv/demo/日本/a.rs"
    );
    assert_eq!(
        read(r#"`é` and ``ü`` :codex-file-citation{path="/srv/demo/a.rs"}"#),
        "`é` and ``ü`` /srv/demo/a.rs"
    );
    // What is no directive is as it was, whatever characters it holds.
    for text in [
        "日本語 :codex- x",
        "ñ :codex-1{} ü",
        "é ::codex-followup[ç]",
    ] {
        assert_eq!(read(text), text);
    }
}

#[test]
fn text_with_no_directive_is_given_back_as_it_is_without_a_copy() {
    for text in [
        "",
        "Nothing to read here.",
        "A colon: and codex-file-citation without one.",
        "https://example.test/codex-file-citation",
    ] {
        assert!(
            matches!(readable(text), std::borrow::Cow::Borrowed(_)),
            "{text:?}"
        );
    }
}

#[test]
fn what_is_not_a_whole_directive_is_left_as_it_was_written() {
    for text in [
        // Its braces are never closed.
        r#"x :codex-file-citation{path="/srv/demo/a.rs" purpose="source""#,
        r#":codex-file-citation{path="/srv/demo/a.rs}"#,
        // Its label is never closed.
        ":codex-followup[Run the tests",
        // No name, or none that starts with a letter.
        ":codex-",
        ":codex- {path=\"/srv/demo/a.rs\"}",
        ":codex-1{path=\"/srv/demo/a.rs\"}",
        // The directive of a leaf, which is a line of its own, and of another's.
        r#"::codex-file-citation{path="/srv/demo/a.rs"}"#,
        r#":other-file-citation{path="/srv/demo/a.rs"}"#,
        // No colon in front of it.
        "codex-file-citation{path=\"/srv/demo/a.rs\"}",
    ] {
        assert_eq!(read(text), text, "{text:?}");
    }
}

#[test]
fn a_directive_quoted_as_code_is_text_and_stays_so() {
    let citation = r#":codex-file-citation{path="/srv/demo/a.rs"}"#;
    // Between backticks, of any number.
    assert_eq!(
        read(&format!(
            "Write `{citation}` to cite a file, as in {citation}."
        )),
        format!("Write `{citation}` to cite a file, as in /srv/demo/a.rs.")
    );
    assert_eq!(
        read(&format!(
            "A ``{citation}`` span, and `a` another {citation}"
        )),
        format!("A ``{citation}`` span, and `a` another /srv/demo/a.rs")
    );
    // In a fenced block, and after it.
    let fenced = format!("Use it like this:\n\n```text\n{citation}\n```\n\nThen {citation}.");
    assert_eq!(
        read(&fenced),
        format!("Use it like this:\n\n```text\n{citation}\n```\n\nThen /srv/demo/a.rs.")
    );
    let tilde = format!("~~~\n{citation}\n~~~\n{citation}");
    assert_eq!(
        read(&tilde),
        format!("~~~\n{citation}\n~~~\n/srv/demo/a.rs")
    );
    // A fence that is never closed holds the rest.
    let open = format!("```\n{citation}\nand more");
    assert_eq!(read(&open), open);
    // A backtick that opens no span is a backtick.
    assert_eq!(
        read(&format!("It costs 5` and {citation}")),
        "It costs 5` and /srv/demo/a.rs"
    );
}
