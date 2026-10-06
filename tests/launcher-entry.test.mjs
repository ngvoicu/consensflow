import assert from 'node:assert/strict'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { launcherEntry, nativeCf } from '../app/scripts/launcher-entry.mjs'
import { installTerminalCommand } from '../src/terminal.js'
import { tempEnv } from './helpers.mjs'

/**
 * Which CLI the command on PATH runs, as `app/scripts/sync-cli.mjs` reads it, in both
 * shapes of the launcher: the one alpha.78 wrote (the runtime and the `cf.mjs`),
 * and the one the app's start repairs it to (the native `cf` beside the `cf.mjs`).
 * Once an app has started, a developer's PATH copy is the second.
 */

const REPO = fileURLToPath(new URL('..', import.meta.url))
const MARK = 'Installed by ConsensFlow'

/** The new shape's launcher text, as `cf_launcher` writes it (`text.rs`), for sh and for cmd.exe. */
const sh = (cf, pin = '') =>
  `#!/bin/sh\n# ${MARK}. Runs the app's own cf, so the terminal and the\n# window never drift apart.\n${pin}exec "${cf}" "$@"\n`
const cmd = (cf, pin = '') =>
  `@echo off\r\nREM ${MARK}. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n${pin}"${cf}" %*\r\n`

/** An app bundle's `cf` path, with the bundle's `Info.plist` naming `identifier` when it has one. */
function bundle(t, name, identifier = null) {
  const contents = join(t.root, `${name}.app`, 'Contents')
  const cf = join(contents, 'Resources', 'cli', 'bin', 'cf')
  if (identifier !== null) {
    mkdirSync(contents, { recursive: true })
    writeFileSync(
      join(contents, 'Info.plist'),
      `<plist><dict><key>CFBundleIdentifier</key>\n<string>${identifier}</string></dict></plist>`,
    )
  }
  return cf
}

/** Puts `text` where the command of this home is looked for first. */
function launcher(t, text, name = 'consensflow') {
  const file = join(t.env.CONSENSFLOW_BIN_DIR, name)
  mkdirSync(t.env.CONSENSFLOW_BIN_DIR, { recursive: true })
  writeFileSync(file, text)
  return file
}

describe('the CLI the command on PATH runs, by either shape of the launcher', () => {
  it('is the cf.mjs the old shape names', () => {
    const t = tempEnv()
    try {
      installTerminalCommand(t.env)
      assert.deepEqual(launcherEntry(t.env), { entry: join(REPO, 'bin', 'cf.mjs'), live: false })
    } finally {
      t.cleanup()
    }
  })

  it('is the cf.mjs beside the native cf the new shape names', () => {
    const t = tempEnv()
    try {
      const cf = bundle(t, 'Candidate', 'dev.ngvoicu.consensflow.candidate')
      launcher(t, sh(cf))
      assert.deepEqual(launcherEntry(t.env), {
        entry: join(t.root, 'Candidate.app', 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs'),
        live: false,
      })
    } finally {
      t.cleanup()
    }
  })

  it('says the live app’s bundle is live, in the new shape as in the old', () => {
    const t = tempEnv()
    try {
      const cf = bundle(t, 'ConsensFlow', 'dev.ngvoicu.consensflow')
      launcher(t, sh(cf, `export CONSENSFLOW_HOME="${t.env.CONSENSFLOW_HOME}"\n`))
      assert.equal(launcherEntry(t.env).live, true)
    } finally {
      t.cleanup()
    }
  })

  it('reads the form of cmd.exe where the environment is Windows’s', () => {
    const t = tempEnv()
    const env = { ...t.env, OS: 'Windows_NT' }
    try {
      // A percent sign is doubled between the quotes of a `.cmd`.
      launcher(t, cmd('C:/Apps/100%%/cli/bin/cf.exe'), 'consensflow.cmd')
      assert.equal(launcherEntry(env).entry, join('C:/Apps/100%/cli/bin', 'cf.mjs'))
    } finally {
      t.cleanup()
    }
  })

  it('is none where there is no command of ours, or one that says neither', () => {
    const t = tempEnv()
    try {
      assert.equal(launcherEntry(t.env), null, 'no command')
      launcher(t, '#!/bin/sh\nexec echo hello\n')
      assert.equal(launcherEntry(t.env), null, 'not ours')
      launcher(t, `#!/bin/sh\n# ${MARK}.\necho nothing the repair knows\n`)
      assert.equal(launcherEntry(t.env), null, 'ours, and it runs something else')
    } finally {
      t.cleanup()
    }
  })
})

describe('the native cf a launcher names', () => {
  it('is the path, with what sh reads for itself unescaped', () => {
    assert.equal(
      nativeCf(sh('/Applications/ConsensFlow.app/cli/bin/cf')),
      '/Applications/ConsensFlow.app/cli/bin/cf',
    )
    // As the launcher writes a path with a space, a quote, a dollar and a backquote in it.
    const written = ['/a b/', '\\"', '\\$', '\\`', '/cf'].join('')
    assert.equal(nativeCf(sh(written)), '/a b/"$`/cf')
  })

  it('is none for a line that is not the command alone', () => {
    for (const text of [
      'exec "/n" "/x/cf.mjs" "$@"\n',
      'exec "/cf" "$@" extra\n',
      '"/cf" %* extra\n',
    ]) {
      assert.equal(nativeCf(text), null, text)
    }
  })
})
