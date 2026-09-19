import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { promisify } from 'node:util'
import { rosterPath } from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

const run = promisify(execFile)
const CF = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
const FIXTURES = join(import.meta.dirname, 'fixtures')

it('task CLI uses scoped, revision-protected operations for both coordinators', async () => {
  const { createServer } = await import('node:http')
  const t = tempEnv()
  const requests = []
  const server = createServer(async (request, response) => {
    let body = ''
    for await (const chunk of request) body += chunk
    requests.push({ path: request.url, body: JSON.parse(body) })
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ id: 'task-1', revision: 2, tasks: [], total: 0 }))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    for (const role of ['lead', 'pm']) {
      const env = {
        ...t.env,
        CONSENSFLOW_ROLE: role,
        CONSENSFLOW_APP: `http://127.0.0.1:${server.address().port}`,
        CONSENSFLOW_APP_TOKEN: 'test-token',
        CONSENSFLOW_TAB: 't-owner',
      }
      const calls = [
        [['task', 'list', '--offset', '100', '--json'], 'task.list', { offset: 100 }],
        [['task', 'get', 'task-1', '--json'], 'task.get', { id: 'task-1' }],
        [
          [
            'task',
            'add',
            'Review contracts',
            '--kind',
            'review',
            '--review-of',
            'task-0',
            '--depends-on',
            'task-0',
            '--json',
          ],
          'task.change',
          {
            change: {
              action: 'add',
              title: 'Review contracts',
              kind: 'review',
              reviewOf: 'task-0',
              dependsOn: ['task-0'],
            },
          },
        ],
        [
          [
            'task',
            'update',
            'task-1',
            '--revision',
            '2',
            '--status',
            'blocked',
            '--question',
            'Which constraint wins?',
            '--json',
          ],
          'task.change',
          {
            change: {
              action: 'update',
              id: 'task-1',
              revision: 2,
              status: 'blocked',
              question: 'Which constraint wins?',
            },
          },
        ],
      ]
      for (const [args, op, body] of calls) {
        const result = await cf(args, env)
        assert.equal(result.code, 0, result.stderr)
        assert.equal(JSON.parse(result.stdout).revision, 2)
        assert.deepEqual(requests.at(-1), {
          path: `/api/panes/${op}`,
          body: { tab: 't-owner', ...body },
        })
      }
      const before = requests.length
      for (const args of [
        ['task', 'update', 'task-1', '--status', 'accepted'],
        ['task', 'update', 'task-1', '--revision', '-1'],
        ['task', 'list', '--offset', 'NaN'],
        ['task', 'answer', 'task-1', 'forged answer'],
        ['task', 'get', 'task-1', 'extra'],
      ])
        assert.equal((await cf(args, env)).code, 1, args.join(' '))
      assert.equal((await cf(['task', 'list'], { ...env, CONSENSFLOW_CHILD: '1' })).code, 1)
      assert.equal(requests.length, before)
    }
    assert.equal((await cf(['task', 'list'], t.env)).code, 1)
  } finally {
    await new Promise((resolve) => server.close(resolve))
    t.cleanup()
  }
})

