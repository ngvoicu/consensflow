/**
 * Live: does a harness's window send what ConsensFlow pastes into it?
 *
 * Each harness opens in a window of the app's own pane host, started the way
 * the app starts it, in a folder of its own under the user's home, on its
 * cheap eval model. Three messages go in the way the daemon sends one
 * (`pane.write_paste`: a bracketed paste, then Enter once the window has
 * drawn it): one line, a few lines shaped like a ConsensFlow message, and one
 * of about 3,800 characters, which Devin and Claude fold into a placeholder.
 * Each asks for a sum the message does not hold, so its answer on the screen
 * means the window sent it. Nothing presses Enter a second time: a message
 * left waiting in the input is a failure, the one Devin showed on Windows.
 *
 *   npm run live:paste                      Devin
 *   npm run live:paste -- --harness claude --harness codex
 *
 * Harnesses: claude, codex, devin, opencode, pi. They run one after another;
 * the exit code is 1 when any message was not sent.
 */
import { execFileSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { codexIsolation, HARNESSES, liveEnvironment, realOnPath } from '../../evals/plan.mjs'
import { interactiveStart } from '../../hosts/lib/windows.js'
import { paneArgv, runnable } from '../../src/harnesses.js'
import { startIntegration } from '../integration/harness.mjs'
import { trustForClaude } from './trust-claude.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const EDITOR = join(HERE, 'core-live-editor.mjs')
const { values } = parseArgs({ options: { harness: { type: 'string', multiple: true } } })
const harnesses = values.harness ?? ['devin']
for (const name of harnesses) {
  if (!(name in HARNESSES)) throw new Error(`no such harness: ${name}`)
}
const H = process.env.HOME ?? homedir()
const ENV = liveEnvironment({ home: H })
const WORKSPACE = join(H, '.consensflow-candidate', 'live', 'paste')
mkdirSync(WORKSPACE, { recursive: true })

/** How long a window may take to draw its prompt, and a sent message its answer. */
const READY_MS = 120_000
const ANSWER_MS = 180_000
/** How long a window holds still before it counts as drawn, or done with its turn. */
const STILL_MS = 3_000

const ask = ([a, b]) => `Reply with only the sum of ${a} and ${b}, in digits, and run no tools.`
const CASES = [
  { name: 'one line', sum: [1234, 4321], body: (line) => line },
  {
    name: 'a ConsensFlow message',
    sum: [2468, 1357],
    body: (line) =>
      [
        '[ConsensFlow m-1 · result from @worker · T-1]',
        'The page is done.',
        '',
        'Files changed: index.html.',
        line,
      ].join('\n'),
  },
  {
    name: 'a long message',
    sum: [3141, 2718],
    body: (line) => {
      const lines = ['[ConsensFlow m-2 · result from @worker · T-1]']
      for (let n = 1; lines.join('\n').length < 3_700; n += 1) {
        lines.push(`${n}. Checked section ${n} of the page: its headings, links and footer match.`)
      }
      return [...lines, line].join('\n')
    },
  },
]

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const lastLines = (text) =>
  text
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(-8)
    .join(' ⏎ ')

/** The extra flags a scripted window needs: no MCP servers, connectors or browser. */
function isolation(name, executable) {
  if (name === 'claude') return ['--strict-mcp-config', '--no-chrome']
  if (name !== 'codex') return []
  const list = runnable(executable, ['mcp', 'list', '--json'])
  return codexIsolation(
    JSON.parse(execFileSync(list.file, list.args, { ...list.options, encoding: 'utf8' })),
  )
}

const app = await startIntegration({ editor: EDITOR, fakeEnv: ENV })
const results = []
try {
  for (const name of harnesses) {
    const { kind, model } = HARNESSES[name]
    // Claude and Pi open on an id they are given; the others name their own.
    const session = kind === 'claude-code' || kind === 'pi' ? randomUUID() : null
    const start = interactiveStart({ kind, model }, session, null)
    const executable = realOnPath(start.command, ENV.PATH)
    if (name === 'claude') {
      process.stdout.write(`trust: ${await trustForClaude(app, WORKSPACE, executable)}\n`)
    }
    const pane = { id: `paste-${name}`, generation: 1 }
    const opened = await app.request('pane.open', {
      ...pane,
      cwd: WORKSPACE,
      argv: paneArgv([executable, ...start.args, ...isolation(name, executable)], ENV),
      env: start.env,
      dropEnv: start.dropEnv,
      size: { rows: 40, cols: 120 },
    })
    if (opened?.ok !== true) throw new Error(`${name} did not open: ${JSON.stringify(opened)}`)
    const screen = () => app.output(pane.id)
    const closed = () => app.exits.some((exit) => exit.id === pane.id)
    /** Until the window has printed and then held still, or `ms` passed; whether it did. */
    const still = async (ms) => {
      const end = Date.now() + ms
      let seen = -1
      let since = Date.now()
      while (Date.now() < end && !closed()) {
        const length = screen().length
        if (length !== seen) [seen, since] = [length, Date.now()]
        else if (length > 0 && Date.now() - since >= STILL_MS) return true
        await sleep(200)
      }
      return false
    }
    try {
      if (!(await still(READY_MS))) {
        results.push({ name, check: 'opens', ok: false, detail: lastLines(screen()) })
        continue
      }
      for (const check of CASES) {
        const answer = String(check.sum[0] + check.sum[1])
        const from = screen().length
        const pasted = Date.now()
        const written = await app.request('pane.write_paste', {
          ...pane,
          body: check.body(ask(check.sum)),
        })
        let shown = false
        while (!shown && Date.now() - pasted < ANSWER_MS && !closed()) {
          await sleep(500)
          shown = screen().slice(from).includes(answer)
        }
        results.push({
          name,
          check: check.name,
          ok: written?.ok === true && shown,
          detail: shown
            ? `sent, answered in ${((Date.now() - pasted) / 1000).toFixed(1)} s`
            : `NOT SENT (${JSON.stringify(written)}): ${lastLines(screen())}`,
        })
        if (!shown) break
        await still(ANSWER_MS)
      }
    } finally {
      await app.request('pane.kill', pane).catch(() => {})
    }
  }
} finally {
  await app.close()
}

for (const { name, check, ok, detail } of results) {
  process.stdout.write(`${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(9)} ${check.padEnd(22)} ${detail}\n`)
}
process.exit(results.every((result) => result.ok) ? 0 : 1)
