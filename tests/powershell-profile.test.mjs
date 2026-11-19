import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, statSync, utimesSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { PowerShellStartLock, pwshEnvironment } from './support/powershell-start-lock.mjs'

test('a PowerShell child is configured to avoid the shared startup profile', () => {
  const env = pwshEnvironment({
    other: 'preserved',
    dotnet_MultiCoreJitMinNumCpus: '0',
    COMPlus_MultiCoreJitMinNumCpus: '0',
  })
  assert.equal(env.other, 'preserved')
  assert.equal(env.DOTNET_MultiCoreJitMinNumCpus, '65535')
  assert.equal(env.POWERSHELL_TELEMETRY_OPTOUT, '1')
  assert.equal(env.dotnet_MultiCoreJitMinNumCpus, undefined)
  assert.equal(env.COMPlus_MultiCoreJitMinNumCpus, '65535')
})

test('a protected PowerShell neither reads nor rewrites an existing owned profile', {
  skip: process.platform === 'win32' || spawnSync('which', ['pwsh']).status !== 0,
}, async (t) => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-pwsh-profile-read-'))
  try {
    const base = { ...process.env, XDG_CACHE_HOME: work, POWERSHELL_TELEMETRY_OPTOUT: '1' }
    delete base.DOTNET_MultiCoreJitMinNumCpus
    delete base.COMPlus_MultiCoreJitMinNumCpus
    const run = async (env) => {
      const result = await new PowerShellStartLock().start(() => ({
        ready: Promise.resolve(),
        value: spawnSync(
          'pwsh',
          ['-NoProfile', '-NonInteractive', '-Command', 'Write-Output ready'],
          {
            env,
            encoding: 'utf8',
          },
        ),
      }))
      assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout.trim(), 'ready')
    }
    await run(base)
    const profile = path.join(work, 'powershell', 'StartupProfileData-NonInteractive')
    assert.ok(existsSync(profile), 'the unprotected control made a profile')
    const old = new Date('2000-01-01T00:00:00.000Z')
    utimesSync(profile, old, statSync(profile).mtime)
    await run(base)
    if (statSync(profile).atimeMs === old.getTime()) {
      return t.skip('the filesystem did not record the control read')
    }
    const lastWrite = statSync(profile).mtime
    utimesSync(profile, old, lastWrite)
    const before = statSync(profile)
    await run(pwshEnvironment(base))
    const after = statSync(profile)
    assert.equal(after.atimeMs, old.getTime(), 'the protected process did not read the profile')
    assert.equal(after.mtimeMs, before.mtimeMs, 'the protected process did not rewrite the profile')
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})

test('the runtime setting prevents writes to an owned PowerShell profile', {
  skip: process.platform === 'win32' || spawnSync('which', ['pwsh']).status !== 0,
}, async () => {
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-pwsh-profile-'))
  try {
    const run = async (name, protectedStart) => {
      const cache = path.join(work, name)
      mkdirSync(cache)
      const base = { ...process.env, XDG_CACHE_HOME: cache, POWERSHELL_TELEMETRY_OPTOUT: '1' }
      delete base.DOTNET_MultiCoreJitMinNumCpus
      delete base.COMPlus_MultiCoreJitMinNumCpus
      const env = protectedStart ? pwshEnvironment(base) : base
      const result = await new PowerShellStartLock().start(() => ({
        ready: Promise.resolve(),
        value: spawnSync(
          'pwsh',
          ['-NoProfile', '-NonInteractive', '-Command', 'Write-Output ready'],
          {
            env,
            encoding: 'utf8',
          },
        ),
      }))
      assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout.trim(), 'ready')
      return existsSync(path.join(cache, 'powershell', 'StartupProfileData-NonInteractive'))
    }
    assert.equal(await run('control', false), true, 'the unprotected control recorded a profile')
    assert.equal(
      await run('protected', true),
      false,
      'the protected child did not record a profile',
    )
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
})
