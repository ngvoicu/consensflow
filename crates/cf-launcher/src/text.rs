//! What a launcher says: the text this build writes, and what a command found
//! in the home's bin reads as (`launcher` and `terminalRuntime`,
//! `src/terminal.js`).
//!
//! Two shapes are read and one is written. The old shape, which alpha.78 and
//! every build before it wrote, names the runtime and the `cf.mjs` it runs:
//!
//! ```text
//! exec "<runtime>" "<cf.mjs>" "$@"
//! "<runtime>" "<cf.mjs>" %*
//! ```
//!
//! The new shape names the bundle's native `cf` and nothing else, since the
//! bundle after the deletion has no runtime to name, and the fallback to
//! Node's sources is `cf`'s own business:
//!
//! ```text
//! exec "<cf>" "$@"
//! "<cf>" %*
//! ```
//!
//! The first line of each pair is a script for `sh`, the second a `.cmd`
//! for cmd.exe. Either may carry the home's pin, a line before the command
//! that names the home it belongs to, for a terminal that names none of its
//! own. Every shape has the mark, [`MARKER`], in its first comment.
//!
//! Node wrote a path into its double quotes as it was. This build writes the
//! characters a shell reads for itself (`\`, `"`, `$` and the backquote in
//! `sh`, `%` in a `.cmd`) escaped, so a folder named with one still runs the
//! program in it, and reads them back; a path with none is written as Node
//! wrote it.
//!
//! Kept from Node on purpose: a `.cmd` is written in UTF-8, and cmd.exe reads
//! it in the code page of its console, so a path with a letter outside ASCII
//! in it (a user's name, in a portable app's folder) names another there.
//! Node's launcher had the same limit; a `chcp` in it would change the code
//! page of the terminal the command is run in.

use std::path::Path;

use cf_base::js;

/// The mark that says a launcher is ours to replace or remove (`MARKER`).
/// Every shape carries it, so an older build still knows the command as its
/// own.
pub(crate) const MARKER: &str = "Installed by ConsensFlow";

/// What a launcher runs, as the file says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Runs {
    /// The old shape: a runtime, and the `cf.mjs` it is given.
    Node { runtime: String, entry: String },
    /// The new shape: a native `cf`.
    Native { cf: String },
}

/// Whether the launcher in `text` is ours (`includes(MARKER)`): a command
/// someone else put there is not ours to report as installed, and certainly
/// not ours to remove.
///
/// A launcher is text. A program that merely holds the mark in its bytes is
/// none, and this build is such a program: its own `cf` holds [`MARKER`], so a
/// link from the home's bin to it (the plainest way to put a command on PATH)
/// would be ours by Node's test, and a repair, which writes through a link,
/// would then replace the `cf` itself with a script. A file with a NUL in it
/// is no script of `sh` or cmd.exe, whatever else it says.
pub(crate) fn is_ours(text: &str) -> bool {
    text.contains(MARKER) && !text.contains('\0')
}

/// `path` as it is written into a launcher, and as one is compared with it.
///
/// On Windows that is the plain spelling: Tauri may answer its folders in the
/// verbatim form (`\\?\C:\…`, `\\?\UNC\server\…`), which cmd.exe does not
/// start a program through. The app strips it the same way before it hands a
/// path to Node (`plain_path`, `app/src-tauri/src/daemon_command.rs`); a caller
/// that did not is not left with a command that cannot run.
pub(crate) fn spelled(path: &Path, windows: bool) -> String {
    let text = path.to_string_lossy();
    if !windows {
        return text.into_owned();
    }
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

/// The launcher this build writes, in the form of cmd.exe when `windows`:
/// it runs `cf`, the native one of the bundle, with the arguments it was
/// given, and pins `home` when it has one (a copy running with a home of its
/// own: an ordinary terminal names none, and `cf` would otherwise fall back
/// to the live `~/.consensflow`).
///
/// A `.cmd` has no shebang, so it is the same idea spelled the way cmd.exe
/// understands: lines end in CR LF, `%*` forwards the arguments, and a pin
/// is set after `setlocal` so it ends with the command.
pub(crate) fn launcher(windows: bool, cf: &str, home: Option<&str>) -> String {
    if windows {
        let pin = home.map_or_else(String::new, |home| {
            format!(
                "setlocal\r\nset \"CONSENSFLOW_HOME={}\"\r\n",
                quoted_for_cmd(home)
            )
        });
        return format!(
            "@echo off\r\nREM {MARKER}. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n{pin}\"{}\" %*\r\n",
            quoted_for_cmd(cf)
        );
    }
    let pin = home.map_or_else(String::new, |home| {
        format!("export CONSENSFLOW_HOME=\"{}\"\n", quoted_for_sh(home))
    });
    format!(
        "#!/bin/sh\n# {MARKER}. Runs the app's own cf, so the terminal and the\n# window never drift apart.\n{pin}exec \"{}\" \"$@\"\n",
        quoted_for_sh(cf)
    )
}

/// `text` as it goes between the double quotes of `sh`: the four characters
/// that keep their meaning there, each behind a backslash.
fn quoted_for_sh(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '"' | '$' | '`') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted
}