async function cf(args, env) {
  try {
    const { stdout, stderr } = await run(process.execPath, [CF, ...args], {
      env,
      timeout: 30_000,
    })
    return { code: 0, stdout, stderr }
  } catch (cause) {
    return { code: cause.code ?? 1, stdout: cause.stdout ?? '', stderr: cause.stderr ?? '' }
  }
}

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('cf manages the roster', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('adds, lists, edits and removes an agent', async () => {
    const added = await cf(
      [
        'agent',
        'add',
        'zeus',
        '--harness',
        'claude',
        '--model',
        'claude-opus-5',
        '--effort',
        'max',
      ],
      t.env,
    )
    assert.equal(added.code, 0)

    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /zeus/)
    assert.match(listed.stdout, /claude-opus-5/)

    const asJson = await cf(['agent', 'list', '--json'], t.env)
    assert.equal(JSON.parse(asJson.stdout).agents[0].effort, 'max')

    const edited = await cf(['agent', 'edit', 'zeus', '--model', 'claude-fable-5-1'], t.env)
    assert.equal(edited.code, 0)

    const removed = await cf(['agent', 'remove', 'zeus'], t.env)
    assert.equal(removed.code, 0)
    assert.doesNotMatch((await cf(['agent', 'list'], t.env)).stdout, /zeus/)
  })

  it('fails an unknown verb loudly', async () => {
    const out = await cf(['frobnicate'], t.env)
    assert.notEqual(out.code, 0)
  })

  it('prints its version', async () => {
    const out = await cf(['--version'], t.env)
    assert.match(out.stdout, /3\.0\.0/)
  })

  it('survives its output pipe closing early, like `cf … | head`', async () => {
    const { spawn } = await import('node:child_process')
    // `false` never reads: the pipe is closed before cf writes anything, so
    // every write EPIPEs. PIPESTATUS surfaces cf's own exit code.
    const child = spawn(
      '/bin/bash',
      ['-c', `"${process.execPath}" "${CF}" help | false; exit \${PIPESTATUS[0]}`],
      {
        env: { ...t.env, PATH: `${t.env.PATH}:/usr/bin:/bin` },
        stdio: ['ignore', 'ignore', 'pipe'],
      },
    )
    let stderr = ''
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    const code = await new Promise((resolve) => child.on('close', resolve))
    assert.doesNotMatch(stderr, /EPIPE/)
    assert.equal(code, 0)
  })
})

describe('role files belong to pane launch, not CLI administration', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('roster edits, setup and diagnostic reads leave role files and old manifests alone', async () => {
    const role = join(
      t.env.CONSENSFLOW_HOME,
      'roles',
      'lead',
      '.claude',
      'skills',
      'consensflow-lead',
      'SKILL.md',
    )
    mkdirSync(dirname(role), { recursive: true })
    writeFileSync(role, 'role canary')
    const manifest = join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')
    writeFileSync(manifest, '{"files":{}}')
    for (const args of [
      ['agent', 'add', 'zeus'],
      ['agent', 'edit', 'zeus', '--effort', 'high'],
      ['agent', 'sync', 'zeus'],
      ['agent', 'list', '--json'],
      ['catalog'],
      ['doctor'],
      ['setup'],
      ['agent', 'remove', 'zeus'],
    ]) {
      const result = await cf(args, t.env)
      assert.equal(result.code, 0, result.stderr)
      assert.equal(readFileSync(role, 'utf8'), 'role canary', args.join(' '))
      assert.equal(readFileSync(manifest, 'utf8'), '{"files":{}}')
    }
    assert.ok(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, 'cf')))
  })

  it('skills administration is absent, including forced uninstall', async () => {
    for (const action of ['install', 'update', 'status', 'uninstall']) {
      const result = await cf(['skills', action, '--force'], t.env)
      assert.equal(result.code, 1)
      assert.match(result.stderr, /unknown command/)
    }
  })
})

