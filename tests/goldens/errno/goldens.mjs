/**
 * libuv's error names and words as Node prints them on this system: what the
 * Rust `cf-base` is held to (`crates/cf-base/tests/errno.rs`). Node says a
 * failed file call as `<code>: <words>, <call> '<path>'`, and
 * `util.getSystemErrorMap()` holds every code libuv has, with the words it
 * says after it.
 *
 * The file is deterministic, so the unit suite holds the committed copy
 * equal to what this computes (tests/fs-error-goldens.test.mjs), and
 * `npm run goldens:errno` writes it again after a change of Node or of
 * libuv. One file a platform, `process.platform` naming it: the words are
 * the same everywhere, the numbers are not. On Unix a number is the negated
 * errno of the system for the names that have one (-13 is `EACCES`), and one
 * of libuv's own (-3000s, -4000s) for those that have none (`EAI_NONAME`,
 * `EOF`); on Windows every number is libuv's own.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { getSystemErrorMap } from 'node:util'

/** Every golden, by its path under crates/cf-base. */
export function errnoGoldens() {
  const errors = [...getSystemErrorMap()].map(([code, [name, words]]) => ({ code, name, words }))
  const lines = errors.map((error) => `    ${JSON.stringify(error)}`).join(',\n')
  return {
    files: {
      [`tests/goldens/errno/${process.platform}.json`]: `{\n  "errors": [\n${lines}\n  ]\n}\n`,
    },
  }
}

/** Writes every golden into `crate`. */
export function writeErrnoGoldens(crate) {
  const { files } = errnoGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const path = join(crate, ...relative.split('/'))
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, text)
  }
  return { written: Object.keys(files).length }
}
