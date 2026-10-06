#!/usr/bin/env node
/**
 * Two runs on copies of a real home, on both daemons (the flip's soak). The
 * copies' projects work in scratch folders and their windows are the rig's
 * stand-in Claude, so neither daemon reaches the home, a project folder or a
 * real harness; the home's own files are only copied, once. The copies are
 * removed unless `--keep`.
 *
 *  - Parity: each daemon starts on a copy of its own, resumes what was open
 *    there, and once nothing moves any more, what each shows is compared: the
 *    projects, each project's board, the inboxes of the chief and the human,
 *    and the agents file each leaves.
 *  - The round trip, the way back after the flip: one copy, Node's daemon, then
 *    the native one, then Node's, each stopped before the next. The native one
 *    writes: the restart's resume, and what a probe project the trip adds to
 *    the copy has it hand out and deliver. Each start must succeed and read
 *    what the one before left, and the ledger stays at the schema Node knows.
 *
 *   npm run live:home-parity [-- --home <dir>] [--only parity|trip] [--keep]
 *     [--looks <n>] [--every <ms>]
 *
 * The home defaults to the Candidate's (`~/.consensflow-candidate`); the live
 * app's is the owner's to name. A daemon has settled when `--looks` looks at it
 * in a row (5), `--every` milliseconds apart (1000), show the same: a pass of
 * its dispatcher takes a second, so less than that settles too soon. The same
 * trip runs on a home built by a fixture, for the suite
 * (tests/integration/home-round-trip.test.mjs); the code of both is
 * tests/integration/home-copies.mjs.
 */
import { existsSync, rmSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { parity, roundTrip, SETTLE, snapshot } from '../integration/home-copies.mjs'

const args = process.argv.slice(2)
const option = (name) => {
  const at = args.indexOf(name)
  return at === -1 ? undefined : args[at + 1]
}
const usage = (why) => {
  process.stderr.write(`${why}\n`)
  process.exit(2)
}
const HOME = option('--home') ?? join(homedir(), '.consensflow-candidate')
const KEEP = args.includes('--keep')
const ONLY = option('--only')
if (ONLY !== undefined && ONLY !== 'parity' && ONLY !== 'trip') usage('--only is parity or trip')
const settle = { ...SETTLE }
for (const [name, key] of [
  ['--looks', 'looks'],
  ['--every', 'everyMs'],
]) {
  if (option(name) === undefined) continue
  settle[key] = Number(option(name))
  if (!Number.isInteger(settle[key]) || settle[key] < 1) usage(`${name} is a whole number`)
}
if (!existsSync(join(HOME, 'consensflow.db')))
  usage(`no ledger in ${HOME}: name a home with --home`)

const say = (line) => process.stdout.write(`${line}\n`)
const NAMES = { node: 'Node', native: 'native' }
/** What a start came to, in a line. */
function line(ran, label) {
  const worked = ran.worked === undefined ? '' : `; handed out and delivered ${ran.worked}`
  say(
    `${label} (${ran.daemon.runtime}): ${ran.settled ? 'settled' : 'NOT settled'} in ${ran.seconds} s; ${ran.windows.length} windows; ${ran.errors.length} errors logged${worked}`,
  )
  for (const error of ran.errors.slice(0, 10)) say(`  ${error}`)
}

const taken = snapshot(HOME)
const roots = [taken]
let ok = true
try {
  if (ONLY !== 'trip') {
    const copies = await parity(taken, { settle })
    roots.push(...copies.roots)
    for (const [select, ran] of Object.entries(copies.runs)) {
      line(ran, NAMES[select])
      ok &&= ran.settled
    }
    const found = [...copies.differences, ...copies.problems]
    for (const difference of found.slice(0, 60)) say(`DIFF ${difference}`)
    say(`${found.length} differences`)
    ok &&= found.length === 0
  }
  if (ONLY !== 'parity') {
    say('Round trip, Node → native → Node, on one copy:')
    const trip = await roundTrip(taken, { settle })
    roots.push(trip.copy.root)
    for (const [at, ran] of trip.starts.entries()) line(ran, `${at + 1} ${NAMES[ran.select]}`)
    for (const problem of trip.problems.slice(0, 60)) say(`PROBLEM ${problem}`)
    say(`${trip.problems.length} problems`)
    ok &&= trip.problems.length === 0
  }
} catch (cause) {
  say(`FAILED ${cause?.message ?? cause}`)
  ok = false
} finally {
  for (const root of roots) {
    if (KEEP) say(`kept: ${root}`)
    else rmSync(root, { recursive: true, force: true })
  }
}
process.exit(ok ? 0 : 1)
