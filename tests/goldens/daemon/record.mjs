/**
 * Records what Node answers on every surface of the daemon, from the suites
 * that exist, for the Rust players to be held to: the suites run with the
 * recorder in (`hooks.mjs`) and each test that reached a surface leaves one
 * trace, written gzipped to crates/cf-daemon/tests/goldens/, beside the files
 * `files.mjs` makes (`FORMAT.md` says what is in all of them, and is rewritten
 * where it shows them: `document.mjs`). A recording made twice is the same
 * bytes twice: `--check` records again and says where it differs from what is
 * checked in, the document included. A recording `--to` a folder of its own, or
 * of some suites only (`--only`), leaves the document as it is.
 *
 *   npm run goldens:daemon
 *   node tests/goldens/daemon/record.mjs [--check] [--to <folder>] [--only <part of a suite's path>]
 */
import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { gunzipSync, gzipSync } from 'node:zlib'
import { check } from './check.mjs'
import { refreshed } from './document.mjs'
import { dataFiles, referred, templates } from './files.mjs'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
// A URL, not a path: `--import` reads `D:\\…` on Windows as a URL with the scheme `d:`.
const HOOKS = new URL('./hooks.mjs', import.meta.url).href
const GOLDENS = join(REPO, 'crates', 'cf-daemon', 'tests', 'goldens')
const FORMAT = fileURLToPath(new URL('./FORMAT.md', import.meta.url))

/**
 * The suites whose tests are recorded: the daemon's surfaces, as their own
 * tests drive them, and the corners of them no suite looked at (`scenarios/`,
 * kept here and not in `tests/`: the ledger recorder takes every suite there,
 * and the odd numbers these send the ledger are not what its replay reads).
 */
const SUITES = [
  'tests/core-api.test.mjs',
  'tests/core-daemon.test.mjs',
  'tests/core-agents-server.test.mjs',
  'tests/core-page.test.mjs',
  'tests/core-trace.test.mjs',
  'tests/core-log.test.mjs',
  'tests/integration/cf-board.test.mjs',
  'tests/goldens/daemon/scenarios/corners-api.test.mjs',
  'tests/goldens/daemon/scenarios/corners-page.test.mjs',
  'tests/goldens/daemon/scenarios/corners-screens.test.mjs',
]

function option(name) {
  const at = process.argv.indexOf(`--${name}`)
  return at < 0 ? undefined : process.argv[at + 1]
}

/** The files `suites` leave when they run with the recorder in, by name: each trace, as the text it is. */
async function record(suites) {
  const recorded = mkdtempSync(join(tmpdir(), 'cf-daemon-traces-'))
  try {
    const env = { ...process.env, CF_DAEMON_TRACES: recorded }
    delete env.CF_LEDGER_TRACES
    // The suites are a test runner of their own, even when a test runs this.
    delete env.NODE_TEST_CONTEXT
    const ran = spawnSync(
      process.execPath,
      ['--import', HOOKS, '--test', '--test-concurrency=1', ...suites],
      { cwd: REPO, env, stdio: ['ignore', 'ignore', 'inherit'] },
    )
    if (ran.status !== 0) throw new Error(`the suites failed while recording (exit ${ran.status})`)
    const pages = await templates()
    const files = {}
    for (const name of readdirSync(recorded).sort()) {
      const trace = referred(JSON.parse(readFileSync(join(recorded, name), 'utf8')), pages)
      check(name, trace)
      files[`${name}.gz`] = `${JSON.stringify(trace)}\n`
    }
    return files
  } finally {
    rmSync(recorded, { recursive: true, force: true })
  }
}

/** What is in `folder`: each file by its path under it, a trace as its text. */
function read(folder) {
  const files = {}
  const walk = (at) => {
    for (const item of readdirSync(at, { withFileTypes: true })) {
      const path = join(at, item.name)
      if (item.isDirectory()) walk(path)
      else {
        const name = relative(folder, path).split(sep).join('/')
        const bytes = readFileSync(path)
        files[name] = name.endsWith('.gz')
          ? gunzipSync(bytes).toString('utf8')
          : bytes.toString('utf8')
      }
    }
  }
  if (existsSync(folder)) walk(folder)
  return files
}

/** Where two sets of files differ, in words. */
function differences(had, made) {
  const found = []
  for (const name of new Set([...Object.keys(had), ...Object.keys(made)])) {
    if (had[name] === undefined) found.push(`${name}: not there, and recorded now`)
    else if (made[name] === undefined) found.push(`${name}: there, and not recorded now`)
    else if (had[name] !== made[name]) found.push(`${name}: differs`)
  }
  return found
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const suites = SUITES.filter((suite) => suite.includes(option('only') ?? ''))
  const native = join(REPO, 'bin', process.platform === 'win32' ? 'cf.exe' : 'cf')
  if (suites.some((suite) => suite.includes('cf-board')) && !existsSync(native)) {
    throw new Error(`no native cf at ${native}: node app/scripts/build-cf.mjs first`)
  }
  const made = { ...(await record(suites)), ...(await dataFiles()) }
  const out = resolve(option('to') ?? GOLDENS)
  const whole = option('to') === undefined && option('only') === undefined
  const document = whole ? readFileSync(FORMAT, 'utf8') : null
  const rewritten = whole ? refreshed(document, made) : null
  if (process.argv.includes('--check')) {
    const found = differences(read(out), made)
    if (whole && rewritten !== document) found.push('FORMAT.md: differs')
    for (const line of found) process.stdout.write(`${line}\n`)
    process.stdout.write(
      `${Object.keys(made).length} files recorded, ${found.length} differ from ${out}\n`,
    )
    process.exitCode = found.length === 0 ? 0 : 1
  } else {
    rmSync(out, { recursive: true, force: true })
    for (const [name, text] of Object.entries(made)) {
      mkdirSync(dirname(join(out, name)), { recursive: true })
      writeFileSync(join(out, name), name.endsWith('.gz') ? gzipSync(text, { level: 9 }) : text)
    }
    process.stdout.write(`${Object.keys(made).length} files → ${out}\n`)
    if (whole && rewritten !== document) {
      writeFileSync(FORMAT, rewritten)
      process.stdout.write('FORMAT.md rewritten where it shows the traces\n')
    }
  }
}
