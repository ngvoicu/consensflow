import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
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

describe('roster changes keep installed skills current', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('roster creation prepares private context and retired skill installation is a no-op', async () => {
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    const out = await cf(['skills', 'install'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(
        t.env.CONSENSFLOW_HOME,
        'roles',
        'lead',
        '.claude',
        'skills',
        'consensflow-lead',
        'SKILL.md',
      ),
      'utf8',
    )
    assert.match(installed, /zeus/)
    assert.match(installed, /claude-opus-5/)
  })

  it('editing an agent regenerates the private lead skill', async () => {
    await cf(['agent', 'edit', 'zeus', '--model', 'claude-fable-5-1'], t.env)

    for (const dir of [join(t.env.CONSENSFLOW_HOME, 'roles', 'lead', '.claude')]) {
      const installed = readFileSync(join(dir, 'skills', 'consensflow-lead', 'SKILL.md'), 'utf8')
      assert.match(installed, /claude-fable-5-1/)
    }
  })

  it('skills status reports every owned file, and uninstall clears them', async () => {
    const status = await cf(['skills', 'status'], t.env)
    assert.match(status.stdout, /consensflow-lead\/SKILL\.md/)
    assert.match(status.stdout, /ok/)

    const out = await cf(['skills', 'uninstall'], t.env)
    assert.equal(out.code, 0)
    assert.equal((await cf(['skills', 'status'], t.env)).stdout.trim(), 'no skills installed')
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

describe('the standalone CLI installs the skill without a mode selection', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('roster creation and explicit install maintain the private lead skill', async () => {
    const added = await cf(['agent', 'add', 'zeus'], t.env)
    assert.equal(added.code, 0, added.stderr)
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'mode.json'), JSON.stringify({ mode: 'claude' }))
    rmSync(join(t.env.CODEX_HOME, 'skills', 'consensflow'), { recursive: true, force: true })
    const installed = await cf(['skills', 'install'], t.env)
    assert.equal(installed.code, 0, installed.stderr)
    for (const home of [join(t.env.CONSENSFLOW_HOME, 'roles', 'lead', '.claude')]) {
      assert.ok(existsSync(join(home, 'skills', 'consensflow-lead', 'SKILL.md')))
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

describe('private skill installation is independent of old host integrations', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('setup generates the private role even when an old host integration exists', async () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })
    await cf(['agent', 'add', 'zeus'], t.env)

    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.equal(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
      true,
    )
    assert.ok(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
    )
  })

  it('legacy --all does not change the private destination', async () => {
    const out = await cf(['skills', 'install', '--all'], t.env)
    assert.equal(out.code, 0)
    assert.ok(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
    )
  })

  it('doctor reports the native integration too', async () => {
    const out = await cf(['doctor'], t.env)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /own consensflow|native/i)
  })
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

describe('the skill heals itself when cc or pi edit the shared roster', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('any cf invocation regenerates a skill the roster has outrun', async () => {
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)

    // cc adds an agent behind v3's back: a raw write to the shared file.
    const rosterFile = rosterPath(t.env)
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.agents.push({
      id: 'apollo',
      name: 'Apollo',
      kind: 'codex',
      toolsPolicy: 'workspace-write',
      model: 'gpt-5.6-terra',
      effort: 'xhigh',
    })
    writeFileSync(rosterFile, JSON.stringify(raw, null, 2))

    // Any verb at all — not a skills verb — notices and heals.
    await cf(['doctor'], t.env)

    const installed = readFileSync(
      join(
        t.env.CONSENSFLOW_HOME,
        'roles',
        'lead',
        '.claude',
        'skills',
        'consensflow-lead',
        'SKILL.md',
      ),
      'utf8',
    )
    assert.match(installed, /apollo/)
  })

  it('never resurrects a skill the user uninstalled', async () => {
    await cf(['skills', 'uninstall'], t.env)

    const rosterFile = rosterPath(t.env)
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.agents[0].model = 'changed-again'
    writeFileSync(rosterFile, JSON.stringify(raw, null, 2))

    await cf(['doctor'], t.env)

    assert.equal(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
      false,
    )
  })
})

describe('cf setup readies a machine in one command', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('a machine that already ran cc or pi gets its skill from the shared roster', async () => {
    // The cc/pi roster IS the roster: no import, no copy.
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))

    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(
        t.env.CONSENSFLOW_HOME,
        'roles',
        'lead',
        '.claude',
        'skills',
        'consensflow-lead',
        'SKILL.md',
      ),
      'utf8',
    )
    assert.match(installed, /hyperion/)
    // The image agent is in the skill too now: one verb spawns it like the rest.
    assert.match(installed, /pygmalion/)
    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /pygmalion/)
  })

  it('an agent added on top regenerates the installed skill', async () => {
    await cf(['agent', 'add', 'freya', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)

    const installed = readFileSync(
      join(
        t.env.CONSENSFLOW_HOME,
        'roles',
        'lead',
        '.claude',
        'skills',
        'consensflow-lead',
        'SKILL.md',
      ),
      'utf8',
    )
    assert.match(installed, /freya/)
    assert.match(installed, /hyperion/)
  })

  it('is idempotent', async () => {
    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)
  })
})

