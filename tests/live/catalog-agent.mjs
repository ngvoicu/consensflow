/**
 * Live: does a catalog agent's window open on its model and answer?
 *
 * Each Claude Code agent named opens as the app opens it: the row the launcher
 * runs for it (`agentRow`, its model and effort as the catalog has them), in a
 * window of the app's own pane host, on real Claude Code with no MCP servers,
 * connectors or browser (`--strict-mcp-config --no-chrome`) and without
 * `CLAUDE_CONFIG_DIR`, in a folder of its own under the user's home. It is
 * asked for a sum its message does not hold, so the answer on the screen means
 * the window took the message; Claude Code's own record of the session must
 * then name the agent's model as the one that answered, and no other.
 * `--effort` opens the agent at each level named instead of its own: the
 * levels a row of that model could take.
 *
 *   npm run live:agent                          huginn, Haiku 5.5 at its own level
 *   npm run live:agent -- --agent huginn --agent hermod
 *   npm run live:agent -- --agent huginn --effort low --effort medium --effort high \
 *     --effort xhigh --effort max
 *
 * They run one after another; the exit code is 1 when any window did not open,
 * did not answer, or was answered by another model.
 */
import { readFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { claudeTranscript } from '../../hosts/lib/completion/claude-code.js'
import { agentRow } from '../../src/roster.js'
import { tempEnv } from '../helpers.mjs'
import { ENV, lastLines, openWindow, READY_MS, send, sleep, startLiveApp } from './live-window.mjs'

const { values } = parseArgs({
  options: {
    agent: { type: 'string', multiple: true },
    effort: { type: 'string', multiple: true },
  },
})

/** The catalog's row for each agent named, read in a home that holds nothing of the human's. */
function rowsOf(names) {
  const t = tempEnv()
  try {
    return names.map((name) => {
      const row = agentRow(name, t.env)
      if (row === undefined) throw new Error(`${name} is no agent of the catalog`)
      if (row.kind !== 'claude-code') {
        throw new Error(`${name} runs on ${row.kind}: this check opens Claude Code agents`)
      }
      return row
    })
  } finally {
    t.cleanup()
  }
}

/** The models Claude Code recorded as answering in the session, once its transcript holds an answer. */
async function answeredBy(session) {
  for (let tries = 0; tries < 20; tries += 1) {
    const file = await claudeTranscript(session, ENV)
    const models = new Set()
    if (file !== null) {
      for (const line of readFileSync(file, 'utf8').split('\n')) {
        let record = null
        try {
          record = JSON.parse(line)
        } catch {}
        const model = record?.type === 'assistant' ? record.message?.model : undefined
        if (typeof model === 'string' && model !== '<synthetic>') models.add(model)
      }
    }
    if (models.size > 0) return [...models]
    await sleep(500)
  }
  return []
}

const ASK = 'Reply with only the sum of 1234 and 4321, in digits, and run no tools.'
const rows = rowsOf(values.agent ?? ['huginn'])
const app = await startLiveApp()
const results = []
try {
  for (const row of rows) {
    for (const effort of values.effort ?? [row.effort]) {
      const name = `${row.id} ${effort ?? 'default'}`
      const window = await openWindow(app, 'claude', {
        folder: 'agent',
        id: `agent-${row.id}-${effort ?? 'default'}`,
        agent: { kind: row.kind, model: row.model, ...(effort ? { effort } : {}) },
      })
      if (window.trust !== null) process.stdout.write(`trust: ${window.trust}\n`)
      try {
        if (!(await window.still(READY_MS))) {
          results.push({ name, ok: false, detail: `did not open: ${lastLines(window.screen())}` })
          continue
        }
        const { shown, seconds } = await send(app, window, window.given(ASK), '5555')
        if (!shown) {
          results.push({ name, ok: false, detail: `NOT ANSWERED: ${lastLines(window.screen())}` })
          continue
        }
        const models = await answeredBy(window.session)
        const ok = models.length === 1 && models[0] === row.model
        results.push({
          name,
          ok,
          detail: ok
            ? `answered in ${seconds} s, as ${row.model}`
            : `answered, but its record names ${models.join(', ') || 'no model'}, not ${row.model}`,
        })
      } finally {
        await window.kill()
      }
    }
  }
} finally {
  await app.close()
}

for (const { name, ok, detail } of results) {
  process.stdout.write(`${ok ? 'ok  ' : 'FAIL'} ${name.padEnd(16)} ${detail}\n`)
}
process.exit(results.every((result) => result.ok) ? 0 : 1)
