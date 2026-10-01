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
const participantId = (ledger, project, handle) =>
  ledger.project(project.id).participants.find((p) => p.handle === handle).id

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
  const participant = (handle) =>
    ledger.project(project.id).participants.find((p) => p.handle === handle)
  const token = (handle) => credentials.issue({ participant: participant(handle), project })
  const call = async (who, method, route, body) => {
    const response = await fetch(`${api.url}${route}`, {
      method,
      headers: { authorization: `Bearer ${who}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    return { status: response.status, body: await response.json() }
  }
  const cf = async (who, ...args) => {
    const options = typeof args.at(-1) === 'object' ? args.pop() : {}
    const out = []
    const err = []
    const code = await runCoreCli(
      args,
      { CONSENSFLOW_URL: api.url, CONSENSFLOW_TOKEN: who },
      {
        out: (line) => out.push(line),
        err: (line) => err.push(line),
        ...(options.input === undefined ? {} : { input: async () => options.input }),
      },
    )
    return { code, out: out.join('\n'), err: err.join('\n') }
  }
  try {
    await fn({ ledger, project, token, call, cf, credentials, changes: () => changes })
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

  it("shows the chief what a task's window did so far, the last items when asked for fewer", async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      const chief = token('chief')
      const empty = await cf(chief, 'task', 'get', 'T-1', '--transcript')
      assert.match(
        empty.out,
        /^T-1 \[working\] @zeus ← @chief: Parser\n\nIts window has written nothing yet\.$/,
      )
      const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
      ledger.copyTranscript(conversation.id, [
        {
          id: 'u1',
          role: 'user',
          text: `[ConsensFlow m-${message.id} · T-1 · task from @chief]\nParser`,
          complete: true,
          at: null,
        },
        {
          id: 'a1',
          role: 'assistant',
          text: 'Reading the grammar first.',
          complete: true,
          at: null,
        },
        { id: 't1', role: 'tool', text: 'ok\n14 passed', complete: true, at: null },
        { id: 'a2', role: 'assistant', text: 'Parser done', complete: false, at: null },
      ])
      const last = await cf(chief, 'task', 'get', 'T-1', '--transcript', '--last', '2')
      assert.equal(
        last.out,
        'T-1 [working] @zeus ← @chief: Parser\n\nWhat its window did, the last 2 of 4 items:\n\n[Tool output]\nok\n14 passed\n\n[The agent · still writing]\nParser done',
      )
      const asJson = JSON.parse(
        (await cf(chief, 'task', 'get', 'T-1', '--transcript', '--json')).out,
      )
      assert.deepEqual([asJson.total, asJson.items.map((i) => i.id)], [4, ['u1', 'a1', 't1', 'a2']])
      const bad = await cf(chief, 'task', 'get', 'T-1', '--transcript', '--last', 'many')
      assert.deepEqual([bad.code, bad.err], [2, 'cf: --last takes a number of items'])
      const member = await cf(token('zeus'), 'task', 'get', 'T-1', '--transcript')
      assert.deepEqual(
        [member.code, member.err],
        [1, "cf: only the chief or @chief may read what T-1's window did"],
      )
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

  it("notes the human from the chief, also on its own task, and refuses a member's --human", async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const notes = (handle) =>
        ledger
          .inbox(participantId(ledger, project, handle))
          .filter((m) => m.kind === 'note')
          .map((m) => [m.sender, m.taskNumber, m.body])
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Build',
      })
      ledger.createTask(project.id, { from: 'chief', to: 'chief', needs: [1], body: 'Check it' })
      deliver(
        ledger,
        ledger.assignTask(project.id, 1, participantId(ledger, project, 'zeus')).message,
      )
      const worker = token(ledger.task(project.id, 1).assignee)
      const refused = await cf(worker, 'note', '--human', 'Built')
      assert.deepEqual(
        [refused.code, refused.err],
        [1, 'cf: only the chief notes the human; without --human, your note goes to @chief'],
      )
      ledger.recordResult(project.id, 1, { body: 'Built.' })
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      deliver(
        ledger,
        ledger.task(project.id, 2).messages.find((m) => m.kind === 'task'),
      )
      assert.equal(ledger.task(project.id, 2).state, 'working', 'the chief is on its own step')
      const chief = token('chief')
      const noted = await cf(chief, 'note', '--human', 'T-1 shipped; the build is green')
      assert.equal(noted.code, 0, noted.err)
      assert.match(noted.out, /^m-\d+ noted to @human; nothing waits on it\.$/)
      assert.equal((await cf(chief, 'note', 'And the docs are next')).code, 0)
      assert.deepEqual(notes('human'), [
        ['chief', 2, 'And the docs are next'],
        ['chief', 2, 'T-1 shipped; the build is green'],
      ])
      assert.deepEqual(notes('chief'), [], 'the chief never notes itself')
    })
  })

  it('takes a text from standard input when it is -, which no shell expands', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      const brief = 'Ask me with `cf ask --human "x"` first; keep $(date) and "quotes" as they are.'
      const added = await cf(chief, 'task', 'add', '--tier', 'standard', '-', {
        input: `${brief}\n`,
      })
      assert.equal(added.code, 0, added.err)
      assert.equal(ledger.task(project.id, 1).body, brief, 'verbatim, the final newline dropped')
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      ledger.assignTask(project.id, 1, zeus.id)
      deliver(
        ledger,
        ledger.task(project.id, 1).messages.find((m) => m.kind === 'task'),
      )
      const worker = ledger.task(project.id, 1).assignee
      const asked = await cf(token(worker), 'ask', '-', { input: 'Which `file`?' })
      assert.equal(asked.code, 0, asked.err)
      const question = ledger.task(project.id, 1).messages.find((m) => m.kind === 'question')
      assert.equal(question.body, 'Which `file`?')
      const answered = await cf(chief, 'answer', `m-${question.id}`, '-', { input: 'Use `a.txt`.' })
      assert.equal(answered.code, 0, answered.err)
      const noted = await cf(chief, 'note', '--human', '-', { input: 'See `a.txt`.' })
      assert.equal(noted.code, 0, noted.err)
      assert.match(noted.out, /^m-\d+ noted to @human;/)
      const told = await cf(chief, 'tell', 'T-1', '-', { input: 'Stop: `v2` now.' })
      assert.equal(told.code, 0, told.err)
      const resumed = await cf(chief, 'task', 'resume', 'T-1', '-', { input: 'Go on with `v2`.' })
      assert.equal(resumed.code, 0, resumed.err)
      const bodies = ledger.task(project.id, 1).messages.map((m) => m.body)
      for (const text of ['Use `a.txt`.', 'Stop: `v2` now.']) {
        assert.ok(
          bodies.some((body) => body.includes(text)),
          text,
        )
      }
      const empty = await cf(chief, 'note', '-', { input: '  \n' })
      assert.equal(empty.code, 2, 'nothing on standard input is no text')
    })
  })

  it("tells a task's window something urgent: the task is paused for it and the question queued", async () => {
    await withApi(async ({ ledger, project, token, cf, call }) => {
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      const chief = token('chief')
      // A tell the board refuses stops nothing.
      const refused = await call(chief, 'POST', '/api/tasks/1/tell', {
        body: 'y'.repeat(1_000_001),
      })
      assert.deepEqual([refused.status, refused.body.error], [400, 'invalid-text'])
      assert.equal(ledger.task(project.id, 1).state, 'working')
      const told = await cf(chief, 'tell', 'T-1', 'Stop: the grammar changed, use v2')
      assert.equal(told.code, 0, told.err)
      assert.match(
        told.out,
        /^T-1 is paused and m-\d+ put to @zeus; its answer arrives as a message\. Then: cf task resume T-1 "…"$/,
      )
      assert.equal(ledger.task(project.id, 1).state, 'paused')
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const queued = ledger.inbox(zeus.id).find((m) => m.kind === 'question')
      assert.deepEqual(
        [queued.sender, queued.urgent, queued.taskNumber, queued.body],
        ['chief', true, 1, 'Stop: the grammar changed, use v2'],
      )
      // Again while paused: no second pause, one more question.
      assert.equal((await cf(chief, 'tell', 'T-1', 'And keep the tests')).code, 0)
      assert.equal(ledger.task(project.id, 1).state, 'paused')
      // Only the chief (or whoever gave the task) tells; a task with no window cannot be told.
      const sideways = await cf(token('zeus'), 'tell', 'T-1', 'Hey')
      assert.deepEqual(
        [sideways.code, sideways.err],
        [1, 'cf: only the chief or @chief may tell T-1'],
      )
      await call(chief, 'POST', '/api/tasks', { tier: 'standard', body: 'Docs' })
      const open = await cf(chief, 'tell', 'T-2', 'Hurry')
      assert.deepEqual([open.code, open.err], [1, 'cf: T-2 has no window to tell: it is open'])
      assert.deepEqual((await cf(chief, 'tell', 'T-1')).code, 2, 'the words are required')
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

  it("answers only a question of the caller's own project: message ids run across projects", async () => {
    await withApi(async ({ ledger, token, call, cf, credentials }) => {
      // Another project, gated, whose chief and member have the same handles as this one's.
      const other = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'codex' },
        gate: true,
      })
      ledger.addMember(other.id, {
        agent: 'zeus',
        harness: 'claude-code',
        role: 'worker',
        tier: 'standard',
      })
      const { message } = ledger.createTask(other.id, { from: 'chief', to: 'zeus', body: 'Site' })
      deliver(ledger, ledger.approveMessage(message.id, { by: 'human' }))
      const question = ledger.ask(other.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Which database?',
      })
      // This chief means its own m-12 and types the other project's number.
      const crossed = await cf(token('chief'), 'answer', `m-${question.id}`, 'Our customer one')
      assert.deepEqual(
        [crossed.code, crossed.err],
        [1, `cf: no question m-${question.id} in this project`],
      )
      assert.equal(ledger.message(question.id).state, 'gated', 'it still waits for its human')
      assert.deepEqual(
        ledger.task(other.id, 1).messages.filter((m) => m.kind === 'answer'),
        [],
        'nothing reached the other project',
      )
      for (const id of [undefined, 'abc', { id: 1 }, 0, -3, 1.5]) {
        const refused = await call(token('chief'), 'POST', '/api/answers', {
          question: id,
          body: 'x',
        })
        assert.deepEqual(
          [refused.status, refused.body.error],
          [404, 'unknown-message'],
          JSON.stringify(id),
        )
      }
      // Its own chief answers it as before.
      ledger.approveMessage(question.id, { by: 'human' })
      const theirs = ledger.project(other.id).participants.find((p) => p.handle === 'chief')
      const answered = await cf(
        credentials.issue({ participant: theirs, project: other }),
        'answer',
        `m-${question.id}`,
        'Postgres',
      )
      assert.equal(answered.code, 0, answered.err)
      const answer = ledger.task(other.id, 1).messages.find((m) => m.kind === 'answer')
      assert.deepEqual(
        [answer.sender, answer.recipient, answer.body],
        ['chief', 'zeus', 'Postgres'],
      )
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

  it("refuses the chief's question: the chief asks the human in its own terminal", async () => {
    await withApi(async ({ ledger, project, token, call, cf }) => {
      const chief = await call(token('chief'), 'POST', '/api/questions', { body: 'Ship?' })
      assert.deepEqual([chief.status, chief.body.error], [403, 'ask-in-your-terminal'])
      const asked = await cf(token('chief'), 'ask', 'Ship?')
      assert.deepEqual(
        [asked.code, asked.err],
        [1, 'cf: ask the human here in your terminal: they read and answer you there'],
      )
      assert.deepEqual(
        ledger.inbox(ledger.project(project.id).participants.find((p) => p.role === 'human').id),
        [],
      )
    })
  })

  it("lets an advisor and a reviewer ask the chief with cf ask, and routes the chief's answer back to them", async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      for (const [agent, role] of [
        ['athena', 'advisor'],
        ['calliope', 'reviewer'],
      ])
        ledger.addMember(project.id, { agent, harness: 'claude-code', role, tier: 'standard' })
      const member = (handle) =>
        ledger.project(project.id).participants.find((p) => p.handle === handle)
      for (const [number, flag, agent] of [
        [1, '--advice', 'athena'],
        [2, '--review', 'calliope'],
      ]) {
        const added = await cf(
          chief,
          'task',
          'add',
          flag,
          '--tier',
          'standard',
          `Task for ${agent}`,
        )
        assert.equal(added.code, 0, added.err)
        deliver(ledger, ledger.assignTask(project.id, number, member(agent).id).message)
        const session = ledger.task(project.id, number).assignee
        const asked = await cf(token(session), 'ask', 'Managers or HR first?')
        assert.equal(asked.code, 0, asked.err)
        const task = ledger.task(project.id, number)
        const question = task.messages.find((m) => m.kind === 'question')
        assert.deepEqual(
          [question.sender, question.recipient, task.state],
          [session, 'chief', 'waiting'],
          `${flag}: to the chief, the task waiting`,
        )
        const answered = await cf(chief, 'answer', `m-${question.id}`, 'Managers first.')
        assert.equal(answered.code, 0, answered.err)
        const answer = ledger.task(project.id, number).messages.find((m) => m.kind === 'answer')
        assert.deepEqual([answer.recipient, answer.body], [session, 'Managers first.'])
      }
    })
  })

  it('gives the chief every character of a long result with cf inbox read', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      assert.equal(
        (await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the report')).code,
        0,
      )
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      deliver(ledger, ledger.assignTask(project.id, 1, zeus.id).message)
      // The window shows the first 3000 characters; the rest is read with this.
      const body = `${'Line of the report.\n'.repeat(1000)}The code at the end: CEDRU-7314`
      ledger.recordResult(project.id, 1, { body })
      const result = ledger.task(project.id, 1).messages.find((m) => m.kind === 'result')
      const read = await cf(chief, 'inbox', 'read', `m-${result.id}`)
      assert.equal(read.code, 0, read.err)
      assert.ok(read.out.includes(body), 'the whole body, unshortened')
    })
  })

  it('shows an agent nothing that still waits for the human', async () => {
    await withApi(async ({ ledger, project, token, call, cf }) => {
      ledger.setGate(project.id, true)
      const chief = token('chief')
      const opened = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the parser')
      assert.equal(opened.code, 0, opened.err)
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      assert.equal(message.state, 'gated')
      const session = token(message.recipient)
      assert.deepEqual((await call(session, 'GET', '/api/inbox')).body.messages, [])
      assert.equal((await call(session, 'GET', `/api/inbox/${message.id}`)).status, 404)
      assert.equal(
        (await call(chief, 'GET', `/api/inbox/${message.id}`)).status,
        200,
        'the sender may read its own',
      )
      assert.deepEqual((await call(chief, 'GET', '/api/tasks/1')).body.task.messages, [])
      ledger.approveMessage(message.id, { by: 'human' })
      assert.deepEqual(
        (await call(session, 'GET', '/api/inbox')).body.messages.map((m) => m.state),
        ['queued'],
      )
      assert.equal((await call(session, 'GET', `/api/inbox/${message.id}`)).status, 200)
      assert.equal((await call(chief, 'GET', '/api/tasks/1')).body.task.messages.length, 1)
    })
  })

  it('tells the chief when what it sent waits for the human', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      ledger.setGate(project.id, true)
      const chief = token('chief')
      const opened = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the parser')
      assert.equal(
        opened.out,
        'T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox. The human approves each message before it moves.',
      )
      const own = await cf(chief, 'task', 'add', '--self', '--needs', 'T-1', 'Plan the release')
      assert.equal(
        own.out,
        'T-2 is yours; finish it with: cf task done T-2 "what you did". It waits until T-1 is accepted.',
      )
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      deliver(ledger, ledger.approveMessage(message.id, { by: 'human' }))
      const question = ledger.ask(project.id, {
        from: message.recipient,
        to: 'chief',
        task: 1,
        body: 'Which format?',
      })
      deliver(ledger, ledger.approveMessage(question.id, { by: 'human' }))
      const answered = await cf(chief, 'answer', `m-${question.id}`, 'JSON')
      assert.equal(
        answered.out,
        `m-${question.id + 1} answered @${message.recipient}; the human passes it on first.`,
      )
      ledger.setGate(project.id, false)
      const again = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the lexer')
      assert.equal(
        again.out,
        'T-3 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.',
      )
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

describe('cf history', () => {
  it('lets the lead read what the human and the leads before it said, in pages and by search; nobody else', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      const first = ledger.startConversation(chief.id, { harness: 'claude-code' })
      ledger.copyTranscript(first.id, [
        { id: 'a', role: 'user', text: 'The codeword is tern' },
        { id: 'b', role: 'assistant', text: 'Noted' },
        { id: 'c', role: 'tool', text: 'ok 3 passed' },
      ])
      ledger.switchChief(project.id, { harness: 'codex' })
      const lead = token('chief')
      const read = await cf(lead, 'history')
      assert.equal(read.code, 0, read.err)
      assert.match(read.out, /^Lead history, page 1 of 1: the most recent\./)
      assert.match(read.out, /Human: The codeword is tern\n\nClaude Code lead: Noted/)
      assert.ok(!read.out.includes('ok 3 passed'))
      assert.match((await cf(lead, 'history', '--tools')).out, /Tool output:\nok 3 passed/)
      assert.match((await cf(lead, 'history', '--find', 'codeword')).out, /entries with "codeword"/)
      const beyond = await cf(lead, 'history', '--page', '3')
      assert.notEqual(beyond.code, 0)
      assert.match(beyond.err, /there is 1 page/)
      const member = await cf(token('zeus'), 'history')
      assert.notEqual(member.code, 0)
      assert.match(member.err, /the lead history is the lead's to read/)
    })
  })

  it("reads out only this project's messages, whatever number a line of the history names", async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const other = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'codex' },
      })
      const theirs = ledger.note(other.id, { from: 'chief', to: 'human', body: 'Launch code 4417' })
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      const first = ledger.startConversation(chief.id, { harness: 'claude-code' })
      ledger.copyTranscript(first.id, [
        { id: 'a', role: 'user', text: `[ConsensFlow m-${theirs.id} · pasted from elsewhere]` },
      ])
      ledger.switchChief(project.id, { harness: 'codex' })
      const read = await cf(token('chief'), 'history')
      assert.equal(read.code, 0, read.err)
      assert.ok(!read.out.includes('4417'), read.out)
      assert.ok(read.out.includes(`m-${theirs.id}: a message ConsensFlow delivered`), read.out)
    })
  })
})

describe('cf inside a core window', () => {
  it('hands out a task and lists the board in plain sentences', async () => {
    await withApi(async ({ token, cf }) => {
      const chief = token('chief')
      const added = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write', 'the', 'parser')
      assert.deepEqual(added, {
        code: 0,
        out: 'T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.',
        err: '',
      })
      assert.equal(
        (await cf(chief, 'task', 'list')).out,
        'Waiting for a member\nT-1 [open] for a standard worker ← @chief: Write the parser',
      )
      assert.equal((await cf(chief, 'whoami')).out, '@chief (chief) in project app')
      const json = await cf(chief, 'task', 'get', 'T-1', '--json')
      assert.deepEqual(
        [JSON.parse(json.out).body, JSON.parse(json.out).messages],
        ['Write the parser', []],
      )
    })
  })

  it('orders the board with --needs and --before, and says what waits for what', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      assert.equal((await cf(chief, 'task', 'add', '--tier', 'standard', 'Lexer')).code, 0)
      const parser = await cf(
        chief,
        'task',
        'add',
        '--tier',
        'standard',
        '--needs',
        'T-1',
        'Parser',
      )
      assert.equal(
        parser.out,
        'T-2 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox. It waits until T-1 is accepted.',
      )
      const fix = await cf(chief, 'task', 'add', '--tier', 'standard', '--before', 'T-1,T-2', 'Fix')
      assert.equal(
        fix.out,
        'T-3 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox. T-1, T-2 wait for it.',
      )
      assert.equal(
        (await cf(chief, 'task', 'list')).out,
        [
          'Waiting for a member',
          'T-1 [open] blocked by T-3 · for a standard worker ← @chief: Lexer',
          'T-2 [open] blocked by T-1, T-3 · for a standard worker ← @chief: Parser',
          'T-3 [open] for a standard worker ← @chief: Fix',
        ].join('\n'),
      )
      assert.deepEqual(JSON.parse((await cf(chief, 'task', 'get', 'T-2', '--json')).out).needs, [
        { number: 1, state: 'open' },
        { number: 3, state: 'open' },
      ])
      const both = await cf(
        chief,
        'task',
        'add',
        '--tier',
        'standard',
        '--needs',
        'T-1',
        '--before',
        'T-2',
        'Between',
      )
      assert.equal(
        both.out,
        'T-4 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox. It waits until T-1 is accepted. T-2 waits for it.',
      )
      const own = await cf(chief, 'task', 'add', '--self', '--needs', 'T-1', 'Plan')
      assert.equal(own.code, 0)
      assert.match(own.out, /^T-5 is yours; finish it with: cf task done T-5 "what you did"\./)
      const bad = await cf(chief, 'task', 'add', '--tier', 'standard', '--needs', 'one', 'Lexer')
      assert.deepEqual([bad.code, bad.err], [2, 'cf: not a task: "one" (write T-3)'])
      const loop = await cf(
        chief,
        'task',
        'add',
        '--tier',
        'standard',
        '--needs',
        'T-2',
        '--before',
        'T-1',
        'Loop',
      )
      assert.deepEqual(
        [loop.code, loop.err],
        [1, 'cf: T-1 is already what T-6 waits for: a plan has no circles'],
      )
      ledger.assignTask(project.id, 3, participantId(ledger, project, 'zeus'))
      const late = await cf(chief, 'task', 'add', '--tier', 'standard', '--before', 'T-3', 'Late')
      assert.deepEqual(
        [late.code, late.err],
        [1, 'cf: T-3 is queued: only a task still on the board can wait for a new one'],
      )
    })
  })

  it('lets the chief pause and resume a task, in plain words, and nobody else', async () => {
    await withApi(async ({ ledger, project, token, cf, call }) => {
      const chief = token('chief')
      assert.equal((await cf(chief, 'task', 'add', '--tier', 'standard', 'Parser')).code, 0)
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      deliver(ledger, message)
      const session = message.recipient
      assert.equal((await call(token(session), 'POST', '/api/tasks/1/pause')).status, 403)
      const paused = await cf(chief, 'task', 'pause', 'T-1')
      assert.deepEqual(
        [paused.code, paused.out],
        [
          0,
          'T-1 is paused: its window stops and its work waits. Resume it with: cf task resume T-1 "what to do now"',
        ],
      )
      assert.equal(ledger.task(project.id, 1).state, 'paused')
      const silent = await cf(chief, 'task', 'resume', 'T-1')
      assert.deepEqual([silent.code, silent.err], [2, 'cf: cf task resume T-1 "what to do now"'])
      const resumed = await cf(chief, 'task', 'resume', 'T-1', 'Go', 'on')
      assert.deepEqual(
        [resumed.code, resumed.out],
        [0, `T-1 resumes in @${session} with your words.`],
      )
      assert.equal(ledger.task(project.id, 1).state, 'queued')
    })
  })

  it('shows the project staff as roles and tiers, nothing to pick a member by', async () => {
    await withApi(async ({ token, cf, ledger, project }) => {
      assert.equal((await cf(token('chief'), 'staff')).out, '@zeus · worker · standard')
      ledger.setRoles(project.id, 'zeus', ['worker', 'reviewer'])
      assert.equal((await cf(token('chief'), 'staff')).out, '@zeus · worker+reviewer · standard')
    })
  })

  it('explains a mistake and exits 2 for bad usage, 1 for a refusal', async () => {
    await withApi(async ({ token, cf }) => {
      const chief = token('chief')
      const usage = await cf(chief, 'task', 'add', 'no target')
      assert.deepEqual(
        [usage.code, usage.err],
        [
          2,
          'cf: cf task add --tier <critical|complex|standard|light> "what to do" (with --advice for an advisor or --review for a reviewer; or --design, --after T-3, or --self; --needs T-3,T-4 and --before T-9,T-10 order the board)',
        ],
      )
      const missing = await cf(chief, 'task', 'done', 'T-9', 'x')
      assert.deepEqual([missing.code, missing.err], [1, 'cf: no task T-9 in this project'])
      const outside = await cf('revoked', 'whoami')
      assert.equal(outside.code, 1)
      assert.match(outside.err, /no ConsensFlow access/)
    })
  })
})

describe('tiered tasks through the API and cf', () => {
  it('puts work for a tier nobody holds on the nearest tier held, and says so', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const moved = await cf(
        token('chief'),
        'task',
        'add',
        '--tier',
        'critical',
        '--purpose',
        'hard-problem',
        'Why is it slow?',
      )
      assert.deepEqual(moved, {
        code: 0,
        out: 'T-1 is on the board for a standard worker (no critical worker is on the staff, so the nearest tier); the first free one gets it, and its result arrives in your inbox.',
        err: '',
      })
      assert.equal(ledger.task(project.id, 1).tier, 'standard')
    })
  })

  it('opens a task for a tier, never for a member by name', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      const opened = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the parser')
      assert.deepEqual(opened, {
        code: 0,
        out: 'T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.',
        err: '',
      })
      const task = ledger.task(project.id, 1)
      assert.deepEqual([task.state, task.pool, task.tier], ['open', 'worker', 'standard'])

      // No task is given by name: `@zeus` is just words with no tier, and the usage says so.
      const named = await cf(chief, 'task', 'add', '@zeus', 'Write the lexer')
      assert.equal(named.code, 2)
      assert.match(named.err, /^cf: cf task add --tier <critical\|complex\|standard\|light>/)
      assert.equal(ledger.task(project.id, 2), null, 'and nothing was created')
      const noPurpose = await cf(chief, 'task', 'add', '--tier', 'critical', 'Why is it slow?')
      assert.equal(noPurpose.code, 1)
      assert.match(noPurpose.err, /critical work names its purpose/)
      // Nobody at all for the role is still a refusal; a tier nobody holds is not (below).
      const critical = await cf(
        chief,
        'task',
        'add',
        '--advice',
        '--tier',
        'critical',
        '--purpose',
        'hard-problem',
        'Why is it slow?',
      )
      assert.equal(critical.code, 1)
      assert.match(critical.err, /no advisor is on the staff: ask the human for one/)
      const noTier = await cf(chief, 'task', 'add', 'Just do it')
      assert.deepEqual(
        [noTier.code, noTier.err],
        [
          2,
          'cf: cf task add --tier <critical|complex|standard|light> "what to do" (with --advice for an advisor or --review for a reviewer; or --design, --after T-3, or --self; --needs T-3,T-4 and --before T-9,T-10 order the board)',
        ],
      )

      const own = await cf(chief, 'task', 'add', '--self', '--needs', 'T-1', 'Plan the release')
      assert.deepEqual(own, {
        code: 0,
        out: 'T-2 is yours; finish it with: cf task done T-2 "what you did". It waits until T-1 is accepted.',
        err: '',
      })
      const now = await cf(chief, 'task', 'add', '--self', 'Plan the release')
      assert.deepEqual(
        [now.code, now.err],
        [
          2,
          'cf: you are already at it: do it now, or give it --needs T-3 to be woken when T-3 is accepted',
        ],
      )
      assert.equal(ledger.task(project.id, 2).assignee, 'chief')

      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      const noReviewer = await cf(chief, 'task', 'add', '--review', '--tier', 'light', 'Check it')
      assert.deepEqual(
        [noReviewer.code, noReviewer.err],
        [1, 'cf: no reviewer is on the staff: ask the human for one, in your terminal'],
      )
      const advice = await cf(
        chief,
        'task',
        'add',
        '--advice',
        '--tier',
        'standard',
        'Compare the two parsers',
      )
      assert.match(advice.out, /^T-3 is on the board for a standard advisor;/)
      assert.equal(ledger.task(project.id, 3).pool, 'advisor')
      ledger.addMember(project.id, {
        agent: 'pygmalion',
        harness: 'image',
        role: 'designer',
        tier: 'light',
      })
      const drawing = await cf(
        chief,
        'task',
        'add',
        '--design',
        'A logo: a compass rose; save it as images/logo.png',
      )
      assert.match(drawing.out, /^T-4 is on the board for an image designer;/)
      assert.deepEqual(
        [ledger.task(project.id, 4).pool, ledger.task(project.id, 4).tier],
        ['designer', null],
      )
      const fromAdvisor = await cf(token('athena'), 'task', 'add', '--tier', 'standard', 'Do it')
      assert.deepEqual(
        [fromAdvisor.code, fromAdvisor.err],
        [1, 'cf: members do not hand out tasks: ask your chief instead (cf ask)'],
      )
      const toChief = await cf(token('zeus'), 'task', 'add', '--tier', 'standard', 'Ship it')
      assert.deepEqual(
        [toChief.code, toChief.err],
        [1, 'cf: members do not hand out tasks: ask your chief instead (cf ask)'],
      )

      assert.equal(
        (await cf(chief, 'task', 'list')).out,
        [
          'Waiting for a member',
          'T-1 [open] for a standard worker ← @chief: Write the parser',
          'T-3 [open] for a standard advisor ← @chief: Compare the two parsers',
          'T-4 [open] for an image designer ← @chief: A logo: a compass rose; save it as images/logo.png',
          '@chief (chief)',
          'T-2 [open] @chief ← @chief: Plan the release',
        ].join('\n'),
      )
    })
  })

  it('lets the chief put a review on the board for a reviewer of a tier, and members ask questions only upward', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const chief = token('chief')
      const noReviewer = await cf(
        chief,
        'task',
        'add',
        '--review',
        '--tier',
        'standard',
        'Review T-1',
      )
      assert.deepEqual(
        [noReviewer.code, noReviewer.err],
        [1, 'cf: no reviewer is on the staff: ask the human for one, in your terminal'],
      )
      ledger.addMember(project.id, {
        agent: 'diana',
        harness: 'codex',
        role: 'reviewer',
        tier: 'standard',
      })
      const review = await cf(chief, 'task', 'add', '--review', '--tier', 'standard', 'Review T-1')
      assert.match(review.out, /^T-1 is on the board for a standard reviewer;/)
      assert.deepEqual(
        [ledger.task(project.id, 1).pool, ledger.task(project.id, 1).tier],
        ['reviewer', 'standard'],
      )
      const gone = await cf(chief, 'task', 'review', 'T-1')
      assert.equal(gone.code, 2, 'no command asks for a review: it is a task')

      // A member's question goes to the chief; there is no asking the human.
      const human = await cf(token('zeus'), 'ask', '--human', 'Which parser?')
      assert.equal(human.code, 2, human.err)
    })
  })
})

describe("cf hook claude: Claude's question tool answered from the board", () => {
  const EVENT = {
    session_id: 'abc',
    hook_event_name: 'PreToolUse',
    tool_name: 'AskUserQuestion',
    tool_input: {
      questions: [
        {
          question: 'Which colour?',
          header: 'Colour',
          options: [{ label: 'red', description: 'Warm' }, { label: 'blue' }],
          multiSelect: false,
        },
        {
          question: 'Which tools?',
          header: 'Tools',
          options: [{ label: 'vite' }, { label: 'esbuild' }],
          multiSelect: true,
        },
      ],
    },
  }

  it('puts the questions on the board, waits for the answer, and hands it back as updatedInput', async () => {
    await withApi(async ({ ledger, project, token, call, cf }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const hook = cf(token('zeus'), 'hook', 'claude', { input: JSON.stringify(EVENT) })
      let question = null
      for (let tries = 0; question === null && tries < 50; tries += 1) {
        await new Promise((resolve) => setTimeout(resolve, 20))
        question = ledger.inbox(participantId(ledger, project, 'chief'))[0] ?? null
      }
      assert.ok(question, 'the chief has the question')
      assert.equal(
        question.body,
        'Colour: Which colour?\n- red: Warm\n- blue\n\nTools: Which tools?\n- vite\n- esbuild',
      )
      const answered = await call(token('chief'), 'POST', '/api/answers', {
        question: question.id,
        body: 'blue\nvite, esbuild',
      })
      assert.equal(answered.status, 201)
      const { code, out } = await hook
      assert.equal(code, 0)
      assert.deepEqual(JSON.parse(out), {
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'allow',
          updatedInput: {
            ...EVENT.tool_input,
            answers: { 'Which colour?': 'blue', 'Which tools?': 'vite, esbuild' },
          },
        },
      })
    })
  })

  it("answers Devin's question tool by refusing it with the answer as the reason", async () => {
    // Probed 2026-09-20: Devin draws its dialog even over a pre-filled input,
    // but reads a refusal's reason and continues with it.
    await withApi(async ({ ledger, project, token, call, cf }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const event = { ...EVENT, tool_name: 'ask_user_question' }
      const hook = cf(token('zeus'), 'hook', 'devin', { input: JSON.stringify(event) })
      let question = null
      for (let tries = 0; question === null && tries < 50; tries += 1) {
        await new Promise((resolve) => setTimeout(resolve, 20))
        question = ledger.inbox(participantId(ledger, project, 'chief'))[0] ?? null
      }
      assert.ok(question, 'the chief has the question')
      await call(token('chief'), 'POST', '/api/answers', {
        question: question.id,
        body: 'blue\nvite, esbuild',
      })
      const { code, out } = await hook
      assert.equal(code, 0)
      assert.deepEqual(JSON.parse(out), {
        decision: 'block',
        reason:
          'ConsensFlow answered from the board: Which colour? blue · Which tools? vite, esbuild. Continue with that answer; do not ask again.',
      })
      const claudeNamed = await cf(token('zeus'), 'hook', 'devin', { input: JSON.stringify(EVENT) })
      assert.deepEqual(
        [claudeNamed.code, claudeNamed.out],
        [0, ''],
        "Claude's tool name is not Devin's",
      )
    })
  })

  it('tells the member why the board refused its question, instead of a dialog nobody sees', async () => {
    // A member's window is watched by no one: a refused question left to the
    // harness's own dialog held a reviewer's task for good (2026-09-29).
    await withApi(async ({ ledger, project, token, cf }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' }).message,
      )
      const five = {
        ...EVENT,
        tool_input: { questions: Array(5).fill(EVENT.tool_input.questions[0]) },
      }
      const claude = await cf(token('zeus'), 'hook', 'claude', { input: JSON.stringify(five) })
      assert.deepEqual(JSON.parse(claude.out), {
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason:
            'ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask "…" instead.',
        },
      })
      const devin = await cf(token('zeus'), 'hook', 'devin', {
        input: JSON.stringify({ ...five, tool_name: 'ask_user_question' }),
      })
      assert.deepEqual(JSON.parse(devin.out), {
        decision: 'block',
        reason:
          'ConsensFlow could not put this question to the chief (questions: one to 4 questions). Ask with cf ask "…" instead.',
      })
    })
  })

  it('stays silent for any other tool, and when ConsensFlow cannot be reached', async () => {
    await withApi(async ({ token, cf }) => {
      const other = await cf(token('zeus'), 'hook', 'claude', {
        input: JSON.stringify({ ...EVENT, tool_name: 'Bash', tool_input: { command: 'ls' } }),
      })
      assert.deepEqual([other.code, other.out, other.err], [0, '', ''])
      const gone = await runCoreCli(
        ['hook', 'claude'],
        { CONSENSFLOW_URL: 'http://127.0.0.1:1', CONSENSFLOW_TOKEN: 'x' },
        {
          out: () => assert.fail('nothing is printed: the window shows its own dialog'),
          err: () => assert.fail('nothing is printed'),
          input: async () => JSON.stringify(EVENT),
        },
      )
      assert.equal(gone, 0)
    })
  })
})

describe('continuing a window with --after', () => {
  it('sends a follow-up to the session that did the task, and says why when it cannot', async () => {
    await withApi(async ({ ledger, project, token, cf, call }) => {
      const chief = token('chief')
      const opened = await cf(chief, 'task', 'add', '--tier', 'standard', 'Write the parser')
      assert.equal(opened.code, 0, opened.err)
      const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
      const { message } = ledger.assignTask(project.id, 1, zeus.id)
      const session = ledger.task(project.id, 1).assignee
      assert.match(session, /^zeus-/)
      const busy = await cf(chief, 'task', 'add', '--after', 'T-1', 'Also the lexer')
      assert.deepEqual(
        [busy.code, busy.err],
        [
          1,
          `cf: @${session} is still on its work: wait for its result, or open the task for its tier`,
        ],
      )
      deliver(ledger, message)
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const followed = await cf(chief, 'task', 'add', '--after', 'T-1', 'Now the lexer')
      assert.equal(followed.code, 0, followed.err)
      assert.equal(
        followed.out,
        `T-2 continues in @${session}, the window that did T-1; its result arrives in your inbox.`,
      )
      assert.deepEqual(
        [ledger.task(project.id, 2).assignee, ledger.task(project.id, 2).state],
        [session, 'queued'],
      )
      const staff = await call(chief, 'GET', '/api/staff')
      assert.deepEqual(
        staff.body.members.map((m) => m.handle),
        ['zeus'],
        'the staff lists members, never their sessions',
      )
      ledger.cancelTask(project.id, 2, { by: 'chief' })
      ledger.acceptTask(project.id, 1, { by: 'chief' })
      const still = await cf(chief, 'task', 'add', '--after', 'T-1', 'One more')
      assert.equal(still.code, 0, 'accepted work keeps the session')
      ledger.cancelTask(project.id, 3, { by: 'chief' })
      ledger.endSession(project.id, session, { by: 'human' })
      const gone = await cf(chief, 'task', 'add', '--after', 'T-1', 'One more')
      assert.deepEqual(
        [gone.code, gone.err],
        [1, 'cf: the session that did T-1 has ended: open the task for its tier instead'],
      )
    })
  })
})

describe('cf inside a window explains itself', () => {
  const run = async (args) => {
    const lines = []
    const code = await runCoreCli(
      args,
      {},
      { out: (line) => lines.push(line), err: (line) => lines.push(line) },
    )
    return { code, text: lines.join('\n') }
  }
  it('prints its usage on --help, help, or nothing at all', async () => {
    for (const args of [['--help'], ['help'], []]) {
      const { code, text } = await run(args)
      assert.equal(code, 0)
      assert.match(text, /cf task add --tier <critical\|complex\|standard\|light>/)
      assert.match(text, /cf ask "…" {24}a question to the chief/)
    }
  })
  it('prints the task commands on cf task --help', async () => {
    const { code, text } = await run(['task', '--help'])
    assert.equal(code, 0)
    assert.match(text, /cf task add --self/)
    assert.match(text, /cf task reopen T-3/)
    assert.doesNotMatch(text, /cf whoami/)
  })
})