describe('the CLI can undo an install as completely as the app', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('takes everything back but the roster', async () => {
    stubCli(t, 'claude')
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    await cf(['setup'], t.env)
    const skill = join(
      t.env.CONSENSFLOW_HOME,
      'roles',
      'lead',
      '.claude',
      'skills',
      'consensflow-lead',
      'SKILL.md',
    )
    assert.ok(existsSync(skill), 'the generated skill is installed for the chosen scope')

    const off = await cf(['off'], t.env)
    assert.equal(off.code, 0)
    assert.match(off.stdout, /off/i)

    assert.equal(existsSync(skill), false, 'the skill is taken back')
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts')), false, 'no payload survives')
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'mode.json')), false)

    // Agents are the user's, and outlive any install.
    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /zeus/)
  })
})

describe('cf reset is the clean slate, and refuses until you say so', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  const skill = () =>
    join(
      t.env.CONSENSFLOW_HOME,
      'roles',
      'lead',
      '.claude',
      'skills',
      'consensflow-lead',
      'SKILL.md',
    )

  it('prints what it would destroy and touches nothing', async () => {
    stubCli(t, 'claude')
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    await cf(['setup'], t.env)
    mkdirSync(join(t.env.CONSENSFLOW_HOME, 'workspaces', 'proj', 'runs', 'ask-1'), {
      recursive: true,
    })
    assert.ok(existsSync(skill()))

    const out = await cf(['reset'], t.env)

    // The refusal IS the preview — same two numbers the page puts in its
    // dialog, printed while everything is still there.
    assert.notEqual(out.code, 0, 'a destructive default is not a default')
    assert.match(out.stdout + out.stderr, /1 agent and 1 run/)
    assert.match(out.stdout + out.stderr, /nothing was touched/)
    assert.ok(existsSync(skill()), 'the skill survives a refusal')
    assert.ok(existsSync(rosterPath(t.env)), 'and so does the roster')
  })

  it('removes everything once told, and says what went', async () => {
    const out = await cf(['reset', '--yes'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /1 agent and 1 run went with it/)
    assert.equal(existsSync(skill()), false)
    assert.equal(existsSync(t.env.CONSENSFLOW_HOME), false, 'the whole root is gone')
  })

  it('counts nothing, and still works, on a machine with nothing installed', async () => {
    const out = await cf(['reset', '--yes'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /0 agents and 0 runs/)
  })
})

it('retired skill install/update commands explain bundled skills without creating files', async () => {
  const t = tempEnv()
  try {
    for (const action of ['install', 'update']) {
      const result = await cf(['skills', action], t.env)
      assert.equal(result.code, 0)
      assert.match(result.stdout, /included with ConsensFlow/)
      assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false)
    }
  } finally {
    t.cleanup()
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

it('PM CLI rejects local roster and worker commands before changing any app files', async () => {
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
      ['run', '@zeus', 'forbidden'],
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
