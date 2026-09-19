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
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    lead: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
    tags: ['coding', 'rust'],
  })
  const participant = (handle) =>
    ledger.project(project.id).participants.find((p) => p.handle === handle)
  const token = (handle) =>
    credentials.issue({ participant: participant(handle), project, generation: 1 })
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
      const added = await call(token('lead'), 'POST', '/api/tasks', {
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
      const named = await call(token('lead'), 'POST', '/api/tasks', { to: 'zeus', body: 'Lexer' })
      assert.deepEqual([named.status, named.body.error], [403, 'name-a-tier'])
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
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'human', to: 'lead', body: 'Ship v2' }).message,
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
    await withApi(async ({ ledger, project, token, call }) => {
      deliver(
        ledger,
        ledger.createTask(project.id, { from: 'lead', to: 'zeus', body: 'Parser' }).message,
      )
      const zeus = token('zeus')
      const asked = await call(zeus, 'POST', '/api/questions', { body: 'Which format?' })
      assert.equal(asked.status, 201)
      assert.deepEqual([asked.body.message.recipient, asked.body.message.task], ['lead', 1])
      assert.equal(ledger.task(project.id, 1).state, 'waiting')
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
    await withApi(async ({ ledger, project, token, call }) => {
      const note = ledger.note(project.id, { from: 'lead', to: 'human', body: 'private' })
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
      const added = await cf(lead, 'task', 'add', '--tier', 'standard', 'Write', 'the', 'parser')
      assert.deepEqual(added, {
        code: 0,
        out: 'T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.',
        err: '',
      })
      assert.equal(
        (await cf(lead, 'task', 'list')).out,
        'Waiting for a member\nT-1 [open] for a standard worker ← @lead: Write the parser',
      )
      assert.equal((await cf(lead, 'whoami')).out, '@lead (lead) in project app')
      const json = await cf(lead, 'task', 'get', 'T-1', '--json')
      assert.deepEqual(
        [JSON.parse(json.out).body, JSON.parse(json.out).messages],
        ['Write the parser', []],
      )
    })
  })

  it('shows the project team as roles, tiers and tags, nothing to pick a member by', async () => {
    await withApi(async ({ token, cf, ledger, project }) => {
      assert.equal(
        (await cf(token('lead'), 'team')).out,
        '@zeus · worker · standard · coding, rust',
      )
      ledger.setRoles(project.id, 'zeus', ['worker', 'reviewer'])
      assert.equal(
        (await cf(token('lead'), 'team')).out,
        '@zeus · worker+reviewer · standard · coding, rust',
      )
    })
  })

  it('explains a mistake and exits 2 for bad usage, 1 for a refusal', async () => {
    await withApi(async ({ token, cf }) => {
      const lead = token('lead')
      const usage = await cf(lead, 'task', 'add', 'no target')
      assert.deepEqual(
        [usage.code, usage.err],
        [2, 'cf: cf task add --tier <critical|complex|standard|light> "what to do" (or --self)'],
      )
      const missing = await cf(lead, 'task', 'done', 'T-9', 'x')
      assert.deepEqual([missing.code, missing.err], [1, 'cf: no task T-9 in this project'])
      const outside = await cf('revoked', 'whoami')
      assert.equal(outside.code, 1)
      assert.match(outside.err, /no ConsensFlow access/)
    })
  })
})

