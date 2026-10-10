//! A tool's output with an image in it. Codex writes an image a tool made or
//! showed into the output as a part of its own, `{"type": "input_image",
//! "image_url": "data:image/png;base64,…"}`: the picture itself, megabytes of
//! it, in one line of the rollout. A transcript that copied it would hold that
//! blob, cut where the ledger cuts a tool's output, and the words after it
//! (where Codex says it saved the image) cut away with the rest. So the part is
//! one line that says what the image is, and the output's other parts are what
//! they were.

use serde_json::Value;

use crate::shared::record::reading::{visible_part, visible_text};

/// The most of a path Codex names that is said: a path is a line, not a text.
const PATH_MAX: usize = 1024;

/// An output as a transcript shows it: [`visible_text`], but for each image
/// in it, a line naming the tool whose output it is (when the rollout named the
/// call), the image's type and size, and where Codex says it saved it, when it
/// does in the output's words after the image.
pub(super) fn output_text(output: Option<&Value>, tool: Option<&str>) -> String {
    let Some(Value::Array(parts)) = output else {
        return visible_text(output);
    };
    let images: Vec<Option<Image>> = parts.iter().map(Image::of).collect();
    if images.iter().all(Option::is_none) {
        return visible_text(output);
    }
    parts
        .iter()
        .zip(images)
        .enumerate()
        .map(|(at, (part, image))| match image {
            Some(image) => image.line(tool, saved_path(&parts[at + 1..])),
            None => visible_part(part),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An image Codex put in an output: what its `data:` URL says it is.
struct Image<'a> {
    /// Its type (`image/png`), as the URL names it; none where it names none.
    kind: Option<&'a str>,
    bytes: usize,
}

impl<'a> Image<'a> {
    /// The image a part holds, if it is an `input_image` of a `data:` URL.
    fn of(part: &'a Value) -> Option<Self> {
        if part.get("type")?.as_str()? != "input_image" {
            return None;
        }
        let url = part.get("image_url")?.as_str()?.strip_prefix("data:")?;
        let (head, payload) = url.split_once(',')?;
        let mut words = head.split(';');
        let kind = words.next().filter(|kind| !kind.is_empty());
        let bytes = if words.any(|word| word == "base64") {
            // Four digits are three bytes; the padding stands for none.
            payload.trim_end_matches('=').len() * 3 / 4
        } else {
            // Text, where a byte may be written as an escape of three characters
            // (and a payload of nothing but percent signs is no more than none).
            payload
                .len()
                .saturating_sub(2 * payload.matches('%').count())
        };
        Some(Self { kind, bytes })
    }

    /// The line that stands for the image.
    fn line(&self, tool: Option<&str>, saved: Option<&str>) -> String {
        let from = tool.map_or_else(String::new, |tool| format!(" from {tool}"));
        let kind = self.kind.unwrap_or("unknown type");
        let saved = saved.map_or_else(String::new, |path| format!(", saved to {path}"));
        format!("[image{from}: {kind}, {}{saved}]", size(self.bytes))
    }
}

/// A number of bytes as a person reads it, one decimal and never rounded up
/// into the next unit: `812 B`, `48.0 KB`, `1.5 MB`.
fn size(bytes: usize) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{}.{} KB", bytes / 1_000, bytes % 1_000 / 100),
        _ => format!("{}.{} MB", bytes / 1_000_000, bytes % 1_000_000 / 100_000),
    }
}

/// Where the first of `after` that says so says Codex saved the image:
/// "Generated images are saved to <folder> as <path> by default."
fn saved_path(after: &[Value]) -> Option<&str> {
    after.iter().find_map(|part| {
        let words = part.as_str().or_else(|| part.get("text")?.as_str())?;
        let (_, rest) = words.split_once("saved to ")?;
        let (_, rest) = rest.split_once(" as ")?;
        let (path, _) = rest.split_once(" by default")?;
        Some(path).filter(|path| {
            !path.is_empty() && path.len() <= PATH_MAX && !path.contains(['\n', '\r'])
        })
    })
}

#[cfg(test)]
mod tests;
