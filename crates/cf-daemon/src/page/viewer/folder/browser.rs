//! An HTML file of the folder read for the browser's copy: the file through the
//! door as any file is read, an `.html` or `.htm` one of at most [`MAX_FILE`]
//! bytes, with the assets it names in the `assets` folder beside its own, each
//! read the same way, at most [`MAX_ASSETS`] of them.

use std::io::Read;

use super::super::door::Opened;
use super::super::names::parts;
use super::{cannot, has_extension, measure, Folder, HTML, MAX_FILE};

/// How an HTML file of Specmint's names its assets: in the `assets` folder
/// beside its spec's folder (`../assets/spec-styles.css`).
const ASSETS: &[u8] = b"../assets/";
/// The most assets an HTML file is copied with.
pub(in super::super) const MAX_ASSETS: usize = 16;

/// An HTML file of the folder and the assets it names, as the browser's copy
/// holds them: each file's place in the copy, by names, and its bytes, the
/// HTML file first. The HTML file keeps its name and its folder's
/// (`checkout/SPEC.html`), and the assets theirs beside that folder
/// (`assets/spec-styles.css`), so `../assets/` leads to them in the copy as
/// it does in the folder.
#[derive(Debug, PartialEq)]
pub(in super::super) struct ForBrowser {
    pub(in super::super) files: Vec<(Vec<String>, Vec<u8>)>,
}

impl Folder {
    /// The HTML file at `path`, for the browser's copy: read through the door
    /// as any file is (a regular file of one name, in the folder), an `.html`
    /// or `.htm` one of at most [`MAX_FILE`] bytes; with the assets it names
    /// (`../assets/<name>`), each read the same way from the `assets` folder
    /// beside its own, and passed over where one cannot be.
    pub(in super::super) fn for_browser(
        &self,
        path: &str,
        between: &dyn Fn(),
    ) -> Result<ForBrowser, String> {
        // One look for the file and every asset: a folder is listed once in it.
        self.look();
        let named = parts(path, cfg!(windows))?;
        let opened = self.open_file(path, between)?;
        if !has_extension(path, &HTML) {
            return Err(format!(
                "{path} is no HTML file: only an .html or .htm file opens in the browser"
            ));
        }
        let html = whole(opened, path)?;
        let Some((file, folders)) = named.split_last() else {
            return Err(format!("{path} names no file"));
        };
        let at = folders.last().into_iter().chain([file]);
        let mut files = vec![(at.map(|name| (*name).to_owned()).collect(), html)];
        // Above the folder at the top, `../assets/` is out of it: none is copied.
        if let Some((_, above)) = folders.split_last() {
            for name in assets_named(&files[0].1) {
                let place = vec!["assets".to_owned(), name.clone()];
                let asset = above
                    .iter()
                    .copied()
                    .chain(["assets", name.as_str()])
                    .collect::<Vec<_>>()
                    .join("/");
                let read = self
                    .open_file(&asset, &|| {})
                    .and_then(|opened| whole(opened, &asset));
                if let (Ok(bytes), false) = (read, files.iter().any(|(at, _)| *at == place)) {
                    files.push((place, bytes));
                }
            }
        }
        Ok(ForBrowser { files })
    }
}

/// The whole of the opened file (`path` in the folder) for the browser's copy,
/// read through its handle: refused over [`MAX_FILE`] bytes.
fn whole(opened: Opened, path: &str) -> Result<Vec<u8>, String> {
    let too_large = |size| {
        format!(
            "{path} is {}, more than the {} the viewer opens",
            measure(size),
            measure(MAX_FILE)
        )
    };
    if opened.size > MAX_FILE {
        return Err(too_large(opened.size));
    }
    let mut bytes = Vec::new();
    { opened.file }
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| cannot(path, &error))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(too_large(bytes.len() as u64));
    }
    Ok(bytes)
}

/// The assets an HTML file names in the `assets` folder beside its own
/// (`../assets/<name>`), each once, in the order it names them, at most
/// [`MAX_ASSETS`]: a name is what follows up to the first character a file's
/// name of Specmint's has none of.
fn assets_named(html: &[u8]) -> Vec<String> {
    let in_name = |byte: &u8| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-');
    let mut names: Vec<String> = Vec::new();
    let mut from = 0;
    while names.len() < MAX_ASSETS {
        let Some(found) = html[from..]
            .windows(ASSETS.len())
            .position(|window| window == ASSETS)
        else {
            break;
        };
        let start = from + found + ASSETS.len();
        let end = html[start..]
            .iter()
            .position(|byte| !in_name(byte))
            .map_or(html.len(), |length| start + length);
        let name = String::from_utf8_lossy(&html[start..end]).into_owned();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
        from = end;
    }
    names
}
