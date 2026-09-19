import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { Credentials, startApi } from '../src/core/api.js'
import { runCoreCli } from '../src/core/cli.js'
import { openLedger } from '../src/ledger/index.js'

/**
 * The agents' API and `cf` commands (TEST-BDC-11): each window's token names
 * one participant, the API decides who may do what, and `cf` turns it into
 * plain sentences and exit codes.
 */
async function withApi(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-core-api-'))
  const ledger = openLedger(path.join(dir, 'consensflow.db'))
  const credentials = new Credentials()
  let changes = 0
  const api = await startApi({
    ledger,
    credentials,
    changed: () => changes++,
    roster: (agent) =>
      agent === 'zeus'
        ? { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' }
        : null,
  })
  const session = ledger.createSession({
    directory: '/work/app',
    name: 'app',
    lead: { harness: 'claude-code' },
  })
  ledger.addMember(session.id, { agent: 'zeus', harness: 'claude-code', role: 'worker' })
  const participant = (handle) =>
    ledger.session(session.id).participants.find((p) => p.handle === handle)
  const token = (handle) =>
    credentials.issue({ participant: participant(handle), session, generation: 1 })
  const call = async (who, method, route, body) => {
    const response = await fetch(`${api.url}${route}`, {
      method,
      headers: { authorization: `Bearer ${who}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    return { status: response.status, body: await response.json() }
  }
  const cf = async (who, ...args) => {
    const out = []
    const err = []
    const code = await runCoreCli(
      args,
      { CONSENSFLOW_URL: api.url, CONSENSFLOW_TOKEN: who },
      {
        out: (line) => out.push(line),
        err: (line) => err.push(line),
      },
    )
    return { code, out: out.join('\n'), err: err.join('\n') }
  }
  try {
    await fn({ ledger, session, token, call, cf, credentials, changes: () => changes })
  } finally {
    await api.close()
    ledger.close()
    await rm(dir, { recursive: true, force: true })
  }
}

const deliver = (ledger, message) => {
  ledger.beginDelivery(message.id)
  ledger.confirmDelivery(message.id, { item: 'test' })
}

describe('the agents API', () => {
  it('lets a coordinator hand out a task and refuses a worker', async () => {
    await withApi(async ({ token, call, changes }) => {
      const added = await call(token('lead'), 'POST', '/api/tasks', {
        to: 'zeus',
        body: 'Write the parser',
      })
      assert.equal(added.status, 201)
      assert.deepEqual([added.body.task.number, added.body.task.assignee], [1, 'zeus'])
      assert.equal(changes(), 1, 'the dispatcher is woken')
      const refused = await call(token('zeus'), 'POST', '/api/tasks', { to: 'lead', body: 'Do it' })
      assert.deepEqual([refused.status, refused.body.error], [403, 'not-a-coordinator'])
      const stranger = await call(token('lead'), 'POST', '/api/tasks', { to: 'ghost', body: 'Boo' })
      assert.deepEqual([stranger.status, stranger.body.error], [404, 'unknown-participant'])
    })
  })

  it('refuses a missing, unknown or revoked token', async () => {
    await withApi(async ({ token, call, credentials }) => {
      assert.equal((await call('nope', 'GET', '/api/whoami')).status, 401)
      const lead = token('lead')
      assert.equal((await call(lead, 'GET', '/api/whoami')).status, 200)
      credentials.revoke(lead)
      assert.equal((await call(lead, 'GET', '/api/whoami')).status, 401)
    })
  })

  it('lets only the assignee finish a task and only a coordinator or the requester move it', async () => {
    await withApi(async ({ ledger, session, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(session.id, { from: 'human', to: 'lead', body: 'Ship v2' }).message,
      )
      const lead = token('lead')
      const zeus = token('zeus')
      assert.equal((await call(zeus, 'POST', '/api/tasks/1/done', { body: 'done' })).status, 403)
      const done = await call(lead, 'POST', '/api/tasks/1/done', { body: 'Shipped' })
      assert.deepEqual([done.status, done.body.task.state], [200, 'done'])
      assert.equal((await call(zeus, 'POST', '/api/tasks/1/accept')).status, 403)
      assert.equal((await call(lead, 'POST', '/api/tasks/1/accept')).body.task.state, 'accepted')
      assert.equal((await call(lead, 'GET', '/api/tasks/9')).status, 404)
    })
  })

  it('sends a question to whoever gave the task, and lets only the one asked answer it', async () => {
    await withApi(async ({ ledger, session, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(session.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      const zeus = token('zeus')
      const asked = await call(zeus, 'POST', '/api/questions', { body: 'Which format?' })
      assert.equal(asked.status, 201)
      assert.deepEqual([asked.body.message.recipient, asked.body.message.task], ['lead', 1])
      assert.equal(ledger.task(session.id, 1).state, 'waiting')
      const wrong = await call(zeus, 'POST', '/api/answers', {
        question: asked.body.message.id,
        body: 'JSON',
      })
      assert.deepEqual([wrong.status, wrong.body.error], [403, 'not-your-question'])
      const answered = await call(token('lead'), 'POST', '/api/answers', {
        question: asked.body.message.id,
        body: 'JSON',
      })
      assert.deepEqual([answered.status, answered.body.message.recipient], [201, 'zeus'])
    })
  })

  it('shows an agent only the messages it sent or received', async () => {
    await withApi(async ({ ledger, session, token, call }) => {
      const note = ledger.note(session.id, { from: 'lead', to: 'human', body: 'private' })
      assert.equal((await call(token('zeus'), 'GET', `/api/inbox/${note.id}`)).status, 404)
      assert.equal((await call(token('lead'), 'GET', `/api/inbox/${note.id}`)).status, 200)
      assert.deepEqual((await call(token('zeus'), 'GET', '/api/inbox')).body.messages, [])
    })
  })
})

describe('cf inside a core window', () => {
  it('hands out a task and lists the board in plain sentences', async () => {
    await withApi(async ({ token, cf }) => {
      const lead = token('lead')
      const added = await cf(lead, 'task', 'add', '@zeus', 'Write', 'the', 'parser')
      assert.deepEqual(added, {
        code: 0,
        out: 'T-1 queued for @zeus. The result arrives in your inbox when @zeus finishes.',
        err: '',
      })
      assert.equal(
        (await cf(lead, 'task', 'list')).out,
        '@zeus (worker)\nT-1 [queued] @zeus ← @lead: Write the parser',
      )
      assert.equal((await cf(lead, 'whoami')).out, '@lead (lead) in session app')
      const json = await cf(lead, 'task', 'get', 'T-1', '--json')
      assert.equal(JSON.parse(json.out).messages[0].body, 'Write the parser')
    })
  })

  it('shows the session team with each member model', async () => {
    await withApi(async ({ token, cf }) => {
      assert.equal(
        (await cf(token('lead'), 'team')).out,
        '@zeus (worker): claude-code, claude-sonnet-5, effort high',
      )
    })
  })

  it('explains a mistake and exits 2 for bad usage, 1 for a refusal', async () => {
    await withApi(async ({ token, cf }) => {
      const lead = token('lead')
      const usage = await cf(lead, 'task', 'add', 'no target')
      assert.deepEqual([usage.code, usage.err], [2, 'cf: cf task add @agent "what to do"'])
      const missing = await cf(lead, 'task', 'done', 'T-9', 'x')
      assert.deepEqual([missing.code, missing.err], [1, 'cf: no task T-9 in this session'])
      const outside = await cf('revoked', 'whoami')
      assert.equal(outside.code, 1)
      assert.match(outside.err, /no ConsensFlow access/)
    })
  })
})