/// `text` as it goes between the double quotes of a `.cmd`: a percent sign,
/// which cmd.exe reads as the start of a variable even there, doubled.
fn quoted_for_cmd(text: &str) -> String {
    text.replace('%', "%%")
}

/// What the launcher in `text` runs, in either shape, or none when it says
/// nothing of the kind (`terminalRuntime` found no line). The old shape is
/// tried first, as Node knew only that one.
pub(crate) fn runs(text: &str, windows: bool) -> Option<Runs> {
    node_runs(text).or_else(|| cf_runs(text, windows))
}

/// The home the launcher in `text` pins, none when it pins none: the first
/// line of the form this build, and Node, write it in, whatever else the
/// file holds.
pub(crate) fn pinned_home(text: &str, windows: bool) -> Option<String> {
    let opening = if windows {
        "set \"CONSENSFLOW_HOME="
    } else {
        "export CONSENSFLOW_HOME=\""
    };
    line_starts(text).find_map(|line| {
        let rest = text.get(line..)?.strip_prefix(opening)?;
        let (home, after) = unquote(rest, windows)?;
        ends_the_line(rest.get(after..)?).then_some(home)
    })
}

/// The first match in `text` of `/"([^"]+)"\s+"([^"]+cf\.mjs)"/`, which is
/// how Node read the old shape: a quoted runtime, white space, and a quoted
/// path that ends in `cf.mjs`, with something before it.
///
/// A match is four quotes in a row: the text between the first two is not
/// empty, between the second and the third is white space, and between the
/// third and the fourth is not empty and ends in `cf.mjs`; no quote can be
/// inside any of them. The first such four wins, as a match found from the
/// left does.
fn node_runs(text: &str) -> Option<Runs> {
    const ENTRY: &str = "cf.mjs";
    let quotes: Vec<usize> = text.match_indices('"').map(|(at, _)| at).collect();
    quotes.windows(4).find_map(|four| {
        let between = |from: usize, to: usize| text.get(four[from] + 1..four[to]);
        let (runtime, space, entry) = (between(0, 1)?, between(1, 2)?, between(2, 3)?);
        let found = !runtime.is_empty()
            && !space.is_empty()
            && space.chars().all(js::is_space)
            && entry.len() > ENTRY.len()
            && entry.ends_with(ENTRY);
        found.then(|| Runs::Node {
            runtime: runtime.to_owned(),
            entry: entry.to_owned(),
        })
    })
}

/// The native `cf` the first line of the new shape names: `exec "<cf>"
/// "$@"` in `sh`, `"<cf>" %*` in a `.cmd`, alone on its line.
fn cf_runs(text: &str, windows: bool) -> Option<Runs> {
    let (opening, closing) = if windows {
        ("\"", " %*")
    } else {
        ("exec \"", " \"$@\"")
    };
    line_starts(text).find_map(|line| {
        let rest = text.get(line..)?.strip_prefix(opening)?;
        let (cf, after) = unquote(rest, windows)?;
        let tail = rest.get(after..)?.strip_prefix(closing)?;
        ends_the_line(tail).then_some(Runs::Native { cf })
    })
}

/// Where each line of `text` starts.
fn line_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1))
}

/// Whether `rest`, what follows a command on its line, is the end of it.
fn ends_the_line(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n")
}

/// The text of the double-quoted string that begins at the start of `text`
/// (just past its opening quote), as `sh` reads one or, for a `.cmd`, as
/// cmd.exe reads a quoted path, and where it ends: just past its closing
/// quote. None when it never closes.
fn unquote(text: &str, windows: bool) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut characters = text.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        match character {
            '"' => return Some((body, at + 1)),
            '\\' if !windows && matches!(characters.peek(), Some((_, '\\' | '"' | '$' | '`'))) => {
                body.extend(characters.next().map(|(_, escaped)| escaped));
            }
            '%' if windows && matches!(characters.peek(), Some((_, '%'))) => {
                characters.next();
                body.push('%');
            }
            other => body.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests;
