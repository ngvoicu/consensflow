//! An `Info.plist`, read with the `plist` crate: the keys of an app bundle that
//! `prepare-update` asks, as text. The bundle's own is read from the folder, the
//! archive's from the file inside the archive; either may be XML or binary.

use std::io::Cursor;

use plist::{Dictionary, Value};

/// The name of the app's executable, in `Contents/MacOS`.
pub const EXECUTABLE: &str = "CFBundleExecutable";

/// The app's version, the one installed apps compare.
pub const VERSION: &str = "CFBundleShortVersionString";

/// What an `Info.plist` says.
pub struct Info(Dictionary);

impl Info {
    /// The dictionary in `bytes`, if they are a property list of that kind.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match Value::from_reader(Cursor::new(bytes)).ok()? {
            Value::Dictionary(dictionary) => Some(Self(dictionary)),
            _ => None,
        }
    }

    /// The text under `key`, if it holds some.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>ConsensFlow</string>
  <key>CFBundleShortVersionString</key><string>3.0.0-alpha.99</string>
  <key>CFBundleVersion</key><integer>7</integer>
  <key>LSMinimumSystemVersion</key><string></string>
</dict></plist>"#;

    #[test]
    fn xml_is_read_by_key_and_only_text_is_text() {
        let info = Info::parse(XML.as_bytes()).unwrap();
        assert_eq!(info.text(EXECUTABLE), Some("ConsensFlow"));
        assert_eq!(info.text(VERSION), Some("3.0.0-alpha.99"));
        assert_eq!(info.text("LSMinimumSystemVersion"), Some(""));
        // A number is not text, and a key that is not there is nothing.
        assert_eq!(info.text("CFBundleVersion"), None);
        assert_eq!(info.text("CFBundleName"), None);
    }

    #[test]
    fn a_binary_list_is_read_as_the_xml_one_is() {
        let Value::Dictionary(dictionary) = Value::from_reader(Cursor::new(XML)).unwrap() else {
            panic!("the XML is a dictionary");
        };
        let mut binary = Vec::new();
        plist::to_writer_binary(&mut binary, &Value::Dictionary(dictionary)).unwrap();
        assert!(binary.starts_with(b"bplist00"));
        let info = Info::parse(&binary).unwrap();
        assert_eq!(info.text(VERSION), Some("3.0.0-alpha.99"));
        assert_eq!(info.text(EXECUTABLE), Some("ConsensFlow"));
    }

    #[test]
    fn what_is_not_a_dictionary_is_not_an_info_plist() {
        let array = r#"<?xml version="1.0"?><plist version="1.0"><array><string>a</string></array></plist>"#;
        assert!(Info::parse(array.as_bytes()).is_none());
        for bytes in [&b""[..], b"not a plist", b"bplist00", b"<plist><dict>"] {
            assert!(Info::parse(bytes).is_none(), "{bytes:?}");
        }
    }
}
