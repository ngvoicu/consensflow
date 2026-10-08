import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { consoleText } from '../app/ui/core/console-text.js'

/**
 * The page's own `consoleText` (app/ui/core/console-text.js), which spells in
 * ASCII what Windows' console would drop of what the human types into a Devin
 * or Codex window, held to the table the Rust `console_text` is held to
 * (crates/cf-harness/tests/launch/tables.rs reads the same file): every code
 * point the console changes, alone, and the texts that compose or decompose as
 * a whole. The page is JavaScript and the daemon's twin is Rust, so this table
 * is all that keeps the two spelling alike.
 */
const FILES = ['crates/cf-harness/tests/goldens/launch/tables.json', 'app/ui/core/console-text.js']
const [TABLES, SOURCE] = FILES.map((file) =>
  readFileSync(fileURLToPath(new URL(`../${file}`, import.meta.url)), 'utf8'),
)
const TABLE = JSON.parse(TABLES).consoleText

/** The surrogates are no characters of their own: a text holds pairs. */
const isSurrogate = (code) => code >= 0xd800 && code <= 0xdfff

describe("the page's console text", () => {
  it('runs on the Unicode the table was recorded on, so a difference is the page’s', () => {
    assert.equal(
      process.versions.unicode,
      TABLE.unicode,
      `the table was recorded on Unicode ${TABLE.unicode}, and this Node has ${process.versions.unicode}: ` +
        'which marks are letters, and how a mark decomposes, are the engine’s',
    )
  })

  it('spells every code point the console changes as the table has it', () => {
    assert.ok(TABLE.changed.length > 1000, `${TABLE.changed.length} code points are in the table`)
    const differ = TABLE.changed.flatMap(([code, spelled]) => {
      const carried = consoleText(String.fromCodePoint(code))
      return carried === spelled
        ? []
        : [`U+${code.toString(16).toUpperCase().padStart(4, '0')}: ${carried}, not ${spelled}`]
    })
    assert.deepEqual(differ, [])
  })

  it('leaves every other code point as it is', () => {
    const changed = new Set(TABLE.changed.map(([code]) => code))
    const differ = []
    for (let code = 0; code <= 0x10ffff; code += 1) {
      if (isSurrogate(code) || changed.has(code)) continue
      const alone = String.fromCodePoint(code)
      if (consoleText(alone) !== alone) differ.push(`U+${code.toString(16).toUpperCase()}`)
    }
    assert.deepEqual(differ, [])
  })

  it('spells the texts that compose or decompose as a whole as the table has them, and null as null', () => {
    assert.ok(
      TABLE.texts.some((row) => row.text === null),
      'the table holds null',
    )
    assert.ok(TABLE.texts.length > 5, `${TABLE.texts.length} texts are in the table`)
    for (const { text, console: spelled } of TABLE.texts) {
      assert.equal(consoleText(text), spelled, JSON.stringify(text))
    }
  })

  it('imports nothing, so the page loads it as it is', () => {
    assert.doesNotMatch(SOURCE, /^\s*import[\s{(*]/m)
    assert.doesNotMatch(SOURCE, /\bimport\(/)
    assert.doesNotMatch(SOURCE, /\brequire\(/)
  })
})
