/**
 * A small program that reaches every surface of the daemon through the real
 * modules, for `tests/daemon-recorder.test.mjs` to run with the recorder in
 * and read what it leaves. Each test here leaves one trace; what they do is
 * what the assertions there say they did.
 */
import { spawn } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { agentsUi } from '../../../../src/core/agents-server.js'
import { Credentials, startApi } from '../../../../src/core/api.js'
import { daemonLog } from '../../../../src/core/log.js'
import { pageOperations } from '../../../../src/core/page.js'
import { eventTrace } from '../../../../src/core/trace.js'
import { openLedger } from '../../../../src/ledger/index.js'

/** What this program runs as `cf`: it asks the API who it is, as the native `cf` does, and ends with 3. */
const CF = `
const http = require('node:http')
let input = ''
process.stdin.on('data', (chunk) => { input += chunk })
process.stdin.on('end', () => {
  const url = new URL(process.env.CONSENSFLOW_URL)
  const asked = http.request(
    { hostname: url.hostname, port: url.port, path: '/api/whoami',
      headers: { authorization: 'Bearer ' + process.env.CONSENSFLOW_TOKEN, 'user-agent': 'ureq/3.4.2' } },
    (reply) => {
      reply.resume()
      reply.on('end', () => {
        process.stdout.write('said ' + input + ' ' + reply.statusCode)
        process.stderr.write('oops')
        process.exit(3)
      })
    },
  )
  asked.end()
})`

const window = (ledger, project, handle) =>
  ledger.project(project.id).participants.find((p) => p.handle === handle)

async function withApi(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-recorder-api-'))
  const ledger = openLedger(path.join(dir, 'consensflow.db'))
  const credentials = new Credentials()
  let kicks = 0
  const api = await startApi({
    ledger,
    credentials,
    changed: () => kicks++,
    roster: (agent) => (agent === 'zeus' ? { model: 'm', effort: 'high' } : null),
  })
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
  })
  try {
    await fn({
      api,
      ledger,
      project,
      credentials,
      token: (handle) =>
        credentials.issue({ participant: window(ledger, project, handle), project }),
      kicks: () => kicks,
    })
  } finally {
    await api.close()
    ledger.close()
    await rm(dir, { recursive: true, force: true })
  }
}

const call = (api, token, method, route, body) =>
  fetch(`${api.url}${route}`, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    ...(body === undefined ? {} : { body }),
  }).then(async (reply) => [reply.status, await reply.text()])

