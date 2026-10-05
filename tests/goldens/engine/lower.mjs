/**
 * `toLowerCase`, which `historyPage` reads an entry and a search by: every
 * code point that changes alone, under the Unicode version Node's ICU holds,
 * and texts whose lower case depends on what is next to a letter (a Greek
 * sigma at the end of a word) or that grows (a dotted capital I).
 */

const TEXTS = [
  '',
  'plain ASCII',
  'MiXeD CaSe',
  'Σ',
  'ΑΣ',
  'ΑΣ.',
  'ΑΣ ',
  'ΑΣΑ',
  'ΣΑΣ ΑΣ ΟΔΟΣ',
  'ΑΣ́',
  'Α­Σ',
  'Α­Σ­Α',
  'Α’Σ',
  'Α.Σ',
  '1Σ',
  'Σ1',
  'ΑΣ1',
  'İ',
  'İSTANBUL İ',
  'İ',
  'FİNAL',
  'ẞ',
  'STRASSE Straße',
  'K Å',
  'ǅ ǈ ǋ ǲ Ǆ',
  'ΐ ᾈ Ὀ',
  'Ⅻ Ⓐ',
  'Ა Ꭰ Ա',
  'ṬẢ',
  'résumé 🙂 漢字',
  '\ud800'.toWellFormed(),
]

/** The table. */
export function lowerCaseTable() {
  const changed = []
  for (let code = 0; code <= 0x10ffff; code += 1) {
    if (code >= 0xd800 && code <= 0xdfff) continue
    const character = String.fromCodePoint(code)
    const lower = character.toLowerCase()
    if (lower !== character) changed.push([code, lower])
  }
  return {
    unicode: process.versions.unicode,
    changed,
    texts: TEXTS.map((text) => ({ text, lower: text.toLowerCase() })),
  }
}
