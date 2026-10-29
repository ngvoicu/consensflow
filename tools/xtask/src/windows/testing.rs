//! What the tests of the parts of this module have in common.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// `words` as the owned words the options and a command are made of.
pub fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The script that `encoded` is, as PowerShell's `-EncodedCommand` reads it: the
/// base64 of its UTF-16.
pub fn script_of(encoded: &str) -> String {
    let bytes = STANDARD.decode(encoded).unwrap();
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).unwrap()
}

/// What an `ssh` stand-in does first (the stand-ins are shell scripts: the cases that write them
/// are the Mac's and Linux's), unless the case answers the machine's state itself: asked
/// the script that says the free disk and whether a run holds the copy (it names `PSDrive`), it
/// answers a free disk and a copy nothing holds, and writes nothing down, so that what the case
/// counts of its own is as it was before the runner asked the state of every machine (T-777).
#[cfg(unix)]
const ASKS_STATE: &str = r#"for word; do last=$word; done
case "$(printf '%s' "${last##* }" | base64 -d 2>/dev/null | iconv -f UTF-16LE -t UTF-8 2>/dev/null)" in *PSDrive*) echo "free 614"; exit 0 ;; esac
"#;

/// The body of the `ssh` stand-in a case writes: `body`, after the answer to the machine's state
/// unless `body` answers it (it names `PSDrive`).
#[cfg(unix)]
pub fn ssh_stand_in(body: &str) -> String {
    if body.contains("PSDrive") {
        body.to_owned()
    } else {
        format!("{ASKS_STATE}{body}")
    }
}

/// What an `ssh` or `scp` stand-in does before anything else: writes down every word it was
/// given (`all`), as a line `<program> <words>` of `$CF_LOG.calls`, and then leaves out the
/// options the runner puts before the host and the files (`-o <option>`, after scp's `-q`), so
/// that the stand-in reads its host and its command, or its files, where it always did. The
/// cases that look at the options read the calls.
#[cfg(unix)]
pub const NOTES_THE_CALL_AND_LEAVES_OUT_THE_OPTIONS: &str = r#"all="$*"; printf '%s %s\n' "${0##*/}" "$all" >> "$CF_LOG.calls"
quiet=; if [ "$1" = -q ]; then quiet=-q; shift; fi
while [ "$1" = -o ]; do shift 2; done
if [ -n "$quiet" ]; then set -- -q "$@"; fi
"#;

/// The body of the `ssh` or `scp` stand-in a case writes: `body`, after what every stand-in does
/// first ([`NOTES_THE_CALL_AND_LEAVES_OUT_THE_OPTIONS`]), and for `ssh` after the answer to the
/// machine's state ([`ssh_stand_in`]).
#[cfg(unix)]
pub fn stand_in(name: &str, body: &str) -> String {
    match name {
        "ssh" => format!(
            "{NOTES_THE_CALL_AND_LEAVES_OUT_THE_OPTIONS}{}",
            ssh_stand_in(body)
        ),
        "scp" => format!("{NOTES_THE_CALL_AND_LEAVES_OUT_THE_OPTIONS}{body}"),
        _ => body.to_owned(),
    }
}
