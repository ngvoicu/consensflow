import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * No retained test imports Node's sources (`src/`, `hosts/lib/`), step 4 of the
 * Rust rewrite: the tests that stay after the deletion run the native daemon
 * and `cf`, or fixtures of their own. What still imports them is listed here,
 * file by file, with the landing that removes it; a file that is not listed
 * and imports them fails. The folder `hosts/` keeps what is not Node's daemon:
 * the Pi extension, the OpenCode plugin and its door.
 *
 *   B1  the bundle without Node: the app's own scripts
 *   B2  the deletion: the unit suites of ported modules, the recorders and the
 *       parity tools, and the tests of the hand-off between the two
 *   B4  the evals and the live tools
 */
const REPO = fileURLToPath(new URL('..', import.meta.url))
const ROOTS = ['tests', 'app/tests', 'app/scripts', 'evals']

/** What of `hosts/` stays after the deletion, and so may be imported. */
const STAYS = ['hosts/pi-extension/', 'hosts/opencode-extension/', 'hosts/lib/question-door.js']

/** The files that import Node's sources and go: a file, or a folder when it ends in a slash. */
const REMOVED_BY = {
  B1: ['app/scripts/launcher-entry.mjs', 'tests/launcher-entry.test.mjs'],
  B2: [
    ...[
      'adapter-claude',
      'adapter-codex',
      'adapter-devin',
      'adapter-opencode',
      'adapter-pi',
      'adapter-shared',
      'catalog',
      'channels',
      'core-agents-server',
      'core-delivery-text',
      'core-dispatcher',
      'core-launch-files',
      'core-log',
      'core-pane-host',
      'core-page',
      'core-roles',
      'core-trace',
      'devin-hooks',
      'handoff',
      'harness-admin',
      'harnesses',
      'host-payloads',
      'install',
      'launch-runner',
      'ledger',
      'ledger-conversations',
      'ledger-gate',
      'ledger-page-reads',
      'ledger-schema',
      'ledger-staff',
      'ledger-tasks',
      'opencode-install',
      'pi-install',
      'quota',
      'role-skills',
      'roster',
      'skill',
      'terminal',
      'way-back',
    ].map((name) => `tests/${name}.test.mjs`),
    'tests/core-api-fixture.mjs',
    'tests/ledger-fixtures.mjs',
    'tests/integration/home-round-trip.test.mjs',
    'tests/bench/records-node.mjs',
    'tests/engine/',
    'tests/goldens/',
    'tests/parity/',
  ],
  B4: [
    'evals/bare.mjs',
    'evals/measure.mjs',
    'evals/run.mjs',
    'tests/evals-bare.test.mjs',
    'tests/evals-measure.test.mjs',
    'tests/integration/home-copies.mjs',
    'tests/integration/home-fixture.mjs',
    'tests/live/catalog-agent.mjs',
    'tests/live/codex-interrupted-question.mjs',
    'tests/live/codex-resume.mjs',
    'tests/live/interrupt-idle.mjs',
    'tests/live/live-window.mjs',
    'tests/live/receipt-rig.mjs',
  ],
}

const posix = (path) => path.replaceAll('\\', '/')
const covers = (listed, file) => (listed.endsWith('/') ? file.startsWith(listed) : file === listed)

/** Every script under the roots that tests, tools and scripts live in. */
function scripts() {
  return ROOTS.flatMap((root) =>
    readdirSync(join(REPO, root), { recursive: true })
      .map(posix)
      .filter((file) => /\.(mjs|js|cjs)$/.test(file) && !file.includes('node_modules/'))
      .map((file) => `${root}/${file}`),
  )
}

/** The relative modules the `text` of the script `file` imports, by the path of each from the repository. */
function importsIn(text, file) {
  const specifier = /(?:\bfrom\s*|\bimport\s*\(?\s*)(['"])(\.{1,2}\/[^'"]+)\1/g
  return [...text.matchAll(specifier)].map(([, , path]) =>
    posix(relative(REPO, resolve(dirname(join(REPO, file)), path))),
  )
}

/** The relative modules the script `file` imports. */
const importsOf = (file) => importsIn(readFileSync(join(REPO, file), 'utf8'), file)

/** Whether a module is one of Node's sources that goes. */
const goes = (module) =>
  (module.startsWith('src/') || module.startsWith('hosts/')) &&
  !STAYS.some((kept) => covers(kept, module))

const listed = Object.values(REMOVED_BY).flat()
/** This file holds samples of what it looks for, which are not imports. */
const SELF = 'tests/off-node-sources.test.mjs'

describe('no retained test imports the sources of Node', () => {
  const importers = scripts()
    .filter((file) => file !== SELF)
    .filter((file) => importsOf(file).some(goes))

  // What it looks for, shown on text of its own: it finds an import of Node's
  // sources in each way one is written, and none of what stays.
  it('finds a module of Node’s sources however it is imported, and nothing else', () => {
    const text = [
      "import { rosterPath } from '../../src/roster.js'",
      "export * from '../../hosts/lib/windows.js'",
      "const { Bridge } = await import('../../src/bridge.js')",
      "import '../../hosts/lib/quota.js'",
      "import { send } from '../../hosts/pi-extension/consensflow-delivery.mjs'",
      "import { ask } from '../../hosts/lib/question-door.js'",
      "import { join } from 'node:path'",
      "import { startIntegration } from '../integration/harness.mjs'",
    ].join('\n')
    const imported = importsIn(text, 'tests/engine/example.test.mjs')
    assert.deepEqual(imported.filter(goes), [
      'src/roster.js',
      'hosts/lib/windows.js',
      'src/bridge.js',
      'hosts/lib/quota.js',
    ])
    assert.equal(imported.length, 7, 'those of a relative path are read, and only those')
  })

  it('reads the scripts it is to hold, and knows the host extensions stay', () => {
    assert.ok(scripts().includes(SELF), 'tests are read')
    assert.ok(
      scripts().some((file) => file.startsWith('app/tests/')),
      'the page tests are read',
    )
    const extension = importsOf('tests/opencode-extension.test.mjs')
    assert.ok(extension.some((module) => module.startsWith('hosts/')))
    assert.equal(extension.some(goes), false, 'the host extensions are not Node’s daemon')
  })

  it('has every importer listed with the landing that removes it', () => {
    const unlisted = importers.filter((file) => !listed.some((entry) => covers(entry, file)))
    assert.deepEqual(unlisted, [], 'a retained test imports Node’s sources: use the native side')
  })

  // A landing that removes a file need not touch this list; the one that
  // removes the sources has the list emptied with it.
  it('lists nothing once the sources of Node are gone', {
    skip: existsSync(join(REPO, 'src')) && 'src/ is still there',
  }, () => {
    assert.deepEqual(listed, [])
  })
})
