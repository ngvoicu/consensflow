/**
 * Writes the legacy CLI's goldens into crates/cf and crates/cf-base
 * (goldens.mjs says what each holds): `npm run goldens:cli`, after a change to
 * what they record, and once on each platform the tests run on.
 *
 * `--check` records again and says where the result differs from the files
 * that are checked in, writing nothing.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cliGoldens } from './goldens.mjs'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
const files = await cliGoldens()

if (process.argv.includes('--check')) {
  const differ = Object.entries(files).filter(([relative, text]) => {
    const file = join(REPO, ...relative.split('/'))
    return !existsSync(file) || readFileSync(file, 'utf8') !== text
  })
  for (const [relative] of differ) process.stdout.write(`${relative}: differs\n`)
  process.stdout.write(`${Object.keys(files).length} goldens recorded, ${differ.length} differ\n`)
  process.exitCode = differ.length === 0 ? 0 : 1
} else {
  for (const [relative, text] of Object.entries(files)) {
    const file = join(REPO, ...relative.split('/'))
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, text)
  }
  process.stdout.write(`${Object.keys(files).length} goldens → ${REPO}\n`)
}
