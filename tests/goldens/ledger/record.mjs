/**
 * Records the ledger traces the Rust ledger replays: the unit suite runs
 * with the recording ledger (`hooks.mjs`, `recording.mjs`), and each ledger a
 * test opened becomes a trace of what it was asked and answered, written
 * gzipped to crates/cf-ledger/tests/traces/. A trace holds every value its
 * ledger was given or drew, the clock's readings and the session names
 * included, so it replays the same whatever ran it; its bytes differ from one
 * recording to the next (the launch ids the dispatcher's tests make), so it
 * is recorded again only when the Node ledger changes: npm run goldens:ledger.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
const OUT = join(REPO, 'crates', 'cf-ledger', 'tests', 'traces')
// A URL, not a path: `--import` reads `D:\\…` on Windows as a URL with the scheme `d:`.
const HOOKS = new URL('./hooks.mjs', import.meta.url).href
const SUITE = ['tests', join('tests', 'engine')].flatMap((dir) =>
  readdirSync(join(REPO, dir))
    .filter((file) => file.endsWith('.test.mjs'))
    .map((file) => join(dir, file)),
)

const recorded = mkdtempSync(join(tmpdir(), 'cf-ledger-traces-'))
try {
  const ran = spawnSync(process.execPath, ['--import', HOOKS, '--test', ...SUITE], {
    cwd: REPO,
    env: { ...process.env, CF_LEDGER_TRACES: recorded },
    stdio: ['ignore', 'ignore', 'inherit'],
  })
  if (ran.status !== 0) throw new Error(`the suite failed while recording (exit ${ran.status})`)
  rmSync(OUT, { recursive: true, force: true })
  mkdirSync(OUT, { recursive: true })
  const traces = readdirSync(recorded).sort()
  for (const trace of traces) {
    writeFileSync(
      join(OUT, `${trace}.gz`),
      gzipSync(readFileSync(join(recorded, trace)), { level: 9 }),
    )
  }
  process.stdout.write(`${traces.length} traces → ${OUT}\n`)
} finally {
  rmSync(recorded, { recursive: true, force: true })
}
