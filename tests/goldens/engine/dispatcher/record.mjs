/**
 * Records the dispatcher's traces the Rust engine is held to: the dispatcher's
 * suite runs with the recording dispatcher (`hooks.mjs`), and each test's
 * trace, what the engine asked of its seams in order and the database it
 * left, is written gzipped to crates/cf-engine/tests/traces/:
 * npm run goldens:dispatcher, after a change to the Node engine.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'

const REPO = fileURLToPath(new URL('../../../..', import.meta.url))
const OUT = join(REPO, 'crates', 'cf-engine', 'tests', 'traces')
// A URL, not a path: `--import` reads `D:\\…` on Windows as a URL with the scheme `d:`.
const HOOKS = new URL('./hooks.mjs', import.meta.url).href
const SUITE = [join('tests', 'core-dispatcher.test.mjs')]

const recorded = mkdtempSync(join(tmpdir(), 'cf-engine-traces-'))
try {
  const ran = spawnSync(process.execPath, ['--import', HOOKS, '--test', ...SUITE], {
    cwd: REPO,
    env: { ...process.env, CF_ENGINE_TRACES: recorded },
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
