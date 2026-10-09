import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * The call sites that moved to `cargo xtask` (tools/xtask): the npm names stay
 * for the habit, the workflows and the scripts that run them, and each now
 * hands over to the one command. That each command named here is one xtask has
 * is tools/xtask/tests/cli.rs's.
 */

const read = (path) =>
  JSON.parse(readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8'))
const only = (scripts, names) => Object.fromEntries(names.map((name) => [name, scripts[name]]))

describe('the root package.json', () => {
  it('runs each driver and build step through cargo xtask', () => {
    const { scripts } = read('package.json')
    assert.deepEqual(
      only(scripts, [
        'build:cf',
        'test:app',
        'clippy:app',
        'clippy:windows',
        'test:daemons',
        'test:integration',
        'test:clis',
        'test:agents',
        'load',
        'departures',
        'bench:records-memory',
        'smoke',
        'smoke:updater',
        'candidate',
      ]),
      {
        'build:cf': 'cargo xtask build-cf',
        'test:app': 'cargo xtask app test',
        'clippy:app': 'cargo xtask app clippy',
        'clippy:windows': 'cargo xtask clippy-windows',
        'test:daemons': 'cargo xtask test daemons',
        'test:integration': 'cargo xtask test integration',
        'test:clis': 'cargo xtask test clis',
        'test:agents': 'cargo xtask test agents',
        load: 'cargo xtask test load',
        departures: 'cargo xtask departures',
        'bench:records-memory': 'cargo xtask bench records-memory',
        smoke: 'cargo xtask smoke',
        'smoke:updater': 'cargo xtask smoke-updater',
        candidate: 'cargo xtask candidate',
      },
    )
  })

  it('keeps lint and test as the npm check, which cargo xtask check calls', () => {
    const { scripts } = read('package.json')
    assert.equal(scripts.check, 'npm run lint && npm test')
  })
})

describe('the app package.json', () => {
  it('stages and packs through cargo xtask, and leaves building to Tauri', () => {
    const { scripts } = read('app/package.json')
    assert.deepEqual(
      only(scripts, ['prepare-sidecar', 'prepare:app', 'portable', 'build', 'dev']),
      {
        'prepare-sidecar': 'cargo xtask stage',
        'prepare:app': 'npm run bundle:ui && npm run prepare-sidecar',
        portable: 'cargo xtask portable',
        build: 'tauri build',
        dev: 'tauri dev',
      },
    )
  })
})

describe('the Tauri configuration', () => {
  it('prepares the app before a build, and before a dev run, which waits for it', () => {
    const { build } = read('app/src-tauri/tauri.conf.json')
    assert.equal(build.beforeBuildCommand, 'npm run prepare:app')
    assert.deepEqual(build.beforeDevCommand, { script: 'npm run prepare:app', wait: true })
  })

  it('is merged over by the candidate and the Windows configurations without a word of build', () => {
    // They are merged over this one by Tauri (JSON merge patch): a `build` of theirs
    // would replace the hook, and a candidate or a Windows bundle would go unstaged.
    for (const file of ['tauri.candidate.conf.json', 'tauri.windows.conf.json']) {
      assert.equal(Object.hasOwn(read(`app/src-tauri/${file}`), 'build'), false, file)
    }
  })
})
