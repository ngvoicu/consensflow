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
  it('removes direct conversation writes and terminal-window discovery from cf', () => {
    const source = readFileSync(CF, 'utf8')
    assert.doesNotMatch(source, /\bsaveThread\b|liveWindowElsewhere|CMUX_SURFACE_ID|cmux tree/)
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
