import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { after, before, describe, it } from 'node:test'
import { AppRefused, AppUnreachable, appRequester } from '../src/requester.js'

/**
 * The one place `cf` speaks HTTP to the app (Phase 2, IMPL-PANE-20).
 *
 * A module, not a branch inside a verb, for two reasons: `bin/cf.mjs` is an
 * entry point and reads `process.env`, so nothing inside it can be unit
 * tested; and the credential rules — which field a lead must send, which
 * two a controller must send — are the same for every verb, so they are
 * worth stating once. This module reads no environment: the url, the
 * bearer and the dimensions all arrive as arguments.
 */

/** A server that records what it was asked and answers what the test says. */
async function stub() {
  const seen = []
  let answer = { status: 200, body: { ok: true } }
  const server = createServer((request, reply) => {
    let raw = ''
    request.on('data', (chunk) => {
      raw += chunk
    })
    request.on('end', () => {
      seen.push({
        method: request.method,
        path: request.url,
        authorization: request.headers.authorization ?? null,
        contentType: request.headers['content-type'] ?? null,
        body: raw.length === 0 ? null : JSON.parse(raw),
      })
      reply.writeHead(answer.status, { 'content-type': 'application/json' })
      reply.end(JSON.stringify(answer.body))
    })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    seen,
    answers: (status, body) => {
      answer = { status, body }
    },
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

describe('the requester carries the credential and its dimensions', () => {
  let app

  before(async () => {
    app = await stub()
  })
  after(async () => {
    await app.close()
  })

  it('posts a lead operation with the bearer and the token’s own tab', async () => {
    app.answers(200, { outcome: 'opened', conversation: 'zeus-quiet-fern' })
    const lead = appRequester({ url: app.url, token: 'lead-token', tab: 'tab-1' })
    const answer = await lead.post('consult', { agent: 'zeus', task: 'q', opId: 'op-1' })

    assert.deepEqual(answer, { outcome: 'opened', conversation: 'zeus-quiet-fern' })
    const call = app.seen.at(-1)
    assert.equal(call.method, 'POST')
    assert.equal(call.path, '/api/panes/consult')
    assert.equal(call.authorization, 'Bearer lead-token')
    assert.equal(call.contentType, 'application/json')
    assert.equal(call.body.tab, 'tab-1', 'ui.js refuses a lead body that names no tab')
    assert.equal(call.body.agent, 'zeus')
  })

  it('posts a controller operation with its launch and generation', async () => {
    app.answers(200, { outcome: 'bound' })
    const controller = appRequester({
      url: app.url,
      token: 'cap-token',
      launch: 'launch-9',
      generation: 3,
    })
    await controller.post('session.bind', { candidate: { sessionId: 's-1' } })

    const call = app.seen.at(-1)
    assert.equal(call.path, '/api/panes/session.bind')
    assert.equal(call.authorization, 'Bearer cap-token')
    assert.equal(call.body.launch, 'launch-9')
    assert.equal(call.body.generation, 3)
    assert.equal(
      Object.hasOwn(call.body, 'tab'),
      false,
      'a controller names no tab: its authority is the launch',
    )
  })

  it('never lets a caller name an identity the credential already fixes', async () => {
    app.answers(200, { ok: true })
    const lead = appRequester({ url: app.url, token: 'lead-token', tab: 'tab-1' })
    await assert.rejects(
      () => lead.post('say', { tab: 'tab-other', session: 's', text: 't' }),
      /tab/,
      'the credential owns the tab; a body that names one is a bug, not a request',
    )
  })

  it('redeems a ticket without a bearer, because a ticket IS the credential', async () => {
    app.answers(200, { conversation: 'zeus-quiet-fern', launch: 'l-1', capability: 'cap' })
    const redeemed = await appRequester({ url: app.url }).redeem('ticket-abc')

    assert.equal(redeemed.conversation, 'zeus-quiet-fern')
    const call = app.seen.at(-1)
    assert.equal(call.path, '/api/launch/redeem')
    assert.equal(call.authorization, null)
    assert.deepEqual(call.body, { ticket: 'ticket-abc' })
  })

  it('turns the app’s refusal into an error carrying the app’s own words', async () => {
    app.answers(409, { error: 'no-live-pane', reason: 'zeus-quiet-fern has no live pane' })
    const lead = appRequester({ url: app.url, token: 'lead-token', tab: 'tab-1' })
    const failure = await lead.post('say', { session: 'x', text: 'y' }).catch((cause) => cause)

    assert.ok(failure instanceof AppRefused)
    assert.equal(failure.status, 409)
    assert.equal(failure.code, 'no-live-pane')
    assert.match(failure.message, /has no live pane/)
  })

  it('names the app when nothing is listening there', async () => {
    const lead = appRequester({ url: 'http://127.0.0.1:1', token: 't', tab: 'tab-1' })
    const failure = await lead.post('consult', { agent: 'zeus' }).catch((cause) => cause)

    assert.ok(failure instanceof AppUnreachable)
    assert.match(failure.message, /127\.0\.0\.1:1/)
  })
})
