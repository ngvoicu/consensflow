import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { preparePiExtension } from '../src/pi-install.js'
import { tempEnv } from './helpers.mjs'

test('Pi absent never creates an extension', () => {
  const t = tempEnv()
  try {
    assert.equal(preparePiExtension(t.env).state, 'not-installed')
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'extensions')), false)
  } finally {
    t.cleanup()
  }
})

test('detected Pi gets an immutable private extension with working imports and no global edits', async () => {
  const t = tempEnv()
  try {
    const bin = join(t.root, 'bin')
    mkdirSync(bin)
    writeFileSync(join(bin, 'pi'), '#!/bin/sh\nexit 0\n')
    chmodSync(join(bin, 'pi'), 0o755)
    t.env.PATH = bin
    const global = join(t.env.HOME, '.pi', 'agent')
    mkdirSync(global, { recursive: true })
    writeFileSync(join(global, 'settings.json'), '{"extensions":["user-extension"]}')
    const first = preparePiExtension(t.env)
    assert.equal(first.state, 'installed-unverified')
    assert.ok(first.path.startsWith(t.env.CONSENSFLOW_HOME))
    await import(first.path)
    assert.deepEqual(preparePiExtension(t.env), first)
    assert.equal(
      readFileSync(join(global, 'settings.json'), 'utf8'),
      '{"extensions":["user-extension"]}',
    )
    writeFileSync(first.path, 'damaged')
    assert.equal(
      preparePiExtension(t.env).state,
      'error',
      'never overwrite code possibly loaded by a live process',
    )
  } finally {
    t.cleanup()
  }
})

test('Pi preparation failure is reported, not a crash or false OK', () => {
  const t = tempEnv()
  try {
    const bin = join(t.root, 'bin')
    mkdirSync(bin)
    writeFileSync(join(bin, 'pi'), '#!/bin/sh\nexit 0\n')
    chmodSync(join(bin, 'pi'), 0o755)
    t.env.PATH = bin
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'extensions'), 'cannot create directory here')
    assert.equal(preparePiExtension(t.env).state, 'error')
  } finally {
    t.cleanup()
  }
})

test('opening the app prepares Pi only when its executable is detected', async () => {
  const { installEverywhere } = await import('../src/install.js')
  const t = tempEnv()
  try {
    assert.equal(installEverywhere(t.env).piExtension.state, 'not-installed')
    mkdirSync(t.env.PATH, { recursive: true })
    writeFileSync(join(t.env.PATH, 'pi'), '#!/bin/sh\nexit 0\n')
    chmodSync(join(t.env.PATH, 'pi'), 0o755)
    assert.equal(installEverywhere(t.env).piExtension.state, 'installed-unverified')
  } finally {
    t.cleanup()
  }
})
