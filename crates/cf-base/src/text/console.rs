//! Text as Windows' console carries it to a window that reads key presses: a
//! paste into Devin on Windows reaches it as key presses, and every non-ASCII
//! punctuation mark and symbol is lost on the way while letters arrive (Devin
//! 3000.11, 2026-10-03; Codex 0.160 the same). Unicode is ICU4X's, 17.0, the
//! version Node 26's ICU holds.

use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::CodePointMapData;

use crate::js;

/// A window's text as Windows' console carries it to Devin, composed (NFC)
/// first: letters and ASCII as they are, and every other character it
/// would drop in the ASCII that spells it: a mapped mark (\u{2014} as `--`,
/// \u{20AC} as `EUR`), a shown control character in caret notation
/// (\u{241B} as `^[`), a space as a space, other box drawing as `+`, and
/// what Unicode also writes plainly (\u{B2} as `2`, \u{BD} as `1/2`). A
/// character with no ASCII spelling (an emoji) is left as it is.
pub fn console_text(text: &str) -> String {
    let composed = ComposingNormalizerBorrowed::new_nfc().normalize(text);
    let mut carried = String::with_capacity(composed.len());
    for character in composed.chars() {
        carry(character, &mut carried);
    }
    carried
}

/// What the console carries of `character`, written onto `carried`.
fn carry(character: char, carried: &mut String) {
    let code = u32::from(character);
    if code < 0x80 || is_letter(character) {
        carried.push(character);
    } else if let Some(mapped) = ascii(character) {
        carried.push_str(mapped);
    } else if (0x2400..0x2420).contains(&code) {
        carried.push('^');
        carried.extend(char::from_u32(code - 0x2400 + 64));
    } else if code == 0x2421 {
        carried.push_str("^?");
    } else if js::is_space(character) {
        carried.push(' ');
    } else if (0x2500..0x2580).contains(&code) {
        carried.push('+');
    } else {
        match plain(character) {
            Some(plain) => carried.push_str(&plain),
            None => carried.push(character),
        }
    }
}

/// `character`'s compatibility decomposition without its marks, the
/// fraction slash as `/` (`.replace`, the first alone): what Unicode also
/// writes it as, when that is printable ASCII.
fn plain(character: char) -> Option<String> {
    let mut decomposed = String::new();
    decomposed.push(character);
    let plain: String = DecomposingNormalizerBorrowed::new_nfkd()
        .normalize(&decomposed)
        .chars()
        .filter(|character| !is_mark(*character))
        .collect();
    let plain = plain.replacen('\u{2044}', "/", 1);
    let printable = !plain.is_empty() && plain.bytes().all(|byte| (0x20..=0x7E).contains(&byte));
    printable.then_some(plain)
}

/// `/\p{L}/u`.
fn is_letter(character: char) -> bool {
    GeneralCategoryGroup::Letter.contains(CodePointMapData::<GeneralCategory>::new().get(character))
}

/// `/\p{M}/u`.
fn is_mark(character: char) -> bool {
    GeneralCategoryGroup::Mark.contains(CodePointMapData::<GeneralCategory>::new().get(character))
}

/// The ASCII for a mark the console drops (`CONSOLE_ASCII`).
fn ascii(character: char) -> Option<&'static str> {
    Some(match character {
        '\u{B7}' => "|",
        '\u{2022}' => "*",
        '\u{2014}' => "--",
        '\u{2013}' => "-",
        '\u{2010}' => "-",
        '\u{2011}' => "-",
        '\u{2212}' => "-",
        '\u{2026}' => "...",
        '\u{2018}' => "'",
        '\u{2019}' => "'",
        '\u{201A}' => "'",
        '\u{201C}' => "\"",
        '\u{201D}' => "\"",
        '\u{201E}' => "\"",
        '\u{AB}' => "<<",
        '\u{BB}' => ">>",
        '\u{2192}' => "->",
        '\u{2190}' => "<-",
        '\u{2194}' => "<->",
        '\u{21D2}' => "=>",
        '\u{D7}' => "x",
        '\u{F7}' => "/",
        '\u{B1}' => "+/-",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2260}' => "!=",
        '\u{2248}' => "~",
        '\u{B0}' => "deg",
        '\u{20AC}' => "EUR",
        '\u{A3}' => "GBP",
        '\u{A5}' => "JPY",
        '\u{BF}' => "?",
        '\u{A1}' => "!",
        '\u{A6}' => "|",
        '\u{AC}' => "!",
        '\u{A7}' => "S",
        '\u{B6}' => "P",
        '\u{A9}' => "(c)",
        '\u{AE}' => "(R)",
        '\u{2122}' => "(TM)",
        '\u{2713}' => "OK",
        '\u{2714}' => "OK",
        '\u{2705}' => "OK",
        '\u{2717}' => "X",
        '\u{2718}' => "X",
        '\u{274C}' => "X",
        '\u{2500}' => "-",
        '\u{2501}' => "-",
        '\u{2550}' => "-",
        '\u{2502}' => "|",
        '\u{2503}' => "|",
        '\u{2551}' => "|",
        '\u{FFFD}' => "?",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_in_ascii_every_mark_the_console_dropped_and_keeps_letters_as_they_are() {
        // What a paste into Devin lost on its way.
        let lost = "\u{B7}\u{2014}\u{2013}\u{2026}\u{2192}\u{2190}\u{2019}\u{2018}\u{201C}\u{201D}\u{AB}\u{BB}\u{2022}\u{B0}\u{B1}\u{D7}\u{F7}\u{20AC}\u{A3}\u{A5}\u{A9}\u{AE}\u{2122}\u{A7}\u{B6}\u{A6}\u{A8}\u{AC}\u{AF}\u{B4}\u{B8}\u{BC}\u{BD}\u{BE}\u{BF}";
        for character in lost.chars() {
            let carried = console_text(&character.to_string());
            assert!(
                !carried.is_empty() && carried.bytes().all(|byte| (0x20..=0x7E).contains(&byte)),
                "{character:?} as {carried:?}"
            );
        }
        assert_eq!(
            console_text("[ConsensFlow m-3 \u{B7} T-1 \u{B7} result from @worker]\nCosts \u{20AC}100 \u{2014} 20\u{D7} faster \u{2192} \u{201C}done\u{201D}\u{2026}"),
            "[ConsensFlow m-3 | T-1 | result from @worker]\nCosts EUR100 -- 20x faster -> \"done\"..."
        );
        let letters = "Culoarea: albastr\u{103}; \u{EE}\u{21B}i scriu, caf\u{E9}, Stra\u{DF}e, 5 \u{B5}s, \u{D1}and\u{FA}";
        assert_eq!(console_text(letters), letters);
    }

    #[test]
    fn a_text_is_composed_first_and_an_emoji_has_no_ascii() {
        assert_eq!(console_text("e\u{301}"), "\u{E9}");
        assert_eq!(
            console_text("\u{241B}\u{2421}\u{3000}\u{2502}\u{2554}"),
            "^[^? |+"
        );
        assert_eq!(console_text("\u{1F600}"), "\u{1F600}");
    }
}