describe('the standalone switch-over (TEST-PANE-47)', () => {
  it('retires use and mode with the current command list', async () => {
    const t = tempEnv()
    try {
      for (const args of [['mode'], ['use', 'cmux'], ['use', 'claude'], ['use', 'pi']]) {
        const result = await cf(args, t.env)
        assert.equal(result.code, 1)
        assert.match(result.stderr, /ConsensFlow has one shape now/)
        assert.match(result.stdout + result.stderr, /cf <command>/)
        assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'mode.json')), false)
      }
    } finally {
      t.cleanup()
    }
  })

  it('doctor reports a legacy mode file once without treating it as configuration', async () => {
    const t = tempEnv()
    try {
      mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
      const path = join(t.env.CONSENSFLOW_HOME, 'mode.json')
      const original = JSON.stringify({ mode: 'claude' })
      writeFileSync(path, original)
      const result = await cf(['doctor'], t.env)
      assert.equal(result.code, 0, result.stderr)
      assert.equal((result.stdout.match(/mode\.json/g) ?? []).length, 1)
      assert.match(result.stdout, /ignored.*remov|remov.*ignored/i)
      assert.doesNotMatch(result.stdout, /^mode:/m)
      assert.equal(readFileSync(path, 'utf8'), original)
    } finally {
      t.cleanup()
    }
  })

  it('run outside an app pane refuses without launching or writing conversations', async () => {
    const t = tempEnv()
    try {
      stubCli(t, 'codex')
      await cf(['agent', 'add', 'diana'], t.env)
      const result = await cf(['run', '@diana', 'hello', '--new'], t.env)
      assert.equal(result.code, 1)
      assert.match(result.stderr, /ConsensFlow.*app|app.*ConsensFlow/i)
      assert.doesNotMatch(result.stderr, /cmux/)
      assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'workspaces')), false)
    } finally {
      t.cleanup()
    }
  })

  it('removes direct conversation writes and terminal-window discovery from cf', () => {
    const source = readFileSync(CF, 'utf8')
    assert.doesNotMatch(source, /\bsaveThread\b|liveWindowElsewhere|CMUX_SURFACE_ID|cmux tree/)
  })
})

describe('app-owned conversation names (TEST-PANE-47)', () => {
  it('retires pre-minting a name outside the app', async () => {
    const t = tempEnv()
    try {
      await cf(['agent', 'add', 'zeus'], t.env)
      const result = await cf(['mint', '@zeus'], t.env)
      assert.equal(result.code, 1)
      assert.match(result.stderr, /app.*names|names.*app/i)
      assert.match(result.stderr, /cf run/)
    } finally {
      t.cleanup()
    }
  })
})

describe('the host-integration verbs are gone, not hidden', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  for (const verb of ['hosts', 'install', 'uninstall']) {
    it(`no longer answers \`${verb}\``, async () => {
      const out = await cf([verb, 'claude'], t.env)
      assert.notEqual(out.code, 0)
      assert.match(out.stdout + out.stderr, /unknown command/)
    })
  }
})

describe('the catalog turns a name into a working agent', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('lists ready-made agents per tool', async () => {
    const out = await cf(['catalog'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /zeus/)
    assert.match(out.stdout, /codex/)
    assert.match(out.stdout, /hyperion/)
    assert.match(out.stdout, /glm-5\.3/)
  })

  it('narrows to one tool on request, and answers JSON for scripts', async () => {
    const out = await cf(['catalog', '--harness', 'opencode'], t.env)
    assert.match(out.stdout, /mani/)
    assert.doesNotMatch(out.stdout, /hyperion/)

    const json = await cf(['catalog', '--json'], t.env)
    assert.ok(JSON.parse(json.stdout).catalog.pi.length > 0)
  })

  it('adds a catalog agent from its name alone', async () => {
    const out = await cf(['agent', 'add', 'hyperion'], t.env)
    assert.equal(out.code, 0)

    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const hyperion = listed.agents[0]
    assert.equal(hyperion.harness, 'codex')
    assert.equal(hyperion.model, 'gpt-5.6-sol')
    assert.equal(hyperion.effort, 'max')
  })

  it('syncs a hyperion row from ultra to max — label, effort and description', async () => {
    // The row was added when the catalog still named ultra; pin it back to
    // the old values, then `cf agent sync` moves the preset-owned fields.
    const path = rosterPath(t.env)
    const document = JSON.parse(readFileSync(path, 'utf8'))
    const row = document.agents.find((r) => r.id === 'hyperion')
    row.effort = 'ultra'
    row.description = 'Codex GPT 5.6 Sol ULTRA'
    writeFileSync(path, `${JSON.stringify(document, null, 2)}\n`)

    const out = await cf(['agent', 'sync', 'hyperion'], t.env)
    assert.equal(out.code, 0)

    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const hyperion = listed.agents.find((p) => p.name === 'hyperion')
    assert.equal(hyperion.effort, 'max')
    assert.equal(hyperion.description, 'Codex GPT 5.6 Sol MAX')
  })

  it('still requires harness and model for a name it does not know', async () => {
    const out = await cf(['agent', 'add', 'nemo'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /cf catalog|--harness/)
  })

  it('lets explicit flags override a catalog entry', async () => {
    await cf(['agent', 'add', 'diana', '--effort', 'low'], t.env)
    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const diana = listed.agents.find((p) => p.name === 'diana')
    assert.equal(diana.model, 'gpt-5.6-luna')
    assert.equal(diana.effort, 'low')
  })
})

it('setup preserves a legacy roster and prepares no role or manifest before pane launch', async () => {
  const t = tempEnv()
  try {
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))
    const before = readFileSync(rosterPath(t.env))
    for (let i = 0; i < 2; i++) assert.equal((await cf(['setup'], t.env)).code, 0)
    assert.deepEqual(readFileSync(rosterPath(t.env)), before)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')), false)
    assert.match((await cf(['agent', 'list'], t.env)).stdout, /pygmalion/)
  } finally {
    t.cleanup()
  }
})

