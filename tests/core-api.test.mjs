import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { deliver, withApi } from './core-api-fixture.mjs'

/** The agents' API (TEST-BDC-11); `cf` against it is tests/integration/cf-board.test.mjs. */
describe('the agents API', () => {
  it('lets a coordinator hand out a task and refuses a worker', async () => {
    await withApi(async ({ token, call, changes }) => {
      const added = await call(token('chief'), 'POST', '/api/tasks', {
        tier: 'standard',
        body: 'Write the parser',
      })
      assert.equal(added.status, 201)
      assert.deepEqual(
        [
          added.body.task.number,
          added.body.task.state,
          added.body.task.assignee,
          added.body.message,
        ],
        [1, 'open', null, null],
      )
      assert.equal(changes(), 1, 'the dispatcher is woken')
      const refused = await call(token('zeus'), 'POST', '/api/tasks', {
        tier: 'standard',
        body: 'Do it',
      })
      assert.deepEqual([refused.status, refused.body.error], [403, 'not-a-coordinator'])
    })
  })

  it('refuses a missing, unknown or revoked token', async () => {
    await withApi(async ({ token, call, credentials }) => {
      assert.equal((await call('nope', 'GET', '/api/whoami')).status, 401)
      const chief = token('chief')
      assert.equal((await call(chief, 'GET', '/api/whoami')).status, 200)
      credentials.revoke(chief)
      assert.equal((await call(chief, 'GET', '/api/whoami')).status, 401)
    })
  })

  it('lets only the assignee finish a task and only a coordinator or the requester move it', async () => {
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Ship v2' }).message,
      )
      const chief = token('chief')
      const zeus = token('zeus')
      assert.equal((await call(zeus, 'POST', '/api/tasks/1/done', { body: 'done' })).status, 403)
      const done = await call(chief, 'POST', '/api/tasks/1/done', { body: 'Shipped' })
      assert.deepEqual([done.status, done.body.task.state], [200, 'done'])
      assert.equal((await call(zeus, 'POST', '/api/tasks/1/accept')).status, 403)
      assert.equal((await call(chief, 'POST', '/api/tasks/1/accept')).body.task.state, 'accepted')
      assert.equal((await call(chief, 'GET', '/api/tasks/9')).status, 404)
    })
  })

  it('sends a note to whoever gave the task, or to the human from the chief; never to a member', async () => {
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const zeus = token('zeus')
      const noted = await call(zeus, 'POST', '/api/notes', { body: 'The schema moved.' })
      assert.equal(noted.status, 201)
      assert.deepEqual(
        [noted.body.message.kind, noted.body.message.recipient, noted.body.message.task],
        ['note', 'chief', 1],
      )
      assert.equal(ledger.task(project.id, 1).state, 'working', 'a note stops nothing')
      const chief = token('chief')
      const told = await call(chief, 'POST', '/api/notes', { body: 'Done for today.' })
      assert.deepEqual([told.status, told.body.message.recipient], [201, 'human'])
      assert.equal(
        ledger
          .inbox(ledger.project(project.id).participants.find((p) => p.handle === 'human').id)
          .some((m) => m.id === told.body.message.id),
        true,
      )
    })
  })

  it('sends a question to whoever gave the task, and lets only the one asked answer it', async () => {
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const zeus = token('zeus')
      const asked = await call(zeus, 'POST', '/api/questions', { body: 'Which format?' })
      assert.equal(asked.status, 201)
      assert.deepEqual([asked.body.message.recipient, asked.body.message.task], ['chief', 1])
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      const wrong = await call(zeus, 'POST', '/api/answers', {
        question: asked.body.message.id,
        body: 'JSON',
      })
      assert.deepEqual([wrong.status, wrong.body.error], [403, 'not-your-question'])
      const answered = await call(token('chief'), 'POST', '/api/answers', {
        question: asked.body.message.id,
        body: 'JSON',
      })
      assert.deepEqual([answered.status, answered.body.message.recipient], [201, 'zeus'])
    })
  })

  const COLOUR = {
    question: 'Which colour?',
    header: 'Colour',
    options: [{ label: 'red', description: 'Warm' }, { label: 'blue' }],
  }

  it('relays a question with options: the asker collects the answer the coordinator gives by label', async () => {
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const zeus = token('zeus')
      const asked = await call(zeus, 'POST', '/api/questions', { questions: [COLOUR] })
      assert.equal(asked.status, 201, JSON.stringify(asked.body))
      const id = asked.body.message.id
      assert.deepEqual(
        [asked.body.message.recipient, asked.body.message.questions[0].header],
        ['chief', 'Colour'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
      const bad = await call(zeus, 'POST', '/api/questions', { questions: [{ question: 'x' }] })
      assert.deepEqual([bad.status, bad.body.error], [400, 'bad-questions'])

      const waiting = await call(zeus, 'GET', `/api/questions/${id}`)
      assert.deepEqual(
        [waiting.status, waiting.body.answer, waiting.body.question.questions.length],
        [200, null, 1],
      )
      const started = Date.now()
      const polled = await call(zeus, 'GET', `/api/questions/${id}?wait=300`)
      assert.equal(polled.body.answer, null)
      assert.ok(Date.now() - started >= 250, 'a poll with wait holds until its time is up')
      const others = await call(token('chief'), 'GET', `/api/questions/${id}`)
      assert.deepEqual([others.status, others.body.error], [403, 'not-your-question'])

      const answered = await call(token('chief'), 'POST', '/api/answers', {
        question: id,
        body: 'Blue',
      })
      assert.deepEqual(
        [answered.status, answered.body.message.choices, answered.body.message.state],
        [201, [['blue']], 'read'],
      )
      const collected = await call(zeus, 'GET', `/api/questions/${id}?wait=5000`)
      assert.deepEqual(
        [collected.body.answer.choices, collected.body.answer.body, collected.body.answer.from],
        [[['blue']], 'Colour: blue', 'chief'],
      )
      assert.equal(ledger.task(project.id, 1).state, 'working')
      const chosen = await call(token('chief'), 'POST', '/api/answers', {
        question: id,
        choices: [['red']],
      })
      assert.deepEqual([chosen.status, chosen.body.error], [409, 'already-answered'])
    })
  })

  it('shows an agent only the messages it sent or received', async () => {
    await withApi(async ({ ledger, project, token, call }) => {
      const note = ledger.note(project.id, { from: 'chief', to: 'human', body: 'private' })
      assert.equal((await call(token('zeus'), 'GET', `/api/inbox/${note.id}`)).status, 404)
      assert.equal((await call(token('chief'), 'GET', `/api/inbox/${note.id}`)).status, 200)
      assert.deepEqual((await call(token('zeus'), 'GET', '/api/inbox')).body.messages, [])
    })
  })
})
