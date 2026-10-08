import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { pathToFileURL } from 'node:url'
import { promisify } from 'node:util'
import { cliTarget } from './cli-target.mjs'
import { fakeExecutable, tempEnv } from './helpers.mjs'

/** A launcher is `cf` on POSIX and `cf.cmd` on Windows. */
const CMD = process.platform === 'win32' ? '.cmd' : ''

const run = promisify(execFile)
/** The native cf's own sources, whose words a look is taken at. */
const CF_SOURCES = join(import.meta.dirname, '..', 'crates', 'cf', 'src')
const FIXTURES = join(import.meta.dirname, 'fixtures')
/** Where the roster is kept in a home of a test's own. */
const rosterPath = (env) => join(env.CONSENSFLOW_HOME, 'agents.json')
/** A preload that has every Node process say it started: which cf ran is told by it. */
const NODE_SPY = join(FIXTURES, 'node-spy.mjs')
/** The cf these tests run: the native one (tests/cli-target.mjs, `npm run test:clis`). */
const target = cliTarget()
async function cf(args, env) {
  try {
    const { stdout, stderr } = await run(target.command, [...target.args, ...args], {
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
  fakeExecutable(path)
}

describe('cf manages the roster', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it(`runs ${target.name}`, async () => {
    // The native cf is given no Node for the catalog: that it answers says it
    // is the native cf that did, and did not hand the verb on.
    const out = await cf(['catalog', '--harness', 'pi'], t.env)
    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /^pi:\n/)
  })

  // Which cf ran is told by the processes that started, not by the selection:
  // the native cf serves the catalog with no Node at all, and a selection that
  // came to a cf that handed the verb to a Node process would start one.
  it('is the native cf: no Node process ran', async () => {
    const own = tempEnv()
    try {
      const marks = join(own.root, 'node-runs')
      const out = await cf(['catalog', '--harness', 'pi'], {
        ...own.env,
        NODE_OPTIONS: `--import=${pathToFileURL(NODE_SPY).href}`,
        CF_TEST_SPY: marks,
      })
      assert.equal(out.code, 0, out.stderr)
      const ran = existsSync(marks) ? readFileSync(marks, 'utf8').split('\n').filter(Boolean) : []
      assert.deepEqual(ran, [], 'a Node process ran')
    } finally {
      own.cleanup()
    }
  })

  it('adds, lists, edits and removes an agent', async () => {
    const added = await cf(
      [
        'agent',
        'add',
        'mine',
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
    assert.match(listed.stdout, /mine/)
    assert.match(listed.stdout, /claude-opus-5/)

    const asJson = await cf(['agent', 'list', '--json'], t.env)
    assert.equal(JSON.parse(asJson.stdout).agents.find((p) => p.name === 'mine').effort, 'max')

    const edited = await cf(['agent', 'edit', 'mine', '--model', 'claude-fable-5-1'], t.env)
    assert.equal(edited.code, 0)

    const removed = await cf(['agent', 'remove', 'mine'], t.env)
    assert.equal(removed.code, 0)
    assert.doesNotMatch((await cf(['agent', 'list'], t.env)).stdout, /mine/)
  })

  it('adds an image agent: a Codex agent that designs, on no other harness', async () => {
    const added = await cf(
      ['agent', 'add', 'my-draw', '--harness', 'codex', '--model', 'codex-image', '--designer'],
      t.env,
    )
    assert.equal(added.code, 0, added.stderr)
    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const draw = listed.agents.find((p) => p.name === 'my-draw')
    assert.deepEqual(
      [draw.harness, draw.designer, draw.profile.modelLabel],
      ['codex', true, 'Codex Images'],
    )
    const elsewhere = await cf(
      ['agent', 'add', 'pi-draw', '--harness', 'pi', '--model', 'x', '--designer'],
      t.env,
    )
    assert.equal(elsewhere.code, 1)
    assert.match(elsewhere.stderr, /an image agent is a Codex agent/)
    // `image` is no harness of its own any more.
    const old = await cf(['agent', 'add', 'old-draw', '--harness', 'image', '--model', 'x'], t.env)
    assert.match(old.stderr, /unknown harness "image"/)
    await cf(['agent', 'remove', 'my-draw'], t.env)
  })

  it('refuses a flag it would ignore, and writes nothing for it', async () => {
    for (const flag of [['--dry-run'], ['--from', 'x'], ['--presets', 'x']]) {
      const out = await cf(
        ['agent', 'add', 'trial', '--harness', 'codex', '--model', 'gpt-6-astra', ...flag],
        t.env,
      )
      assert.notEqual(out.code, 0, flag[0])
      assert.match(out.stderr, new RegExp(`Unknown option '${flag[0]}'`))
    }
    assert.doesNotMatch((await cf(['agent', 'list'], t.env)).stdout, /trial/)
  })

  it('fails an unknown verb loudly', async () => {
    const out = await cf(['frobnicate'], t.env)
    assert.notEqual(out.code, 0)
  })

  it('prints its version', async () => {
    const out = await cf(['--version'], t.env)
    assert.match(out.stdout, /3\.0\.0/)
  })

  it('survives its output pipe closing early, like `cf … | head`', {
    skip: process.platform === 'win32' && 'a POSIX shell pipeline',
  }, async () => {
    const { spawn } = await import('node:child_process')
    // `false` never reads: the pipe is closed before cf writes anything, so
    // every write EPIPEs. PIPESTATUS surfaces cf's own exit code.
    const command = [target.command, ...target.args].map((word) => `"${word}"`).join(' ')
    const child = spawn('/bin/bash', ['-c', `${command} help | false; exit \${PIPESTATUS[0]}`], {
      env: { ...t.env, PATH: `${t.env.PATH}:/usr/bin:/bin` },
      stdio: ['ignore', 'ignore', 'pipe'],
    })
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
    // A launch's own role file, where a window writes it (crates/cf-harness/src/shared/role.rs).
    const role = join(
      t.env.CONSENSFLOW_HOME,
      'integrations',
      'claude',
      'launch-canary',
      'role',
      '.claude',
      'skills',
      'consensflow-chief',
      'SKILL.md',
    )
    mkdirSync(dirname(role), { recursive: true })
    writeFileSync(role, 'role canary')
    const manifest = join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')
    writeFileSync(manifest, '{"files":{}}')
    for (const args of [
      ['agent', 'add', 'mine', '--harness', 'claude', '--model', 'example'],
      ['agent', 'edit', 'mine', '--effort', 'high'],
      ['agent', 'list', '--json'],
      ['catalog'],
      ['doctor'],
      ['setup'],
      ['agent', 'remove', 'mine'],
    ]) {
      const result = await cf(args, t.env)
      assert.equal(result.code, 0, result.stderr)
      assert.equal(readFileSync(role, 'utf8'), 'role canary', args.join(' '))
      assert.equal(readFileSync(manifest, 'utf8'), '{"files":{}}')
    }
    assert.ok(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, `cf${CMD}`)))
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
  it('removes direct conversation writes and terminal-window discovery from cf', () => {
    const sources = readdirSync(CF_SOURCES, { recursive: true }).filter((file) =>
      file.endsWith('.rs'),
    )
    assert.ok(sources.length > 0, `no source of the native cf in ${CF_SOURCES}`)
    for (const file of sources) {
      assert.doesNotMatch(
        readFileSync(join(CF_SOURCES, file), 'utf8'),
        /\bsaveThread\b|liveWindowElsewhere|CMUX_SURFACE_ID|cmux tree/,
        file,
      )
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

  it('lists a catalog agent from its name alone, and refuses to add it again', async () => {
    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const hyperion = listed.agents.find((p) => p.name === 'hyperion')
    assert.equal(hyperion.harness, 'codex')
    assert.equal(hyperion.model, 'gpt-6.1-sol')
    assert.equal(hyperion.effort, 'max')
    const out = await cf(['agent', 'add', 'hyperion'], t.env)
    assert.equal(out.code, 1)
    assert.match(out.stderr, /catalog agent/)
  })

  it('a catalog agent is not edited or removed from here either', async () => {
    const edited = await cf(['agent', 'edit', 'hyperion', '--effort', 'low'], t.env)
    assert.equal(edited.code, 1)
    assert.match(edited.stderr, /catalog agent and stays/)
    const removed = await cf(['agent', 'remove', 'hyperion'], t.env)
    assert.equal(removed.code, 1)
    assert.match(removed.stderr, /not yours to remove/)
    assert.equal((await cf(['agent', 'reset', 'hyperion'], t.env)).code, 1)
  })

  it('still requires harness and model for a name it does not know', async () => {
    const out = await cf(['agent', 'add', 'nemo'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /cf catalog|--harness/)
  })

  it('an edit changes one field of an agent of your own and keeps the rest', async () => {
    await cf(
      [
        'agent',
        'add',
        'my-luna',
        '--harness',
        'codex',
        '--model',
        'gpt-5.6-luna',
        '--effort',
        'xhigh',
      ],
      t.env,
    )
    await cf(['agent', 'edit', 'my-luna', '--effort', 'low'], t.env)
    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const luna = listed.agents.find((p) => p.name === 'my-luna')
    assert.equal(luna.model, 'gpt-5.6-luna')
    assert.equal(luna.effort, 'low')
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
    // Role files are a launch's: setup writes none.
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'integrations')), false)
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
        await cf(['agent', 'add', 'mine', '--harness', 'claude', '--model', 'example'], t.env)
        assert.equal((await cf(['setup'], t.env)).code, 0)
        const history = join(t.env.CONSENSFLOW_HOME, 'workspaces', 'project', 'runs', 'run-1')
        mkdirSync(history, { recursive: true })
        const files = [
          rosterPath(t.env),
          join(t.env.CONSENSFLOW_BIN_DIR, `cf${CMD}`),
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
it('the chief can discover saved capability profiles without refreshing or changing role files', async () => {
  const t = tempEnv()
  try {
    const added = await cf(
      ['agent', 'add', 'mine', '--harness', 'codex', '--model', 'gpt-5.6-luna'],
      t.env,
    )
    assert.equal(added.code, 0, added.stderr)
    const refused = await cf(['agent', 'add', 'hyperion'], t.env)
    assert.equal(refused.code, 1)
    assert.match(refused.stderr, /catalog agent/)
    const role = join(
      t.env.CONSENSFLOW_HOME,
      'integrations/claude/launch-canary/role/.claude/skills/consensflow-chief/SKILL.md',
    )
    mkdirSync(dirname(role), { recursive: true })
    writeFileSync(role, 'Keep existing chief context untouched')
    const roster = readFileSync(rosterPath(t.env), 'utf8')
    const result = await cf(['agent', 'list', '--json'], { ...t.env, CONSENSFLOW_ROLE: 'chief' })
    assert.equal(result.code, 0, result.stderr)
    const agents = JSON.parse(result.stdout).agents
    const hyperion = agents.find((agent) => agent.name === 'hyperion')
    assert.ok(hyperion, 'every catalog agent is listed')
    assert.equal(agents.find((agent) => agent.name === 'mine').custom, true)
    assert.equal(Object.hasOwn(hyperion.profile, 'categories'), false, 'no role pills')
    assert.ok(hyperion.profile.workTier)
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), roster)
    assert.equal(readFileSync(role, 'utf8'), 'Keep existing chief context untouched')
  } finally {
    t.cleanup()
  }
})