describe('retired off/reset CLI commands preserve the installation and saved data', () => {
  for (const args of [['off'], ['off', '--force'], ['reset'], ['reset', '--yes']]) {
    it(`rejects cf ${args.join(' ')}`, async () => {
      const t = tempEnv()
      try {
        await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'example'], t.env)
        assert.equal((await cf(['setup'], t.env)).code, 0)
        const history = join(t.env.CONSENSFLOW_HOME, 'workspaces', 'project', 'runs', 'run-1')
        mkdirSync(history, { recursive: true })
        const files = [
          rosterPath(t.env),
          join(t.env.CONSENSFLOW_BIN_DIR, 'cf'),
          join(history, 'answer.txt'),
        ]
        writeFileSync(files[2], 'saved result')
        const before = files.map((path) => readFileSync(path))
        const result = await cf(args, t.env)
        assert.equal(result.code, 1)
        assert.match(result.stderr, /unknown command/i)
        assert.deepEqual(
          files.map((path) => readFileSync(path)),
          before,
        )
      } finally {
        t.cleanup()
      }
    })
  }
})

it('PM CLI sends exact file contents and retrieves immutable lead parts in one request', async () => {
  const { createServer } = await import('node:http')
  const t = tempEnv()
  const requests = []
  const server = createServer(async (request, response) => {
    let body = ''
    for await (const chunk of request) body += chunk
    requests.push({ path: request.url, body: JSON.parse(body) })
    response.setHeader('content-type', 'application/json')
    response.end(
      JSON.stringify(
        request.url.endsWith('lead.send')
          ? { outcome: 'admitted' }
          : { text: 'whole requested part\n', deliveryId: 'd-12', of: 2, part: 2 },
      ),
    )
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const env = {
      ...t.env,
      CONSENSFLOW_APP: `http://127.0.0.1:${server.address().port}`,
      CONSENSFLOW_APP_TOKEN: 'pm-token',
      CONSENSFLOW_TAB: 't-2',
      CONSENSFLOW_ROLE: 'pm',
    }
    const file = join(t.root, 'message.md')
    writeFileSync(file, 'Exact plan\nwith its final newline.\n')
    const sent = await cf(['lead', 'send', '--message-file', file], env)
    assert.equal(sent.code, 0, sent.stderr)
    assert.equal(requests[0].path, '/api/panes/lead.send')
    assert.equal(requests[0].body.text, readFileSync(file, 'utf8'))
    const read = await cf(['lead', 'read', '--answer', 'd-12', '--part', '2'], env)
    assert.equal(read.code, 0, read.stderr)
    assert.equal(read.stdout, 'whole requested part\n')
    assert.equal(requests.length, 2)
    assert.equal(requests[1].body.answerId, 'd-12')
    assert.equal(requests[1].body.part, 2)
  } finally {
    await new Promise((resolve) => server.close(resolve))
    t.cleanup()
  }
})

it('PM can discover saved capability profiles without refreshing or changing role files', async () => {
  const t = tempEnv()
  try {
    const added = await cf(['agent', 'add', 'hyperion'], t.env)
    assert.equal(added.code, 0, added.stderr)
    const role = join(t.env.CONSENSFLOW_HOME, 'roles/lead/.claude/skills/consensflow-lead/SKILL.md')
    mkdirSync(dirname(role), { recursive: true })
    writeFileSync(role, 'Keep existing lead context untouched')
    const roster = readFileSync(rosterPath(t.env), 'utf8')
    const result = await cf(['agent', 'list', '--json'], { ...t.env, CONSENSFLOW_ROLE: 'pm' })
    assert.equal(result.code, 0, result.stderr)
    const agents = JSON.parse(result.stdout).agents
    assert.equal(agents.length, 1)
    assert.equal(agents[0].name, 'hyperion')
    assert.ok(agents[0].profile.goodFor.length > 0)
    assert.ok(agents[0].profile.categories.includes('coding'))
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), roster)
    assert.equal(readFileSync(role, 'utf8'), 'Keep existing lead context untouched')
  } finally {
    t.cleanup()
  }
})

