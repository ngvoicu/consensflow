/**
 * The goldens that are files and not traces, made from Node as it is now: the
 * two pages the screens serve, with `$TOKEN` where the UI token goes and
 * `$VERSION` where the version does; the names of the page operations in the
 * order the page offers them; the handle line the daemon prints for the app,
 * and the lines it writes to its log when it starts and stops. The traces
 * name the pages by file (`response.page`), so a player compares what it
 * serves with these, filled in. The tests hold the checked-in files to what
 * this makes: `npm run goldens:daemon` after a change.
 */
import { spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { agentsUi } from '../../../src/core/agents-server.js'
import { pageOperations } from '../../../src/core/page.js'

const VERSION = JSON.parse(
  readFileSync(new URL('../../../package.json', import.meta.url), 'utf8'),
).version
const DAEMON = fileURLToPath(new URL('../../integration/core-daemon.mjs', import.meta.url))

const occurrences = (text, part) => text.split(part).length - 1

/** A screen as Node serves it for the token `$TOKEN`, the version put back as `$VERSION`. */
async function page(path) {
  const ui = agentsUi({}, { token: '$TOKEN' })
  const request = { method: 'GET', headers: { authorization: 'Bearer $TOKEN' } }
  const { html } = await ui.handle(request, new URL(path, 'http://127.0.0.1'))
  if (occurrences(html, '$TOKEN') !== 1)
    throw new Error(`${path} names its token ${occurrences(html, '$TOKEN')} times, not once`)
  if (path === '/harnesses') return html
  if (occurrences(html, VERSION) !== 1) {
    throw new Error(`${path} holds the version ${occurrences(html, VERSION)} times, not once`)
  }
  return html.replace(`v${VERSION}</span>`, 'v$VERSION</span>')
}

/** A page as a player serves it: `$TOKEN` and `$VERSION` filled in, as the screens fill them. */
export function filled(template, token) {
  return template
    .split('$TOKEN')
    .join(JSON.stringify(token).slice(1, -1))
    .split('$VERSION')
    .join(VERSION)
}

/** The pages as `dataFiles` writes them, by name. */
export const templates = async () => ({
  agents: await page('/'),
  harnesses: await page('/harnesses'),
})

/**
 * A trace with each page its exchanges served named by file (`response.page`)
 * in place of its text; throws when the text is not the page Node makes for
 * the trace's UI token, filled in. The screens' pages are 26 KB each: they are
 * written once, beside the traces.
 */
export function referred(trace, pages) {
  const names = { '/': 'agents', '/harnesses': 'harnesses' }
  for (const step of trace.steps) {
    const answer = step.kind === 'exchange' ? step.response : null
    if (answer === null || !answer.contentType?.startsWith('text/html')) continue
    const name = names[step.request.target.split('?')[0]]
    if (name === undefined || answer.body === undefined) {
      throw new Error(`${step.request.target} served a page the recorder does not know`)
    }
    if (answer.body !== filled(pages[name], trace.ui.token)) {
      throw new Error(`${step.request.target} served a page that is not the one Node makes`)
    }
    const { body: _text, ...rest } = answer
    step.response = { ...rest, page: name }
  }
  return trace
}

/**
 * The operation names, in the order `pageOperations` has them, and the one
 * the daemon answers itself (`ping`, `daemon.js`): the reply it gives.
 */
const operations = () => ({
  operations: Object.keys(pageOperations({ ledger: {}, dispatcher: {}, env: {}, kick() {} })),
  ping: JSON.stringify({ ok: true }),
})

/**
 * What the daemon prints and writes when it starts and stops, from a real run
 * on a home of its own: the handle line with its port and token named, and the
 * log's lines with their times, pid, home, Node's version and memory named.
 */
async function daemon() {
  const home = mkdtempSync(join(tmpdir(), 'cf-daemon-handle-'))
  try {
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([name]) => !/^(CONSENSFLOW_|CF_)/.test(name)),
    )
    const child = spawn(process.execPath, [DAEMON], {
      env: { ...env, HOME: home, CONSENSFLOW_HOME: home, CLAUDE_CONFIG_DIR: join(home, '.claude') },
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    let out = ''
    let errors = ''
    child.stderr.on('data', (chunk) => {
      errors += chunk
    })
    await new Promise((resolve) => {
      child.stdout.on('data', (chunk) => {
        out += chunk
        if (out.includes('\n')) resolve()
      })
    })
    child.stdin.end()
    const code = await new Promise((resolve) => child.once('exit', resolve))
    if (code !== 0) throw new Error(`the daemon exited ${code}: ${errors}`)
    const handle = /^\{"url":"http:\/\/127\.0\.0\.1:(\d+)\/","token":"([0-9a-f]{48})"\}\n$/.exec(
      out,
    )
    if (handle === null) throw new Error(`the handle line is not as the app reads it: ${out}`)
    const log = readFileSync(join(home, 'daemon.log'), 'utf8')
    const lines = log
      .split('\n')
      .filter(Boolean)
      .map((line) =>
        line
          .replace(/^\S+ /, '$TIME ')
          .replace(
            `start pid ${child.pid} node ${process.version} home ${home}`,
            'start pid $PID node $NODE home $HOME',
          )
          .replace(/rss \d+ MB/, 'rss $MB MB'),
      )
    return {
      line: out.replace(handle[1], '$PORT').replace(handle[2], '$TOKEN'),
      log: lines,
    }
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
}

/** Every golden that is a file, by its path under the goldens' folder. */
export async function dataFiles() {
  const pages = await templates()
  return {
    'pages/agents.html': pages.agents,
    'pages/harnesses.html': pages.harnesses,
    'operations.json': `${JSON.stringify(operations(), null, 2)}\n`,
    'daemon.json': `${JSON.stringify(await daemon(), null, 2)}\n`,
  }
}