describe('a program that reaches every surface', () => {
  it('exchanges', async () => {
    await withApi(async ({ api, ledger, project, token, credentials }) => {
      const chief = token('chief')
      const zeus = token('zeus')
      await call(
        api,
        chief,
        'POST',
        '/api/tasks',
        JSON.stringify({ tier: 'standard', body: 'Parser' }),
      )
      await call(api, zeus, 'POST', '/api/tasks', 'not json at all')
      await call(api, chief, 'GET', '/api/staff')
      await call(api, 'nope', 'GET', '/api/whoami')
      credentials.revoke(zeus)
      await call(api, zeus, 'GET', '/api/whoami')
      // What a test reads of the ledger is left out; what it writes is a step.
      ledger.task(project.id, 1)
      ledger.note(project.id, { from: 'chief', to: 'human', body: 'Hello' })
      // A write that logs nothing and reads no clock is a step all the same: only a method that reads is left out.
      const other = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        chief: { harness: 'claude-code' },
      })
      ledger.setProjectState(other.id, 'suspended')
      ledger.deleteProject(other.id)
    })
  })

  it('a door that is still waiting when the API closes', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-recorder-door-'))
    const ledger = openLedger(path.join(dir, 'consensflow.db'))
    const credentials = new Credentials()
    const api = await startApi({ ledger, credentials })
    try {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code' },
      })
      ledger.addMember(project.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      const zeus = credentials.issue({ participant: window(ledger, project, 'zeus'), project })
      const asked = ledger.ask(project.id, { from: 'zeus', to: 'chief', body: 'Which?' })
      const polling = call(api, zeus, 'GET', `/api/questions/${asked.id}?wait=25000`)
      await new Promise((resolve) => setTimeout(resolve, 250))
      await api.close()
      await polling
    } finally {
      ledger.close()
      await rm(dir, { recursive: true, force: true })
    }
  })

  it('a run of cf', async () => {
    await withApi(async ({ api, token }) => {
      const env = Object.fromEntries(
        Object.entries(process.env).filter(([name]) => !/^(CONSENSFLOW_|CF_)/.test(name)),
      )
      const child = spawn(process.execPath, ['-e', CF, 'whoami'], {
        env: { ...env, CONSENSFLOW_URL: api.url, CONSENSFLOW_TOKEN: token('chief'), EXTRA: 'yes' },
        stdio: ['pipe', 'pipe', 'pipe'],
      })
      let out = ''
      child.stdout.setEncoding('utf8').on('data', (chunk) => {
        out += chunk
      })
      child.stdin.end('typed in\n')
      await new Promise((resolve) => child.on('close', resolve))
      if (out !== 'said typed in\n 200') throw new Error(out)
    })
  })

  it('page operations', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-recorder-page-'))
    const env = { HOME: home, CONSENSFLOW_HOME: path.join(home, 'consensflow') }
    mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(
      path.join(env.CONSENSFLOW_HOME, 'agents.json'),
      `${JSON.stringify({ schemaVersion: 1, agents: [{ id: 'zeus', preset: 'zeus', kind: 'claude-code', model: 'm' }] })}\n`,
    )
    const ledger = openLedger(path.join(home, 'consensflow.db'))
    let kicks = 0
    const dispatcher = {
      async openProject(request) {
        return ledger.createProject({
          directory: request.directory,
          name: request.name,
          chief: request.chief,
          staff: request.staff,
        })
      },
      async closeProject() {
        throw new Error('the window would not close')
      },
      activity: () => ({ state: 'idle' }),
      pane: () => null,
      pendingSwitch: () => null,
      holding: () => false,
      hidden: () => false,
    }
    const operations = pageOperations({ ledger, dispatcher, env, kick: () => kicks++ })
    try {
      const { project } = await operations['project.open']({
        directory: '/work/app',
        agent: 'leto',
      })
      await operations['board.get']({ project: project.id })
      await operations['project.close']({ project: project.id }).catch(() => {})
      await operations['projects.list']({})
      // A ledger the recorder did not open is a stand-in of the test's.
      const staffed = pageOperations({
        ledger: { lastStaff: () => [] },
        dispatcher,
        env,
        kick() {},
      })
      await staffed['staff.last']({})
      writeFileSync(
        path.join(env.CONSENSFLOW_HOME, 'agents.json'),
        `${JSON.stringify({ schemaVersion: 1, agents: [] })}\n`,
      )
      await operations['agents.list']({})
    } finally {
      ledger.close()
      await rm(home, { recursive: true, force: true })
    }
  })

  it('screens', async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'cf-recorder-screens-'))
    const env = {
      HOME: path.join(root, 'home'),
      CONSENSFLOW_HOME: path.join(root, 'consensflow'),
      PATH: path.join(root, 'bin'),
    }
    mkdirSync(env.PATH, { recursive: true })
    writeFileSync(path.join(env.PATH, 'claude'), '#!/bin/sh\n', { mode: 0o755 })
    const ledger = openLedger(path.join(root, 'consensflow.db'))
    const token = 'the-ui-token'
    const api = await startApi({
      ledger,
      credentials: new Credentials(),
      ui: agentsUi(env, { token }),
    })
    try {
      const ask = (target, init = {}) =>
        fetch(`${api.url}${target}`, {
          ...init,
          headers: { authorization: `Bearer ${token}`, ...init.headers },
        })
      await ask('/')
      await ask('/harnesses')
      await ask('/api/agents', {
        method: 'POST',
        body: JSON.stringify({ name: 'mine', harness: 'claude', model: 'm' }),
      })
      await ask('/api/agents/mine', { method: 'PATCH', body: JSON.stringify({ model: 'n' }) })
      await ask('/api/agents/none', { method: 'DELETE' })
      await ask('/api/whoami')
      if (!readFileSync(path.join(env.CONSENSFLOW_HOME, 'agents.json'), 'utf8').includes('"n"'))
        throw new Error('not written')
    } finally {
      await api.close()
      ledger.close()
      await rm(root, { recursive: true, force: true })
    }
  })

  it('lines', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-recorder-lines-'))
    try {
      const trace = eventTrace(dir, { limit: 150 })
      trace({ kind: 'a', project: 1, data: {} })
      trace({ at: '2026-10-05T10:00:00.000Z', kind: 'b', project: 2, data: {} })
      trace({ kind: 'c', project: 1, data: {} })
      trace({ kind: 'd', project: 2, data: {} })
      trace.forget(1)
      let second = 0
      const log = daemonLog(dir, {
        limit: 100,
        now: () => new Date(Date.UTC(2026, 9, 5, 12, 0, second++)),
      })
      log.info('start')
      log.error('failed', new Error('boom'))
      log.warn('slow')
      log.warn('slower')
      daemonLog(dir).info('no clock of its own')
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
