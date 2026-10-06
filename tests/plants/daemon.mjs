/**
 * Plants bugs in the Rust daemon, one at a time, and checks that a test catches
 * each. A plant is a few pieces of text replaced in the sources; the tests that
 * should notice are run (never in parallel: the sources are changed under them)
 * and each plant is reported caught or missed. Every file a plant touches is
 * first copied outside the repository and is put back from that copy, byte for
 * byte, whatever the run came to, on Ctrl-C and on being terminated too; a run
 * killed past that leaves the copies in the folder it says first.
 *
 *   npm run plants:daemon                  # every plant
 *   npm run plants:daemon -- stop door     # the plants whose names hold a word
 *   npm run plants:daemon -- --check       # only that every plant still applies
 *
 * A plant that stops applying (the text it replaces was changed) is an error to
 * mend here, not a pass. One that does not compile, and one that makes a test
 * wait for ever (its run is ended after five minutes: a test that hangs where
 * it should fail), count as missed. Exit code 1 if any plant is missed or does
 * not apply.
 */
import { spawn } from 'node:child_process'
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { PLANTS as CONSTANTS } from './daemon/constants.mjs'
import { PLANTS as FRONT } from './daemon/front.mjs'
import { PLANTS as LAUNCHER } from './daemon/launcher.mjs'
import { PLANTS as PARTS } from './daemon/parts.mjs'
import { PLANTS as PAUSE } from './daemon/pause.mjs'
import { PLANTS as PLAYERS } from './daemon/players.mjs'
import { PLANTS as RUN } from './daemon/run.mjs'
import { PLANTS as SCREENS } from './daemon/screens.mjs'
import { PLANTS as SUPPORT } from './daemon/support.mjs'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
/** The longest one run of tests may take, in milliseconds: the slowest takes a minute. */
const RUN_LIMIT = 5 * 60 * 1000

/** Every plant, by area; what a plant is, is told in `daemon/kit.mjs`. */
const PLANTS = [
  ...RUN,
  ...FRONT,
  ...PARTS,
  ...CONSTANTS,
  ...SCREENS,
  ...SUPPORT,
  ...PLAYERS,
  ...LAUNCHER,
  ...PAUSE,
]

const args = process.argv.slice(2)
const checkOnly = args.includes('--check')
const words = args.filter((arg) => !arg.startsWith('--'))
const chosen = PLANTS.filter(
  (plant) => words.length === 0 || words.some((word) => plant.name.includes(word)),
)

/** `text` with `from` replaced by `to` where it is found exactly once. */
function replaced(plant, file, text, from, to) {
  const found = text.split(from).length - 1
  if (found !== 1) {
    throw new Error(`${plant.name}: ${file} holds ${JSON.stringify(from)} ${found} times, not once`)
  }
  return text.replace(from, () => to)
}

/** The text of each file `plant` touches once planted, by file. */
function planted(plant) {
  const files = new Map()
  for (const [file, from, to] of plant.edits) {
    const text = files.get(file) ?? readFileSync(join(REPO, file), 'utf8')
    files.set(file, replaced(plant, file, text, from, to))
  }
  return files
}

if (checkOnly) {
  for (const plant of chosen) planted(plant)
  process.stdout.write(`${chosen.length} plants apply\n`)
  process.exit(0)
}

const saved = mkdtempSync(join(tmpdir(), 'cf-plants-'))
process.stdout.write(`copies of what is planted are kept in ${saved}\n`)
/** What is planted now: its files, with the copies they go back from. */
let planting = null
let running = null

/** Puts every file of the plant in hand back from its copy, and says if one was not. */
function restore() {
  if (planting === null) return
  const { copies } = planting
  planting = null
  for (const { path, copy, original } of copies) {
    copyFileSync(copy, path)
    if (!readFileSync(path).equals(original)) {
      throw new Error(`${path} is not as it was: its copy is ${copy}`)
    }
  }
}

/** Ends the run of tests in hand, with every process it started. */
function endRun() {
  if (running === null) return
  try {
    process.kill(-running.pid, 'SIGKILL')
  } catch {}
}

function leave(code) {
  endRun()
  restore()
  rmSync(saved, { recursive: true, force: true })
  process.exit(code)
}
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => leave(130))

/**
 * One `cargo test`: its output, and what caught the plant if anything did: the
 * tests it says failed, or the run itself where a test binary died with no
 * test left to say so (a signal nothing was ready for).
 */
function cargoTest(runArgs) {
  return new Promise((resolve) => {
    const child = spawn('cargo', ['test', '--offline', ...runArgs], {
      cwd: REPO,
      detached: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    running = child
    let output = ''
    child.stdout.on('data', (data) => {
      output += data
    })
    child.stderr.on('data', (data) => {
      output += data
    })
    let hung = false
    const late = setTimeout(() => {
      hung = true
      endRun()
    }, RUN_LIMIT)
    child.on('close', (code) => {
      clearTimeout(late)
      running = null
      const compiled = !output.includes('could not compile')
      const caught = [...output.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((hit) => hit[1])
      const died = output.match(/process didn't exit successfully: `[^`]*` \(([^)]*)\)/)
      if (compiled && !hung && code !== 0 && caught.length === 0) {
        caught.push(`${runArgs.join(' ')}: the test binary died${died ? ` (${died[1]})` : ''}`)
      }
      resolve({ output, caught, compiled, hung, runArgs })
    })
  })
}

async function trial(plant) {
  const copies = []
  const edited = planted(plant)
  for (const [file, text] of edited) {
    const path = join(REPO, file)
    const copy = join(saved, `${copies.length}-${file.replaceAll('/', '_')}`)
    copyFileSync(path, copy)
    copies.push({ path, copy, original: readFileSync(path) })
    planting = { copies }
    writeFileSync(path, text)
  }
  try {
    let ran = null
    for (const runArgs of plant.runs) {
      ran = await cargoTest(runArgs)
      if (!ran.compiled) return { verdict: 'does not compile', ran }
      if (ran.hung) return { verdict: 'hung', ran }
      if (ran.caught.length > 0) return { verdict: 'caught', ran }
    }
    return { verdict: 'missed', ran }
  } finally {
    restore()
  }
}

let wrong = 0
for (const plant of chosen) {
  const started = Date.now()
  const { verdict, ran } = await trial(plant)
  const seconds = ((Date.now() - started) / 1000).toFixed(0)
  const by =
    verdict === 'caught'
      ? ` by ${ran.caught[0]}${ran.caught.length > 1 ? ` (+${ran.caught.length - 1})` : ''}${
          ran.caught.some((name) => name.endsWith(plant.meant)) ? '' : `, not by ${plant.meant}`
        }`
      : ''
  if (verdict !== 'caught') wrong += 1
  process.stdout.write(`${verdict.toUpperCase().padEnd(16)} ${plant.name} (${seconds} s)${by}\n`)
  if (verdict === 'does not compile') process.stdout.write(`${ran.output.slice(-1500)}\n`)
  if (verdict === 'hung') {
    process.stdout.write(`    ${ran.runArgs.join(' ')} did not end in ${RUN_LIMIT / 1000} s\n`)
  }
  if (verdict === 'missed') {
    // What the last run ran, to tell a test that passed from none that ran.
    const summary = ran.output
      .split('\n')
      .filter((line) => /^(running \d+ test|test result:)/.test(line))
    process.stdout.write(`${summary.map((line) => `    ${line}`).join('\n')}\n`)
  }
}
rmSync(saved, { recursive: true, force: true })
process.stdout.write(`${chosen.length - wrong} of ${chosen.length} plants caught\n`)
process.exit(wrong === 0 ? 0 : 1)
