//! The last line of what a stream said, and the first that began with a mark, as the
//! stream comes in chunks that end where they like (inside a line, between a carriage
//! return and its line feed).
//!
//! Only the end is kept, and only as much of it as a line the script ends by can
//! be: a last line that is longer than that is not the one, and it is said not to
//! be known, never read from its middle.

/// How much of the end of a stream is kept: far more than `== exit -2147483648`.
const KEEP: usize = 1024;

/// The end of a stream, so far.
#[derive(Debug, Default)]
pub struct Tail {
    kept: Vec<u8>,
    /// Whether what came before `kept` has been let go.
    cut: bool,
}

impl Tail {
    /// What the stream says next.
    pub fn push(&mut self, bytes: &[u8]) {
        self.kept.extend_from_slice(bytes);
        if self.kept.len() > KEEP {
            let over = self.kept.len() - KEEP;
            self.kept.drain(..over);
            self.cut = true;
        }
    }

    /// The last line, whole, without its line end (a carriage return and a line
    /// feed, or a line feed): the last that is not blank. None if the stream said
    /// none, or if the last was cut by what is kept.
    pub fn last_line(&self) -> Option<String> {
        let text = String::from_utf8_lossy(&self.kept);
        let text = text.trim_end();
        let (line, whole) = match text.rfind('\n') {
            Some(at) => (&text[at + 1..], true),
            None => (text, !self.cut),
        };
        (whole && !line.is_empty()).then(|| line.to_owned())
    }
}

/// The first line a stream says that begins with a mark (the line that says which tree
/// the machine unpacked): whole, without its line end, and none but the first, so that
/// what a command prints later cannot take its place.
///
/// A line is kept while it is read, up to [`KEEP`] bytes: a longer one is not the line the
/// script says, and is never read from its middle.
#[derive(Debug)]
pub struct Marked {
    mark: &'static str,
    /// The line being read, if it can still be the one.
    line: Vec<u8>,
    /// Whether the line being read is longer than a line of the script can be.
    long: bool,
    found: Option<String>,
}

impl Marked {
    pub fn new(mark: &'static str) -> Self {
        Self {
            mark,
            line: Vec::new(),
            long: false,
            found: None,
        }
    }

    /// What the stream says next.
    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.found.is_some() {
                return;
            }
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&self.line);
                let line = line.strip_suffix('\r').unwrap_or(&line);
                if !self.long && line.starts_with(self.mark) {
                    self.found = Some(line.to_owned());
                }
                self.line.clear();
                self.long = false;
            } else if self.line.len() < KEEP {
                self.line.push(byte);
            } else {
                self.long = true;
            }
        }
    }

    /// The first line that began with the mark, whole, if one has been said.
    pub fn found(&self) -> Option<&str> {
        self.found.as_deref()
    }
}

#[cfg(test)]
mod tests;
