import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { GONE, PRESENT, proveAgents } from './agents-proof.mjs'
import { daemonCommand } from './helpers.mjs'

/**
 * The proof of the agents screens (tests/agents-proof.mjs) that the packaged
 * smoke runs against the daemon a built app chose: held here to what it is
 * for, which is failing when the daemon's agents are wrong, and run against each
 * daemon from the checkout (`npm run test:daemons`: Node's, then the native
 * one `CONSENSFLOW_TEST_DAEMON` names).
 */

const TOKEN = 'proof-ui-token'
const DAEMON = fileURLToPath(new URL('./integration/core-daemon.mjs', import.meta.url))

/**
 * A daemon's agents screens, as far as the proof asks, with one thing wrong
 * where `fault` says so (nothing, for a right one). Resolves to what it serves.
 */
async function agentsApi(home, fault = '') {
  const catalog = Array.from({ length: fault === 'catalog' ? 118 : 119 }, (_, at) => ({
    name: at === 0 ? 'pygmalion' : `catalog-${at}`,
    model: at === 0 && fault !== 'model' ? 'codex-image' : 'fake',
    harness: 'codex',
    profile: { workTier: 'standard' },
  }))
  let mine = []
  const file = join(home, 'agents.json')
  const write = () => writeFileSync(file, JSON.stringify({ schemaVersion: 1, agents: mine }))
  const page = (title) =>
    `<title>${title}</title>${PRESENT.filter((text) => fault !== 'page' || text !== 'Work tier').join(' ')}${fault === 'old-page' ? GONE[0] : ''}`
  write()
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://x')
    const presented =
      (request.headers.authorization ?? '').replace(/^Bearer /, '') ||
      url.searchParams.get('token') ||
      ''
    const answer = (status, body, type = 'application/json') => {
      response.writeHead(status, { 'content-type': type })
      response.end(typeof body === 'string' ? body : JSON.stringify(body))
    }
    if (presented !== TOKEN && fault !== 'open') return answer(401, { error: 'unauthorized' })
    if (url.pathname === '/') return answer(200, page('ConsensFlow — Agents'), 'text/html')
    if (url.pathname === '/harnesses') {
      return answer(200, page('ConsensFlow Harnesses'), 'text/html')
    }
    if (request.method === 'GET' && url.pathname === '/api/agents') {
      return answer(200, { agents: [...catalog, ...mine.map((row) => view(row))] })
    }
    let body = ''
    request.on('data', (chunk) => {
      body += chunk
    })
    request.on('end', () => {
      if (request.method === 'POST' && url.pathname === '/api/agents') {
        const input = JSON.parse(body)
        mine.push({
          id: input.name,
          kind: input.harness,
          model: input.model,
          ...(fault === 'effort' ? {} : { effort: input.effort }),
          ...(fault === 'profile-saved' ? { profile: { workTier: 'light' } } : {}),
        })
        write()
        return answer(201, { agent: view(mine.at(-1)) })
      }
      const named = /^\/api\/agents\/([a-z0-9-]+)$/.exec(url.pathname)
      if (request.method === 'DELETE' && named !== null) {
        if (!mine.some((row) => row.id === named[1]) && fault !== 'delete-catalog') {
          return answer(400, { error: 'a catalog agent is not yours to delete' })
        }
        if (fault !== 'delete-kept') {
          mine = mine.filter((row) => row.id !== named[1])
          write()
        }
        return answer(204, '')
      }
      return answer(404, { error: 'not found' })
    })
    return undefined
  })
  const view = (row) => ({
    name: row.id,
    harness: row.kind,
    model: row.model,
    effort: row.effort,
    custom: true,
    profile: { workTier: fault === 'tier' ? 'standard' : 'light' },
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    url: `http://127.0.0.1:${server.address().port}/`,
    close: () => {
      server.closeAllConnections()
      return new Promise((resolve) => server.close(resolve))
    },
  }
}

/** The proof, run against an agents API with `fault` in it. */
async function proving(fault) {
  const home = mkdtempSync(join(tmpdir(), 'cf-agents-fake-'))
  const api = await agentsApi(home, fault)
  try {
    await proveAgents({ url: api.url, token: TOKEN, home })
  } finally {
    await api.close()
    rmSync(home, { recursive: true, force: true })
  }
}

describe('the proof of the agents screens', () => {
  it('passes a daemon whose agents are right', async () => {
    await proving('')
  })

  it('fails a daemon whose agents are wrong, whichever way', async () => {
    for (const [fault, says] of [
      ['open', /\/ with no token: 200/],
      ['catalog', /packaged preset count/],
      ['model', /'codex-image'/],
      ['effort', /'low'/],
      ['profile-saved', /\[ 'low', 'gpt-6-astra', true \]|true/],
      ['tier', /'light'/],
      ['page', /Work tier/],
      ['old-page', /gone: id="catalog-section"/],
      ['delete-catalog', /400/],
      ['delete-kept', /./],
    ]) {
      await assert.rejects(proving(fault), says, fault)
    }
  })
})

describe('the daemon from the checkout', () => {
  it('serves the agents as the packaged smoke holds the built app to', async (t) => {
    const root = mkdtempSync(join(tmpdir(), 'cf-agents-proof-'))
    const home = join(root, 'consensflow')
    const bin = join(root, 'bin')
    mkdirSync(home, { recursive: true })
    mkdirSync(bin)
    const started = daemonCommand([DAEMON], { home })
    t.diagnostic(started.native ? 'the native daemon' : "Node's daemon")
    const env = {
      ...Object.fromEntries(
        Object.entries(process.env).filter(
          ([name]) => !name.startsWith('CONSENSFLOW_') && name.toUpperCase() !== 'PATH',
        ),
      ),
      HOME: join(root, 'home'),
      USERPROFILE: join(root, 'home'),
      CONSENSFLOW_HOME: home,
      CLAUDE_CONFIG_DIR: join(root, 'home', '.claude'),
      CODEX_HOME: join(root, 'home', '.codex'),
      XDG_CONFIG_HOME: join(root, 'home', '.config'),
      PATH: bin,
      ...started.env,
    }
    const child = spawn(started.command, started.args, {
      env,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    const exited = new Promise((resolve) => child.once('exit', resolve))
    let errors = ''
    child.stderr.on('data', (chunk) => {
      errors += chunk
    })
    try {
      // Its handle is the first line it says: where it listens, and the app's token.
      const handle = await new Promise((resolve, reject) => {
        let said = ''
        child.stdout.on('data', (chunk) => {
          said += chunk
          const end = said.indexOf('\n')
          if (end !== -1) resolve(JSON.parse(said.slice(0, end)))
        })
        child.once('exit', () =>
          reject(new Error(`the daemon ended before it said it was ready: ${errors}`)),
        )
      })
      await proveAgents({ url: handle.url, token: handle.token, home })
      assert.equal(
        JSON.parse(readFileSync(join(home, 'agents.json'), 'utf8')).agents.length,
        0,
        'the agent saved is gone from the file',
      )
    } finally {
      child.stdin.end()
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
      rmSync(root, { recursive: true, force: true })
    }
  })
})
