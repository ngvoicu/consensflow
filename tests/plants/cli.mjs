/**
 * Plants bugs in the standalone verbs of `cf`, the parser they read their words
 * with, the roster they write through, the recording that holds them to what
 * Node's CLI said (fixed since Node went), what the flip release left (the
 * repair of the terminal's command) and the bundle without Node (nothing reads
 * the file the flip read, an npm shim with no Node is refused, the daemon the app
 * starts, the update's check, the page's console text), one at a time, and checks
 * that a test catches each. A plant is a few pieces of
 * text replaced in the sources; the tests that should notice are run (never in
 * parallel: the sources are changed under them) and each plant is reported
 * caught or missed. Every file a plant touches is first copied outside the
 * repository and is put back from that copy, byte for byte, whatever the run
 * came to, on Ctrl-C and on being terminated too; a run killed past that leaves
 * the copies in the folder it says first. The native `cf` that `bin/` holds,
 * which the test scripts run, is built again from the sources as they are after
 * each plant that had it built from its own, so that no plant's tests meet the
 * build of another's.
 *
 *   npm run plants:cli                  # every plant
 *   npm run plants:cli -- parser dispatch   # the plants whose names hold a word
 *   npm run plants:cli -- --check       # only that every plant still applies
 *
 * A plant that stops applying (the text it replaces was changed) is an error to
 * mend there, not a pass. One that does not compile, and one that makes a run
 * wait for ever (it is ended after ten minutes), count as missed. Exit code 1 if
 * any plant is missed or does not apply. The same shape as `plants:daemon`, which
 * runs `cargo test` alone: a run here is any command, a script's test among them.
 */
import { spawn, spawnSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { PLANTS as ADMIN } from './cli/admin.mjs'
import { PLANTS as DELETION } from './cli/deletion.mjs'
import { PLANTS as DISPATCH } from './cli/dispatch.mjs'
import { PLANTS as FLIP } from './cli/flip.mjs'
import { BUILD, CLIS } from './cli/kit.mjs'
import { PLANTS as PARSER } from './cli/parser.mjs'
import { PLANTS as VERBS } from './cli/verbs.mjs'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
/** The longest one run may take, in milliseconds: the suites' runner builds a release `cf`. */
const RUN_LIMIT = 10 * 60 * 1000

/** Every plant, by area. */
const PLANTS = [...PARSER, ...VERBS, ...ADMIN, ...DISPATCH, ...FLIP, ...DELETION]

const args = process.argv.slice(2)
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

if (args.includes('--check')) {
  for (const plant of chosen) planted(plant)
  process.stdout.write(`${chosen.length} plants apply\n`)
  process.exit(0)
}

const saved = mkdtempSync(join(tmpdir(), 'cf-plants-'))
process.stdout.write(`copies of what is planted are kept in ${saved}\n`)
/** What is planted now: its files, with the copies they go back from. */
let planting = null
let running = null
/** Whether a run built the native `cf` of `bin/` from planted sources, which stays there until `rebuild`. */
let built = false

/**
 * Builds the native `cf` of `bin/` again from the sources as they are, if a run
 * built it from planted ones: whether it is as the sources say.
 */
function rebuild() {
  if (!built) return true
  built = false
  const again = spawnSync(
    process.execPath,
    [join(REPO, 'app', 'scripts', 'build-cf.mjs'), '--offline'],
    { cwd: REPO, encoding: 'utf8' },
  )
  if (again.status !== 0) {
    process.stdout.write(
      `the native cf of bin/ is not built again:\n${again.stdout}${again.stderr}\n`,
    )
  }
  return again.status === 0
}

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

/** Ends the run in hand, with every process it started. */
function endRun() {
  if (running === null) return
  try {
    process.kill(-running.pid, 'SIGKILL')
  } catch {}
}

function leave(code) {
  endRun()
  restore()
  rebuild()
  rmSync(saved, { recursive: true, force: true })
  process.exit(code)
}
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => leave(130))

/** The tests a run's output says failed: `cargo test`'s and `node --test`'s. */
function failed(output) {
  const names = [...output.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((hit) => hit[1])
  for (const hit of output.matchAll(/^\s*✖ (.+?) \(\d[\d.]*ms\)$/gm)) names.push(hit[1])
  return [...new Set(names)]
}

/**
 * One run: its output, and what caught the plant if anything did: the tests it
 * says failed, or the run itself where it ended badly with no test left to say so.
 */
function execute(command) {
  const [program, ...runArgs] = command
  return new Promise((resolve) => {
    const child = spawn(program, runArgs, {
      cwd: REPO,
      detached: true,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, NODE_TEST_CONTEXT: undefined },
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
      const compiled = !output.includes('could not compile') && !/error\[E\d+\]/.test(output)
      const caught = failed(output)
      if (compiled && !hung && code !== 0 && caught.length === 0) {
        caught.push(`${command.join(' ')}: ended with ${code}`)
      }
      resolve({ output, caught, compiled, hung, command })
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
    for (const command of plant.runs) {
      built = built || command === CLIS || command === BUILD
      ran = await execute(command)
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
          ran.caught.some((name) => name.includes(plant.meant)) ? '' : `, not by ${plant.meant}`
        }`
      : ''
  if (verdict !== 'caught') wrong += 1
  process.stdout.write(`${verdict.toUpperCase().padEnd(16)} ${plant.name} (${seconds} s)${by}\n`)
  if (verdict === 'does not compile') process.stdout.write(`${ran.output.slice(-1500)}\n`)
  if (verdict === 'hung') {
    process.stdout.write(`    ${ran.command.join(' ')} did not end in ${RUN_LIMIT / 1000} s\n`)
  }
  if (verdict === 'missed') {
    // What the last run ran, to tell a test that passed from none that ran.
    const summary = ran.output
      .split('\n')
      .filter((line) => /^(running \d+ test|test result:|ℹ (tests|pass|fail))/.test(line))
    process.stdout.write(`${summary.map((line) => `    ${line}`).join('\n')}\n`)
  }
  if (!rebuild()) wrong += 1
}
rmSync(saved, { recursive: true, force: true })
process.stdout.write(`${chosen.length - wrong} of ${chosen.length} plants caught\n`)
process.exit(wrong === 0 ? 0 : 1)