describe('tiered tasks through the API and cf', () => {
  it('opens a task for a tier, never for a member by name', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      const lead = token('lead')
      const opened = await cf(
        lead,
        'task',
        'add',
        '--tier',
        'standard',
        '--tags',
        'coding,rust',
        'Write the parser',
      )
      assert.deepEqual(opened, {
        code: 0,
        out: 'T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.',
        err: '',
      })
      const task = ledger.task(project.id, 1)
      assert.deepEqual(
        [task.state, task.pool, task.tier, task.tags],
        ['open', 'worker', 'standard', ['coding', 'rust']],
      )

      const named = await cf(lead, 'task', 'add', '@zeus', 'Write the lexer')
      assert.deepEqual(
        [named.code, named.err],
        [1, 'cf: @zeus is a worker: name a tier, not a member (cf task add --tier standard "…")'],
      )
      const noPurpose = await cf(lead, 'task', 'add', '--tier', 'critical', 'Why is it slow?')
      assert.equal(noPurpose.code, 1)
      assert.match(noPurpose.err, /critical work names its purpose/)
      const critical = await cf(
        lead,
        'task',
        'add',
        '--tier',
        'critical',
        '--purpose',
        'hard-problem',
        'Why is it slow?',
      )
      assert.equal(critical.code, 1)
      assert.match(critical.err, /no critical worker is on the team/)
      const noTier = await cf(lead, 'task', 'add', 'Just do it')
      assert.deepEqual(
        [noTier.code, noTier.err],
        [2, 'cf: cf task add --tier <critical|complex|standard|light> "what to do" (or --self)'],
      )

      const own = await cf(lead, 'task', 'add', '--self', 'Plan the release')
      assert.deepEqual(own, {
        code: 0,
        out: 'T-2 is yours; finish it with: cf task done T-2 "what you did".',
        err: '',
      })
      assert.equal(ledger.task(project.id, 2).assignee, 'lead')

      ledger.addPm(project.id, { harness: 'pi' })
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      const advice = await cf(
        token('pm'),
        'task',
        'add',
        '--tier',
        'standard',
        'Compare the two parsers',
      )
      assert.match(advice.out, /^T-3 is on the board for a standard advisor;/)
      assert.equal(ledger.task(project.id, 3).pool, 'advisor')
      const toLead = await cf(token('pm'), 'task', 'add', '@lead', 'Ship it')
      assert.deepEqual(
        [toLead.code, toLead.err],
        [
          1,
          'cf: agents give no task by name: put it on the board for a tier (cf task add --tier standard "…"); only the human gives the lead or the PM a task',
        ],
      )
      assert.equal((await cf(lead, 'task', 'add', '@pm', 'Plan it')).code, 1)

      assert.equal(
        (await cf(lead, 'task', 'list')).out,
        [
          'Waiting for a member',
          'T-1 [open] for a standard worker ← @lead: Write the parser',
          'T-3 [open] for a standard advisor ← @pm: Compare the two parsers',
          '@lead (lead)',
          'T-2 [queued] @lead ← @lead: Plan the release',
        ].join('\n'),
      )
    })
  })

  it('lets a coordinator ask for a review by hand, and members ask questions only upward', async () => {
    await withApi(async ({ ledger, project, token, cf }) => {
      ledger.setReview(project.id, 'none')
      ledger.addMember(project.id, {
        agent: 'diana',
        harness: 'codex',
        role: 'reviewer',
        tier: 'standard',
      })
      const { message } = ledger.createTask(project.id, {
        from: 'lead',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      ledger.recordResult(project.id, 1, { body: 'Done' })
      const asked = await cf(token('lead'), 'task', 'review', 'T-1')
      assert.deepEqual(asked, {
        code: 0,
        out: 'T-1 goes to an independent reviewer; the verdict arrives in your inbox.',
        err: '',
      })
      assert.equal(ledger.task(project.id, 1).state, 'review')
      const refused = await cf(token('zeus'), 'task', 'review', 'T-1')
      assert.equal(refused.code, 1)
      assert.match(refused.err, /only coordinators ask for reviews/)

      const sideways = await cf(token('zeus'), 'ask', '--to', '@diana', 'Which parser?')
      assert.deepEqual(
        [sideways.code, sideways.err],
        [1, 'cf: questions go to your coordinator or the human, not to @diana'],
      )
      const upward = await cf(token('zeus'), 'ask', '--to', '@lead', 'Which parser?')
      assert.equal(upward.code, 0)
      const human = await cf(token('zeus'), 'ask', '--human', 'Which parser?')
      assert.equal(human.code, 0)
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
      assert.match(text, /cf ask "…" \[--human\]/)
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
