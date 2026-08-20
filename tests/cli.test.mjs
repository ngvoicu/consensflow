import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { promisify } from 'node:util'
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

  it('adds, lists, edits and removes a participant', async () => {
    const added = await cf(
      [
        'participant',
        'add',
        'zeus',
        '--runtime',
        'claude',
        '--model',
        'claude-opus-5',
        '--effort',
        'max',
      ],
      t.env,
    )
    assert.equal(added.code, 0)

    const listed = await cf(['participant', 'list'], t.env)
    assert.match(listed.stdout, /zeus/)
    assert.match(listed.stdout, /claude-opus-5/)

    const asJson = await cf(['participant', 'list', '--json'], t.env)
    assert.equal(JSON.parse(asJson.stdout).participants[0].effort, 'max')

    const edited = await cf(['participant', 'edit', 'zeus', '--model', 'claude-fable-5'], t.env)
    assert.equal(edited.code, 0)

    const removed = await cf(['participant', 'remove', 'zeus'], t.env)
    assert.equal(removed.code, 0)
    assert.doesNotMatch((await cf(['participant', 'list'], t.env)).stdout, /zeus/)
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

  it('skills install writes the generated skill into every detected agent', async () => {
    await cf(
      ['participant', 'add', 'zeus', '--runtime', 'claude', '--model', 'claude-opus-5'],
      t.env,
    )
    const out = await cf(['skills', 'install'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /zeus/)
    assert.match(installed, /claude-opus-5/)
  })

  it('editing a participant regenerates the installed skill everywhere', async () => {
    await cf(['participant', 'edit', 'zeus', '--model', 'claude-fable-5'], t.env)

    for (const dir of [t.env.CLAUDE_CONFIG_DIR, t.env.CODEX_HOME]) {
      const installed = readFileSync(join(dir, 'skills', 'consensflow', 'SKILL.md'), 'utf8')
      assert.match(installed, /claude-fable-5/)
    }
  })

  it('skills status reports every owned file, and uninstall clears them', async () => {
    const status = await cf(['skills', 'status'], t.env)
    assert.match(status.stdout, /consensflow\/SKILL\.md/)
    assert.match(status.stdout, /ok/)

    const out = await cf(['skills', 'uninstall'], t.env)
    assert.equal(out.code, 0)
    assert.equal((await cf(['skills', 'status'], t.env)).stdout.trim(), 'no skills installed')
  })
})

describe('the machine runs one mode, and cf keeps it that way', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('reports no mode before one is chosen', async () => {
    const out = await cf(['mode'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /standalone/)
    assert.match(out.stdout, /not set|none/i)
  })

  it('switches to standalone and says who can consult', async () => {
    await cf(['participant', 'add', 'zeus'], t.env)
    const out = await cf(['use', 'standalone'], t.env)

    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /codex/)
    assert.ok(
      existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')),
      'codex got the generated skill',
    )
  })

  it('refuses to hand-install the generated skill in a host mode', async () => {
    // Force the mode file to claude without the payload: the guard is about
    // the invariant, not about what happens to be on disk.
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'mode.json'), JSON.stringify({ mode: 'claude' }))

    const out = await cf(['skills', 'install'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /mode/)
    assert.match(out.stdout + out.stderr, /use standalone/)
  })

  it('names the modes when given one it does not have', async () => {
    const out = await cf(['use', 'emacs'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /claude, pi, standalone/)
  })
})

