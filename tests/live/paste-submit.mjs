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
 * means the window sent it, and the harness's own record must then hold the
 * message whole, its marks included (— “” → € …), as the app reads it there.
 * Each goes in as the app gives it to that window: Devin on Windows gets its
 * marks in ASCII. Nothing presses Enter a second time: a message left
 * waiting in the input is a failure, the one Devin showed on Windows.
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
import { findSession } from '../../evals/bare.mjs'
import {
  codexIsolation,
  codexLiveFlags,
  HARNESSES,
  liveEnvironment,
  realOnPath,
} from '../../evals/plan.mjs'
import { answers } from '../../hosts/lib/completion.js'
import { interactiveStart } from '../../hosts/lib/windows.js'
import { consoleText, recordState, windowText } from '../../src/adapters/shared.js'
import { prepareClaudeSettings } from '../../src/claude-install.js'
import { onWindows, paneArgv, runnable } from '../../src/harnesses.js'
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
/** Where a harness looks for its records: the environment without the sandbox's removals. */
const RECORD_ENV = Object.fromEntries(Object.entries(ENV).filter(([, value]) => value !== null))
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
        '[ConsensFlow m-1 · T-1 · result from @worker]',
        'The page is done — “Contact” moved → the footer, €0 spent…',
        '',
        'Files changed: index.html.',
        line,
      ].join('\n'),
  },
  {
    name: 'a long message',
    sum: [3141, 2718],
    body: (line) => {
      const lines = ['[ConsensFlow m-2 · T-1 · result from @worker]']
      for (let n = 1; lines.join('\n').length < 3_700; n += 1) {
        lines.push(`${n}. Section ${n} — headings, “links” → footer: all match…`)
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

/**
 * The extra flags a scripted window needs: no MCP servers, connectors or
 * browser; and Claude's settings file as the app writes it for each launch,
 * which skips the full-permission warning (here, its home is the folder's own).
 */
async function isolation(name, executable) {
  if (name === 'claude') {
    const home = { ...RECORD_ENV, CONSENSFLOW_HOME: join(WORKSPACE, '.consensflow') }
    return [
      ...(await prepareClaudeSettings(home, 'paste', { boardQuestions: false })),
      '--strict-mcp-config',
      '--no-chrome',
    ]
  }
  if (name !== 'codex') return []
  const list = runnable(executable, ['mcp', 'list', '--json'])
  return [
    ...codexLiveFlags(),
    ...codexIsolation(
      JSON.parse(execFileSync(list.file, list.args, { ...list.options, encoding: 'utf8' })),
    ),
  ]
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
    /** The message as the app gives it to this window: Devin on Windows gets its marks in ASCII. */
    const given = (body) =>
      name === 'devin' && onWindows(ENV) ? consoleText(windowText(body)) : windowText(body)
    let native = session
    const openedAt = Date.now()
    /** The user messages the harness's own record holds so far. */
    const recorded = async () => {
      native ??= findSession(kind, {
        workspace: WORKSPACE,
        since: openedAt,
        home: H,
        env: RECORD_ENV,
      })
      if (native === null) return []
      const read = await answers(kind, native, RECORD_ENV).catch(() => null)
      if (read === null || read.unknown) return []
      return recordState(read).items.filter((item) => item.role === 'user')
    }
    const opened = await app.request('pane.open', {
      ...pane,
      cwd: WORKSPACE,
      argv: paneArgv([executable, ...(await isolation(name, executable)), ...start.args], ENV),
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
        const body = given(check.body(ask(check.sum)))
        const written = await app.request('pane.write_paste', { ...pane, body })
        let shown = false
        while (!shown && Date.now() - pasted < ANSWER_MS && !closed()) {
          await sleep(500)
          shown = screen().slice(from).includes(answer)
        }
        const seconds = ((Date.now() - pasted) / 1000).toFixed(1)
        // Its record holds what was pasted, whole: where the app looks for it.
        const whole = body.replace(/\r\n/g, '\n').trim()
        let users = []
        let kept = false
        for (let tries = 0; shown && !kept && tries < 20; tries += 1) {
          users = await recorded()
          kept = users.some((item) => item.text.replace(/\r\n/g, '\n').includes(whole))
          if (!kept) await sleep(500)
        }
        const escaped = (text) =>
          text.slice(0, 160).replace(/[^\x20-\x7e]/g, (c) => `\\u${c.charCodeAt(0).toString(16)}`)
        results.push({
          name,
          check: check.name,
          ok: written?.ok === true && shown && kept,
          detail: !shown
            ? `NOT SENT (${JSON.stringify(written)}): ${lastLines(screen())}`
            : kept
              ? `sent and recorded whole, answered in ${seconds} s`
              : `sent, but recorded otherwise: ${escaped(users.at(-1)?.text ?? '(no record found)')}`,
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
