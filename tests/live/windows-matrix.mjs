/**
 * The eval matrix on a Windows machine: each scenario for each chief and
 * staff pair, one run after another through `npm run windows`, each run's
 * output kept in evals/reports/windows/ here (the run's own report stays on
 * the machine), and one line per run at the end: PASS when every check
 * passed, else the checks that failed. The exit code is 1 when any run did
 * not pass.
 *
 *   npm run eval:windows -- --host <ssh host> [--build] [--env NAME=VALUE] \
 *     --scenario round-trip --scenario question-trip \
 *     --pair devin:devin --pair claude:devin [--claude-model claude-sonnet-5] [--pi-model openai/gpt-5.6-luna]
 *
 * --build builds the machine's copy before the first run; --claude-model is
 * a Claude chief's model (the eval's own default otherwise). --env NAME=VALUE
 * goes to each run, as `npm run windows` takes it (tests/live/windows.mjs):
 * `--env CONSENSFLOW_TEST_DAEMON=native` has every eval start the native daemon
 * on the machine, where the rig reads it (tests/choice.mjs). Each eval says which
 * daemon it ran against, and the line of its run here ends with it.
 */
import { spawn } from 'node:child_process'
import { createWriteStream, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { parseEnv } from './windows-script.mjs'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values } = parseArgs({
  options: {
    host: { type: 'string' },
    build: { type: 'boolean', default: false },
    scenario: { type: 'string', multiple: true },
    pair: { type: 'string', multiple: true },
    'claude-model': { type: 'string' },
    'pi-model': { type: 'string' },
    env: { type: 'string', multiple: true, default: [] },
  },
})
if (!values.host || !values.scenario || !values.pair) {
  throw new Error(
    'usage: npm run eval:windows -- --host <ssh host> [--env NAME=VALUE] --scenario <name>… --pair <chief>:<staff>…',
  )
}
// Named well before the first run, not by the first one that fails on it.
parseEnv(values.env)
const pairs = values.pair.map((pair) => {
  const [chief, staff] = pair.split(':')
  if (!chief || !staff) throw new Error(`a pair is chief:staff, not ${pair}`)
  return { chief, staff }
})
const folder = join(
  REPO,
  'evals',
  'reports',
  'windows',
  new Date().toISOString().slice(0, 19).replaceAll(':', '-'),
)
mkdirSync(folder, { recursive: true })

/** One eval on the machine, its output kept in `log`; what its checks said. */
async function run({ scenario, chief, staff }, build, log) {
  const args = ['run', 'windows', '--', '--host', values.host, ...(build ? ['--build'] : [])]
  for (const entry of values.env) args.push('--env', entry)
  args.push('--', 'npm', 'run', 'eval', '--', '--scenario', scenario, '--chief', chief)
  args.push('--staff', staff, '--timeout-min', '30')
  // A chief switch goes to the staff's harness.
  if (scenario === 'chief-switch') args.push('--switch-to', staff)
  if (chief === 'claude' && values['claude-model']) args.push('--model', values['claude-model'])
  // A Pi member's model, on the machine's own signed-in provider (zeewin: openai/gpt-5.6-luna).
  if (staff.split(',').includes('pi') && values['pi-model'])
    args.push('--pi-staff-model', values['pi-model'])
  const out = createWriteStream(log)
  const child = spawn('npm', args, { cwd: REPO, stdio: ['ignore', 'pipe', 'pipe'] })
  let text = ''
  for (const stream of [child.stdout, child.stderr]) {
    stream.on('data', (chunk) => {
      text += chunk
      out.write(chunk)
    })
  }
  const code = await new Promise((resolve) => child.on('exit', (exit) => resolve(exit ?? 1)))
  out.end()
  const failed = [...text.matchAll(/^\s*FAIL (.+)$/gm)].map((match) => match[1])
  const checks = (text.match(/^\s*(PASS|FAIL) /gm) ?? []).length
  // The eval says which daemon it ran against (`daemon: native (rust 3.0.0)`).
  const daemon = /^daemon: (.+)$/m.exec(text)?.[1]
  const on = daemon === undefined ? '' : ` · daemon ${daemon}`
  if (checks === 0) return { ok: false, line: `no verdict (exit ${code}): see ${log}${on}` }
  return failed.length === 0
    ? { ok: true, line: `PASS ${checks} checks${on}` }
    : { ok: false, line: `FAIL ${failed.join(' · ')}${on}` }
}

const results = []
// Built once, before the first run that goes.
let build = values.build
for (const scenario of values.scenario) {
  for (const pair of pairs) {
    const name = `${scenario} ${pair.chief}:${pair.staff}`
    if (scenario === 'chief-switch' && pair.chief === pair.staff) {
      results.push({
        name,
        ok: true,
        line: 'skipped: a chief switch needs another harness on the staff',
      })
      continue
    }
    process.stdout.write(`${name}…\n`)
    const log = join(folder, `${scenario}-${pair.chief}-${pair.staff}.log`)
    const result = await run({ scenario, ...pair }, build, log)
    build = false
    results.push({ name, ...result })
    process.stdout.write(`  ${result.line}\n`)
  }
}
process.stdout.write(`\nlogs: ${folder}\n`)
for (const { name, line } of results) process.stdout.write(`${name.padEnd(36)} ${line}\n`)
process.exitCode = results.every((result) => result.ok) ? 0 : 1