describe('one command installs the host integrations', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('lists the hosts and what is installed', async () => {
    const out = await cf(['hosts'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /pi/)
    assert.match(out.stdout, /not installed/)
  })

  it('refuses a host it does not know, naming the ones it does', async () => {
    const out = await cf(['install', 'emacs'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /claude, pi/)
  })

  it('reports a missing host CLI instead of pretending', async () => {
    const out = await cf(['install', 'pi'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /pi/)
  })
})

describe('cf explains where it stands aside for a host integration', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('setup names the host that already has its own ConsensFlow', async () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })
    await cf(['participant', 'add', 'zeus'], t.env)

    const out = await cf(['setup', '--no-cmux'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /consensflow-cc/)
    assert.match(out.stdout, /--all/)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
    assert.ok(existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('installs there anyway when asked with --all', async () => {
    const out = await cf(['skills', 'install', '--all'], t.env)
    assert.equal(out.code, 0)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('doctor reports the native integration too', async () => {
    const out = await cf(['doctor'], t.env)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /own consensflow|native/i)
  })
})

describe('the catalog turns a name into a working participant', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('lists ready-made participants per tool', async () => {
    const out = await cf(['catalog'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /zeus/)
    assert.match(out.stdout, /codex/)
    assert.match(out.stdout, /hyperion/)
    assert.match(out.stdout, /glm-5\.3/)
  })

  it('narrows to one tool on request, and answers JSON for scripts', async () => {
    const out = await cf(['catalog', '--runtime', 'opencode'], t.env)
    assert.match(out.stdout, /mani/)
    assert.doesNotMatch(out.stdout, /hyperion/)

    const json = await cf(['catalog', '--json'], t.env)
    assert.ok(JSON.parse(json.stdout).catalog.pi.length > 0)
  })

  it('adds a catalog participant from its name alone', async () => {
    const out = await cf(['participant', 'add', 'hyperion'], t.env)
    assert.equal(out.code, 0)

    const listed = JSON.parse((await cf(['participant', 'list', '--json'], t.env)).stdout)
    const hyperion = listed.participants[0]
    assert.equal(hyperion.runtime, 'codex')
    assert.equal(hyperion.model, 'gpt-5.6-sol')
    assert.equal(hyperion.effort, 'ultra')
  })

  it('still requires runtime and model for a name it does not know', async () => {
    const out = await cf(['participant', 'add', 'nemo'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /cf catalog|--runtime/)
  })

  it('lets explicit flags override a catalog entry', async () => {
    await cf(['participant', 'add', 'diana', '--effort', 'low'], t.env)
    const listed = JSON.parse((await cf(['participant', 'list', '--json'], t.env)).stdout)
    const diana = listed.participants.find((p) => p.name === 'diana')
    assert.equal(diana.model, 'gpt-5.6-luna')
    assert.equal(diana.effort, 'low')
  })
})

describe('the skill heals itself when cc or pi edit the shared roster', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('any cf invocation regenerates a skill the roster has outrun', async () => {
    await cf(
      ['participant', 'add', 'zeus', '--runtime', 'claude', '--model', 'claude-opus-5'],
      t.env,
    )

    // cc adds a participant behind v3's back: a raw write to the shared file.
    const rosterFile = join(t.env.HOME, '.consensflow', 'participants.json')
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.participants.push({
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
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /apollo/)
  })

  it('never resurrects a skill the user uninstalled', async () => {
    await cf(['skills', 'uninstall'], t.env)

    const rosterFile = join(t.env.HOME, '.consensflow', 'participants.json')
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.participants[0].model = 'changed-again'
    writeFileSync(rosterFile, JSON.stringify(raw, null, 2))

    await cf(['doctor'], t.env)

    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
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
    mkdirSync(join(t.env.HOME, '.consensflow'), { recursive: true })
    cpSync(
      join(FIXTURES, 'v1-participants.json'),
      join(t.env.HOME, '.consensflow', 'participants.json'),
    )

    const out = await cf(['setup', '--no-cmux'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /hyperion/)
    // The unsupported image participant is skipped by the skill, not the roster.
    assert.doesNotMatch(installed, /pygmalion/)
    const listed = await cf(['participant', 'list'], t.env)
    assert.match(listed.stdout, /pygmalion/)
  })

  it('a participant added on top regenerates the installed skill', async () => {
    await cf(
      ['participant', 'add', 'freya', '--runtime', 'claude', '--model', 'claude-opus-5'],
      t.env,
    )

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /freya/)
    assert.match(installed, /hyperion/)
  })

  it('is idempotent', async () => {
    const out = await cf(['setup', '--no-cmux'], t.env)
    assert.equal(out.code, 0)
  })
})
