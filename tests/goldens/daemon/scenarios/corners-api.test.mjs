import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { Credentials, startApi } from '../../../../src/core/api.js'
import { openLedger } from '../../../../src/ledger/index.js'
import { deliver } from '../../../core-api-fixture.mjs'
import { bearer, code, send } from './raw-http.mjs'

/**
 * The agents' API where no other suite looks (TEST-BDC-11): the order of its
 * checks, what it takes for a window, how it reads a target, a body and a
 * number. The requests are sent as they are written (`http.request` keeps a
 * target as given, which `fetch` does not), and the daemon recorder takes
 * what the API answers to each for the Rust API to answer the same.
 */
async function withApi(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-core-api-corners-'))
  const ledger = openLedger(path.join(dir, 'consensflow.db'))
  const credentials = new Credentials()
  let kicks = 0
  const api = await startApi({
    ledger,
    credentials,
    changed: () => kicks++,
    roster: (agent) =>
      agent === 'zeus'
        ? { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' }
        : null,
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
  const token = (handle) =>
    credentials.issue({
      participant: ledger.project(project.id).participants.find((p) => p.handle === handle),
      project,
    })
  try {
    await fn({ api, ledger, project, token, credentials, kicks: () => kicks })
  } finally {
    await api.close()
    ledger.close()
    await rm(dir, { recursive: true, force: true })
  }
}

describe('the agents API, where no other suite looks', () => {
  it('answers a request with no window 401 before it looks for a route, and an unknown route only to a window', async () => {
    await withApi(async ({ api, token, ledger, project, kicks }) => {
      const chief = token('chief')
      const cases = [
        ['GET', '/api/nothing', undefined, 401, 'unauthorized'],
        ['GET', '/nothing', 'Bearer nope', 401, 'unauthorized'],
        ['POST', '/api/nothing', 'Bearer nope', 401, 'unauthorized'],
        ['GET', '/api/nothing', `Bearer ${chief}`, 404, 'unknown-route'],
        ['GET', '/', `Bearer ${chief}`, 404, 'unknown-route'],
        ['GET', '/api/whoami/', `Bearer ${chief}`, 404, 'unknown-route'],
        ['GET', '/API/whoami', `Bearer ${chief}`, 404, 'unknown-route'],
        ['GET', '/api/%77hoami', `Bearer ${chief}`, 404, 'unknown-route'],
        ['PUT', '/api/tasks', `Bearer ${chief}`, 404, 'unknown-route'],
        ['POST', '/api/whoami', `Bearer ${chief}`, 404, 'unknown-route'],
        ['OPTIONS', '/api/whoami', `Bearer ${chief}`, 404, 'unknown-route'],
        ['HEAD', '/api/whoami', `Bearer ${chief}`, 404, null],
        // A task is looked up before the verb: a task that is not there is that, whatever was asked of it.
        ['DELETE', '/api/tasks/1', `Bearer ${chief}`, 404, 'unknown-task'],
        ['GET', '/api/tasks/9/accept', `Bearer ${chief}`, 404, 'unknown-task'],
        ['POST', '/api/tasks/9/pause', `Bearer ${chief}`, 404, 'unknown-task'],
      ]
      for (const [method, target, authorization, status, error] of cases) {
        const answer = await send(api, {
          method,
          target,
          headers: authorization === undefined ? {} : { authorization },
        })
        assert.deepEqual(code(answer), [status, error], `${method} ${target}`)
      }
      const task = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      deliver(ledger, task.message)
      const verb = await send(api, {
        method: 'GET',
        target: '/api/tasks/1/accept',
        headers: bearer(chief),
      })
      assert.deepEqual(
        code(verb),
        [404, 'unknown-route'],
        'a task that is there has no such command',
      )
      assert.equal(verb.json.message, 'no such task command')
      assert.equal(kicks(), 0, 'nothing refused wakes the dispatcher')
    })
  })

  it('takes a window from `Authorization: Bearer <token>` and from nothing else', async () => {
    await withApi(async ({ api, token }) => {
      const chief = token('chief')
      for (const headers of [
        { authorization: `bearer ${chief}` },
        { authorization: 'Bearer' },
        { authorization: 'Bearer ' },
        { authorization: `Basic ${chief}` },
        { authorization: chief },
        { 'x-authorization': `Bearer ${chief}` },
        {},
      ]) {
        const answer = await send(api, { target: '/api/whoami', headers })
        assert.deepEqual(code(answer), [401, 'unauthorized'], JSON.stringify(headers))
        assert.equal(
          answer.json.message,
          'this window has no ConsensFlow access (it may have closed)',
        )
      }
      // The agents screens take `?token=`; the agents' API does not.
      const query = await send(api, { target: `/api/whoami?token=${chief}` })
      assert.equal(query.status, 401)
      assert.equal((await send(api, { target: '/api/whoami', headers: bearer(chief) })).status, 200)
    })
  })

  it('reads a target as a WHATWG URL does: dot segments, a second slash, a backslash, a fragment', async () => {
    await withApi(async ({ api, token }) => {
      const chief = token('chief')
      for (const target of [
        '/api/tasks/../whoami',
        '/api/./whoami',
        '/api/tasks/%2e%2e/whoami',
        '/api/tasks/%2E./whoami',
        '//evil.example/api/whoami',
        '/api\\whoami',
        '/api/whoami#fragment',
        '/api/whoami?x=1&x=2',
        '/api/whoami?',
      ]) {
        const answer = await send(api, { target, headers: bearer(chief) })
        assert.deepEqual([answer.status, answer.json?.participant?.handle], [200, 'chief'], target)
      }
    })
  })

  it('reads a body only on the routes that take one, as a JSON object, and refuses a coordinator’s route before it reads', async () => {
    await withApi(async ({ api, token, kicks }) => {
      const chief = bearer(token('chief'))
      const zeus = bearer(token('zeus'))
      const post = (target, headers, body) => send(api, { method: 'POST', target, headers, body })
      const invalid = [Buffer.from('{'), '[]', 'null', '"x"', '1', 'true', '﻿{}', ' ', '{"a":1}x']
      for (const body of invalid) {
        const answer = await post('/api/tasks', chief, body)
        assert.deepEqual(code(answer), [400, 'invalid-json'], JSON.stringify(String(body)))
        assert.equal(answer.json.message, 'the request body must be a JSON object')
      }
      // An empty body is `{}`: the ledger says what is missing.
      assert.deepEqual(code(await post('/api/tasks', chief)), [400, 'invalid-text'])
      assert.deepEqual(code(await post('/api/notes', chief, '{')), [400, 'invalid-json'])
      // A coordinator's route refuses a member, and the chief's question the chief, before any body is read.
      assert.deepEqual(code(await post('/api/tasks', zeus, '{')), [403, 'not-a-coordinator'])
      assert.deepEqual(code(await post('/api/questions', chief, '{')), [
        403,
        'ask-in-your-terminal',
      ])
      // A route that takes no body leaves one unread.
      assert.deepEqual(code(await post('/api/whoami', chief, '{')), [404, 'unknown-route'])
      // The type of the body is not asked: text is read as JSON.
      const plain = await post(
        '/api/tasks',
        { ...chief, 'content-type': 'text/plain' },
        JSON.stringify({ tier: 'standard', body: 'Plain' }),
      )
      assert.deepEqual([plain.status, plain.json.task.title], [201, 'Plain'])
      assert.equal(kicks(), 1, 'only the task made wakes the dispatcher')
    })
  })

  it('reads a body as UTF-8, bad bytes as U+FFFD, and refuses one over 2 MiB', async () => {
    await withApi(async ({ api, token, ledger, project }) => {
      const chief = bearer(token('chief'))
      const zeus = bearer(token('zeus'))
      const bad = Buffer.concat([
        Buffer.from('{"body":"a'),
        Buffer.from([0xff, 0xc3]),
        Buffer.from('b"}'),
      ])
      const noted = await send(api, {
        method: 'POST',
        target: '/api/notes',
        headers: chief,
        body: bad,
      })
      assert.equal(noted.status, 201)
      assert.equal(noted.json.message.preview, 'a��b')
      const big = JSON.stringify({ tier: 'standard', body: 'x'.repeat(2 * 1024 * 1024) })
      assert.ok(Buffer.byteLength(big) > 2 * 1024 * 1024)
      const refused = await send(api, {
        method: 'POST',
        target: '/api/tasks',
        headers: chief,
        body: big,
      })
      assert.deepEqual(code(refused), [413, 'too-large'])
      assert.equal(refused.json.message, 'the request is larger than 2 MB')
      // A member is refused before the size is looked at.
      const member = await send(api, {
        method: 'POST',
        target: '/api/tasks',
        headers: zeus,
        body: big,
      })
      assert.deepEqual(code(member), [403, 'not-a-coordinator'])
      assert.equal(ledger.task(project.id, 1), null, 'nothing was made of it')
    })
  })

  it('reads the numbers in a target as JavaScript does, and quotes a message’s as they were typed', async () => {
    await withApi(async ({ api, token }) => {
      const chief = bearer(token('chief'))
      const ask = async (target) => (await send(api, { target, headers: chief })).json.message
      assert.equal(await ask('/api/tasks/007'), 'no task T-7 in this project')
      assert.equal(
        await ask('/api/tasks/99999999999999999999'),
        'no task T-100000000000000000000 in this project',
      )
      assert.equal(
        await ask('/api/tasks/1000000000000000000000'),
        'no task T-1e+21 in this project',
      )
      assert.equal(await ask('/api/inbox/00012'), 'no message m-00012 for you')
      assert.equal(
        await ask('/api/inbox/99999999999999999999999'),
        'no message m-99999999999999999999999 for you',
      )
      assert.equal(await ask('/api/questions/00012'), 'no question m-00012')
      // A task number with leading zeros is the same task.
      const made = await send(api, {
        method: 'POST',
        target: '/api/tasks',
        headers: chief,
        body: JSON.stringify({ tier: 'standard', body: 'One' }),
      })
      assert.equal(made.status, 201)
      for (const target of ['/api/tasks/1', '/api/tasks/0001', '/api/tasks/00000000000000000001']) {
        assert.equal((await send(api, { target, headers: chief })).json.task.number, 1, target)
      }
    })
  })

  it('answers 401 to a window whose project has been deleted, with its own words', async () => {
    await withApi(async ({ api, token, ledger, project }) => {
      const zeus = bearer(token('zeus'))
      assert.equal((await send(api, { target: '/api/whoami', headers: zeus })).status, 200)
      ledger.setProjectState(project.id, 'suspended')
      ledger.deleteProject(project.id)
      const gone = await send(api, { target: '/api/whoami', headers: zeus })
      assert.deepEqual(code(gone), [401, 'unauthorized'])
      assert.equal(gone.json.message, 'this window belongs to a project that no longer exists')
    })
  })

  it('gives a finished task back to its assignee with the chief’s words, and refuses one that is not finished', async () => {
    await withApi(async ({ api, token, ledger, project, kicks }) => {
      const chief = bearer(token('chief'))
      const zeus = bearer(token('zeus'))
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const body = (text) => JSON.stringify({ body: text })
      const early = await send(api, {
        method: 'POST',
        target: '/api/tasks/1/reopen',
        headers: chief,
        body: body('Again'),
      })
      assert.deepEqual(code(early), [409, 'invalid-transition'])
      const done = await send(api, {
        method: 'POST',
        target: '/api/tasks/1/done',
        headers: zeus,
        body: body('Done'),
      })
      assert.equal(done.json.task.state, 'done')
      const sideways = await send(api, {
        method: 'POST',
        target: '/api/tasks/1/reopen',
        headers: zeus,
        body: body('Again'),
      })
      assert.deepEqual(code(sideways), [403, 'not-a-coordinator'])
      const before = kicks()
      const again = await send(api, {
        method: 'POST',
        target: '/api/tasks/1/reopen',
        headers: chief,
        body: body('Again'),
      })
      assert.equal(again.status, 200)
      assert.equal(kicks(), before + 1)
    })
  })

  it('shows a transcript between 1 and 50 items, 10 when it is asked for no number', async () => {
    await withApi(async ({ api, token, ledger, project }) => {
      const chief = bearer(token('chief'))
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
      ledger.copyTranscript(
        conversation.id,
        Array.from({ length: 60 }, (_, n) => ({
          id: `i${n + 1}`,
          role: 'assistant',
          text: `item ${n + 1}`,
          complete: true,
        })),
      )
      const shown = async (last) =>
        (await send(api, { target: `/api/tasks/1/transcript${last}`, headers: chief })).json.items
          .length
      assert.deepEqual(
        [
          await shown(''),
          await shown('?last=0'),
          await shown('?last=abc'),
          await shown('?last=-5'),
        ],
        [10, 10, 10, 1],
      )
      assert.deepEqual(
        [
          await shown('?last=99'),
          await shown('?last=3'),
          await shown('?last=1e1'),
          await shown('?last='),
        ],
        [50, 3, 10, 10],
      )
    })
  })

  it('waits for an answer no longer than it is asked, reading the wait as a number', async () => {
    await withApi(async ({ api, token, ledger, project }) => {
      const zeus = bearer(token('zeus'))
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const asked = ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Which?' })
      for (const wait of [
        '',
        '?wait=abc',
        '?wait=-5',
        '?wait=0',
        '?wait=0x10',
        '?wait=1e1',
        '?wait=%20',
        '?wait=20',
      ]) {
        const answer = await send(api, {
          target: `/api/questions/${asked.id}${wait}`,
          headers: zeus,
        })
        assert.deepEqual([answer.status, answer.json.answer], [200, null], wait)
      }
    })
  })
})