it('PM CLI rejects local roster and administration commands before changing any app files', async () => {
  const t = tempEnv()
  try {
    const env = {
      ...t.env,
      CONSENSFLOW_ROLE: 'pm',
      CONSENSFLOW_APP: 'http://127.0.0.1:1',
      CONSENSFLOW_APP_TOKEN: 'pm-token',
      CONSENSFLOW_TAB: 't-2',
    }
    for (const args of [
      ['agent', 'add', 'forbidden', '--harness', 'codex'],
      ['agent', 'edit', 'forbidden', '--model', 'other'],
      ['agent', 'remove', 'forbidden'],
      ['agent', 'sync', '--all'],
      ['skills', 'uninstall', '--force'],
      ['reset', '--yes'],
    ]) {
      const result = await cf(args, env)
      assert.equal(result.code, 1)
      assert.match(result.stderr, /PM.*lead send.*lead read/)
    }
    assert.equal(existsSync(rosterPath(t.env)), false)
  } finally {
    t.cleanup()
  }
})

it('CLI exposes tier overrides and refuses unclassified critical tasks before contacting the app', async () => {
  const t = tempEnv()
  try {
    assert.equal((await cf(['agent', 'add', 'calliope'], t.env)).code, 0)
    const denied = await cf(['run', '@calliope', 'Make a small code change'], t.env)
    assert.notEqual(denied.code, 0)
    assert.match(denied.stderr, /--purpose/)
    const allowed = await cf(
      ['run', '@calliope', 'Review this architecture', '--purpose', 'architecture'],
      t.env,
    )
    assert.doesNotMatch(allowed.stderr, /Unknown option|requires --purpose/)
    const edited = await cf(['agent', 'edit', 'calliope', '--work-tier', 'standard'], t.env)
    assert.equal(edited.code, 0, edited.stderr)
    const row = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout).agents[0]
    assert.equal(row.workTier, 'standard')
    assert.equal(row.profile.workTier, 'standard')
    assert.equal((await cf(['agent', 'edit', 'calliope', '--work-tier', 'auto'], t.env)).code, 0)
    assert.equal(
      (
        await cf(
          [
            'agent',
            'add',
            'custom',
            '--harness',
            'codex',
            '--model',
            'custom-model',
            '--work-tier',
            'complex',
          ],
          t.env,
        )
      ).code,
      0,
    )
    assert.doesNotMatch(
      (await cf(['say', 'conversation', 'Review', '--purpose', 'critical-review'], t.env)).stderr,
      /Unknown option/,
    )
  } finally {
    t.cleanup()
  }
})
