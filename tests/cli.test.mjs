import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { chmodSync, cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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

  it('imports the v1 roster from an explicit path', async () => {
    const out = await cf(
      [
        'participant',
        'import-v1',
        '--from',
        join(FIXTURES, 'v1-participants.json'),
        '--presets',
        join(FIXTURES, 'v1-presets.js'),
      ],
      t.env,
    )
    assert.equal(out.code, 0)
    assert.match(out.stdout, /imported 4/)
    assert.match(out.stdout, /pygmalion/)
    for (const name of ['zeus', 'hyperion', 'endymion', 'mani']) {
      await cf(['participant', 'remove', name], t.env)
    }
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

describe('cf setup readies a machine in one command', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('suggests a v1 import when it finds one, but never imports on its own', async () => {
    // A fake v1 roster in the fake home.
    mkdirSync(join(t.env.HOME, '.consensflow'), { recursive: true })
    cpSync(
      join(FIXTURES, 'v1-participants.json'),
      join(t.env.HOME, '.consensflow', 'participants.json'),
    )

    const out = await cf(['setup', '--no-cmux'], t.env)
    assert.equal(out.code, 0)
    // Participants are the user's to create — cf ui / cf participant add.
    assert.match(out.stdout, /import-v1/)
    assert.doesNotMatch(out.stdout, /imported \d/)
    assert.match(out.stdout, /cf ui/)

    const listed = await cf(['participant', 'list', '--json'], t.env)
    assert.equal(JSON.parse(listed.stdout).participants.length, 0)
  })

  it('the first participant added installs the skill everywhere, no separate step', async () => {
    await cf(
      ['participant', 'add', 'zeus', '--runtime', 'claude', '--model', 'claude-opus-5'],
      t.env,
    )

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /zeus/)
  })

  it('is idempotent', async () => {
    const out = await cf(['setup', '--no-cmux'], t.env)
    assert.equal(out.code, 0)
  })
})
