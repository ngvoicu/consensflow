//! What an output with an image in it shows: the image as a line, the rest as
//! it was. The shapes are the ones Codex wrote (`exec` drawing an image on
//! 2026-10-10, `view_image` in June).

use super::*;
use serde_json::json;

/// The data URL of an image of `bytes` bytes (`A`s are base64 of zero bytes).
fn png(bytes: usize) -> String {
    format!("data:image/png;base64,{}", "A".repeat(bytes / 3 * 4))
}

fn text(text: &str) -> Value {
    json!({ "type": "input_text", "text": text })
}

fn image(url: &str) -> Value {
    json!({ "type": "input_image", "image_url": url })
}

/// Where Codex says it put the image, as the 2026-10-10 rollout words it.
const HINT: &str = "Generated images are saved to /home/me/.codex/generated_images/t as \
                    /home/me/.codex/generated_images/t/exec-1.png by default.\nIf you need to use \
                    a generated image at another path, copy it.";

#[test]
fn an_image_in_an_output_is_one_line_that_says_what_it_is_and_never_its_bytes() {
    let output = json!([
        text("Script completed\nWall time 15.9 seconds\nOutput:\n"),
        image(&png(1_500_000)),
        text(HINT),
        text("[\"image_url\",\"output_hint\"]"),
    ]);
    let shown = output_text(Some(&output), Some("exec"));
    assert_eq!(
        shown,
        format!(
            "Script completed\nWall time 15.9 seconds\nOutput:\n\n\
             [image from exec: image/png, 1.5 MB, saved to /home/me/.codex/generated_images/t/exec-1.png]\n\
             {HINT}\n[\"image_url\",\"output_hint\"]"
        )
    );
    assert!(!shown.contains("base64"));
}

#[test]
fn an_image_alone_in_an_output_is_that_line_and_the_tool_is_named_when_the_call_was() {
    let output = json!([image(&png(48_000))]);
    assert_eq!(
        output_text(Some(&output), Some("view_image")),
        "[image from view_image: image/png, 48.0 KB]"
    );
    assert_eq!(
        output_text(Some(&output), None),
        "[image: image/png, 48.0 KB]",
        "an output whose call the rollout never named"
    );
}

#[test]
fn an_image_is_sized_by_the_bytes_its_base64_holds_padding_and_all() {
    let size = |payload: &str| {
        let output = json!([image(&format!("data:image/gif;base64,{payload}"))]);
        output_text(Some(&output), None)
    };
    assert_eq!(size(""), "[image: image/gif, 0 B]");
    assert_eq!(size("QQ=="), "[image: image/gif, 1 B]");
    assert_eq!(size("QUI="), "[image: image/gif, 2 B]");
    assert_eq!(size("QUJD"), "[image: image/gif, 3 B]");
    assert_eq!(size("QUJDRA"), "[image: image/gif, 4 B]", "unpadded");
    assert_eq!(size(&"A".repeat(1332)), "[image: image/gif, 999 B]");
    assert_eq!(size(&"A".repeat(1336)), "[image: image/gif, 1.0 KB]");
}

#[test]
fn a_size_is_said_in_the_unit_it_reaches_and_never_rounded_up_into_the_next() {
    for (bytes, said) in [
        (0, "0 B"),
        (999, "999 B"),
        (1_000, "1.0 KB"),
        (48_000, "48.0 KB"),
        (999_999, "999.9 KB"),
        (1_000_000, "1.0 MB"),
        (2_345_678, "2.3 MB"),
        (1_500_000_000, "1500.0 MB"),
    ] {
        assert_eq!(size(bytes), said, "{bytes}");
    }
}

#[test]
fn a_data_url_that_is_not_base64_is_sized_by_the_bytes_its_escapes_stand_for() {
    let output = json!([image("data:image/svg+xml,%3Csvg%2F%3E")]);
    assert_eq!(
        output_text(Some(&output), Some("exec")),
        "[image from exec: image/svg+xml, 6 B]"
    );
    let output = json!([image("data:;base64,QUJD")]);
    assert_eq!(
        output_text(Some(&output), None),
        "[image: unknown type, 3 B]"
    );
    // A payload that is all escapes that are none has no bytes, and no panic.
    let output = json!([image("data:image/png,%%%%%%")]);
    assert_eq!(output_text(Some(&output), None), "[image: image/png, 0 B]");
}

#[test]
fn each_image_names_the_first_place_the_words_after_it_say_it_was_saved() {
    let second = HINT.replace("exec-1", "exec-2");
    let output = json!([
        image(&png(3_000)),
        text(HINT),
        image(&png(3_000)),
        text(&second),
        image(&png(3_000)),
    ]);
    let shown = output_text(Some(&output), Some("exec"));
    let lines: Vec<&str> = shown.lines().filter(|line| line.starts_with('[')).collect();
    assert_eq!(
        lines,
        [
            "[image from exec: image/png, 3.0 KB, saved to /home/me/.codex/generated_images/t/exec-1.png]",
            "[image from exec: image/png, 3.0 KB, saved to /home/me/.codex/generated_images/t/exec-2.png]",
            "[image from exec: image/png, 3.0 KB]",
        ]
    );
}

#[test]
fn a_saved_path_is_one_short_line_or_none() {
    let said = |words: &str| {
        let output = json!([image(&png(3_000)), text(words)]);
        output_text(Some(&output), None)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let named = "[image: image/png, 3.0 KB, saved to C:\\Users\\Jane Doe\\.codex\\x.png]";
    assert_eq!(
        said("Generated images are saved to C:\\Users\\Jane Doe\\.codex as C:\\Users\\Jane Doe\\.codex\\x.png by default."),
        named
    );
    for none in [
        "saved to a folder",
        "saved to a folder as ",
        "saved to a folder as something, whenever",
        "saved to a folder as  by default",
        "saved to a folder as one\nline by default",
        &format!("saved to a folder as {} by default", "x".repeat(2_000)),
    ] {
        assert_eq!(said(none), "[image: image/png, 3.0 KB]", "{none:?}");
    }
}

#[test]
fn an_output_that_holds_no_image_data_is_shown_as_it_always_was() {
    let remote = json!([image("https://example.com/a.png"), text("seen")]);
    let drawn = json!([{ "type": "image" }, { "type": "input_image" }, { "type": "input_image", "image_url": 5 }]);
    for output in [
        json!("plain"),
        json!(["a", { "text": "b" }, 3, null]),
        json!({ "ok": true }),
        remote,
        drawn,
    ] {
        assert_eq!(
            output_text(Some(&output), Some("exec")),
            visible_text(Some(&output)),
            "{output}"
        );
    }
    assert_eq!(output_text(None, None), "");
    assert_eq!(output_text(Some(&Value::Null), None), "");
}
