import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * Nothing imports what the deletion took away, step 4 of the Rust rewrite:
 * Node's daemon, CLI and ledger (`src/`, `bin/cf.mjs`) and the host libraries
 * they shared (`hosts/lib/`, but for the question door the OpenCode plugin
 * loads). What stays runs the native daemon and `cf`, or fixtures of its own.
 *
 * The dangling-import check is the guard of the imports: a script that imports a
 * file that was deleted names a file that is not there, so it fails here, in
 * whatever way the import is written, and not when someone runs the tool. The
 * one hole is a file put back, which an import would then find: the deleted
 * tree is held deleted, by name. What is read at run time, by a path in a
 * string, an import cannot show: a last test reads every file of the tree and
 * finds the deleted paths named, but where a recording or an older release's
 * layout says them on purpose.
 *
 * Each check is a function of a tree, so the last tests plant what the checks
 * are for (an import of a deleted file, a deleted file put back, a path in a
 * string) in a tree of their own, and see each one found.
 */
const REPO = fileURLToPath(new URL('..', import.meta.url))
/** The folders whose scripts are held: tests, tools, the page's specs, the evals and the host extensions. */
const ROOTS = ['tests', 'app/tests', 'app/scripts', 'evals', 'hosts']

/** What the deletion took away. */
const DELETED = [
  'src',
  'bin/cf.mjs',
  'hosts/lib/completion',
  'hosts/lib/completion.js',
  'hosts/lib/presets.js',
  'hosts/lib/quota.js',
  'hosts/lib/windows.js',
]

/** The one library of the host extensions that stays. */
const DOOR = 'question-door.js'

const posix = (path) => path.replaceAll('\\', '/')

/** Every script under the roots of the tree `root`. */
function scripts(root = REPO) {
  return ROOTS.flatMap((dir) =>
    readdirSync(join(root, dir), { recursive: true })
      .map(posix)
      .filter((file) => /\.(mjs|js|cjs)$/.test(file) && !file.includes('node_modules/'))
      .map((file) => `${dir}/${file}`),
  )
}

/** The relative modules the `text` of the script `file` imports, by the path of each from the tree `root`. */
function importsIn(text, file, root = REPO) {
  const specifier = /(?:\bfrom\s*|\bimport\s*\(?\s*)(['"])(\.{1,2}\/[^'"]+)\1/g
  return [...text.matchAll(specifier)].map(([, , path]) =>
    posix(relative(root, resolve(dirname(join(root, file)), path))),
  )
}

/** The relative modules the script `file` of the tree `root` imports. */
const importsOf = (file, root = REPO) =>
  importsIn(readFileSync(join(root, file), 'utf8'), file, root)

/** This file holds samples of what it looks for, which are not imports. */
const SELF = 'tests/off-node-sources.test.mjs'

/** The deleted paths, as a text names them. */
const NAMED =
  /\bsrc\/(?:core|adapters|channels)\/|\bsrc\/[a-z/-]+\.js\b|bin\/cf\.mjs|hosts\/lib\/(?:completion|presets|quota|windows)/

/**
 * Where the deleted paths are named on purpose, and why: data recorded from
 * Node and from the harnesses, fixed since (and the one test that gives the
 * dispatcher the text of a task a recorded trace holds, `src/parse.js`, a file
 * of an imagined project); the layout of the bundles older releases shipped,
 * which the updater and its smoke still meet; and the command an older build
 * wrote, which names a `cf.mjs` and is read and repaired.
 */
const SAYS_THEM = [
  /^crates\/[^/]+\/tests\/(?:goldens|traces)\//,
  /^crates\/cf-engine\/tests\/dispatcher\/review\.rs$/,
  /^tests\/(?:engine\/)?fixtures\//,
  /^app\/scripts\/feeds\.mjs$/,
  /^app\/src-tauri\/src\/update_install\.rs$/,
  /^tests\/(?:feeds|publish|release-publish)\.test\.mjs$/,
  /^tests\/updater-smoke/,
  /^crates\/cf-launcher\//,
  /^crates\/cf\/tests\/cli_goldens\//,
  /^tests\/plants\/cli\/admin\.mjs$/,
]

/** Every file of this repository that git knows or would take, as written on this disk. */
function tree() {
  return execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
    cwd: REPO,
    encoding: 'utf8',
    maxBuffer: 1 << 26,
  })
    .split('\0')
    .filter((file) => file !== '' && existsSync(join(REPO, file)))
}

/** The scripts of the tree `root` that import a file that is not there, each as `script imports module`. */
function dangling(root = REPO) {
  return scripts(root)
    .filter((file) => file !== SELF)
    .flatMap((file) =>
      importsOf(file, root)
        .filter((module) => !existsSync(join(root, module)))
        .map((module) => `${file} imports ${module}`),
    )
}

/** What the deletion took away that the tree `root` has again, and any host library beside the door. */
function revived(root = REPO) {
  const lib = join(root, 'hosts', 'lib')
  const beside = existsSync(lib)
    ? readdirSync(lib)
        .filter((name) => !name.startsWith('.') && name !== DOOR)
        .map((name) => `hosts/lib/${name}`)
    : []
  return [...new Set([...DELETED.filter((path) => existsSync(join(root, path))), ...beside])]
}

/** Where the `files` of the tree `root` name a deleted path but where it is said on purpose, each as `file:line`. */
function named(files, root = REPO) {
  const found = []
  for (const file of files) {
    if (file === SELF || SAYS_THEM.some((where) => where.test(file))) continue
    const bytes = readFileSync(join(root, file))
    if (bytes.includes(0)) continue
    const line = bytes
      .toString('utf8')
      .split('\n')
      .findIndex((text) => NAMED.test(text))
    if (line !== -1) found.push(`${file}:${line + 1}`)
  }
  return found
}

describe('Node’s sources are gone for good', () => {
  // What it looks for, shown on text of its own: it finds a module however it
  // is imported, and only the relative ones.
  it('finds a module however it is imported, and only a relative one', () => {
    const text = [
      "import { rosterPath } from '../../src/roster.js'",
      "export * from '../../hosts/lib/windows.js'",
      "const { Bridge } = await import('../../src/bridge.js')",
      "import '../../hosts/lib/quota.js'",
      "import { ask } from '../../hosts/lib/question-door.js'",
      "import { join } from 'node:path'",
      "import { startIntegration } from '../integration/harness.mjs'",
    ].join('\n')
    assert.deepEqual(importsIn(text, 'tests/engine/example.test.mjs'), [
      'src/roster.js',
      'hosts/lib/windows.js',
      'src/bridge.js',
      'hosts/lib/quota.js',
      'hosts/lib/question-door.js',
      'tests/integration/harness.mjs',
    ])
  })

  it('reads the scripts it is to hold', () => {
    const held = scripts()
    assert.ok(held.includes(SELF), 'tests are read')
    assert.ok(
      held.some((file) => file.startsWith('app/tests/')),
      'the page tests are read',
    )
    assert.ok(
      held.includes('hosts/opencode-extension/consensflow-session.mjs'),
      'the host extensions are read',
    )
    const door = importsOf('hosts/opencode-extension/consensflow-session.mjs')
    assert.deepEqual(door, [`hosts/lib/${DOOR}`], 'and the one library they load')
  })

  // A module deleted under a script that still imports it fails there and
  // then, and not when someone runs the tool.
  it('has every script import files that are there', () => {
    assert.deepEqual(dangling(), [])
  })

  it('holds the deleted tree deleted', () => {
    assert.deepEqual(revived(), [], 'Node’s sources are gone: use the native side')
    assert.ok(existsSync(join(REPO, 'hosts', 'lib', DOOR)), 'the question door stays')
  })

  it('finds a deleted path named, and not a Rust crate’s own or what stays', () => {
    for (const text of [
      '../src/roster.js',
      'src/core/daemon.js',
      'join(REPO, "bin/cf.mjs")',
      'hosts/lib/presets.js',
    ]) {
      assert.match(text, NAMED)
    }
    for (const text of [
      'crates/cf-ledger/src/ledger.rs',
      'crates/cf-daemon/src/screens/pages/agents.html',
      'hosts/lib/question-door.js',
      'hosts/pi-extension/consensflow-delivery.mjs',
    ]) {
      assert.doesNotMatch(text, NAMED)
    }
  })

  // What is read at run time by a path in a string, an import cannot show.
  it('names none of them anywhere but where a recording or an older release says them', () => {
    assert.deepEqual(named(tree()), [], 'Node’s sources are gone: say what they did in words')
  })
})

describe('what the checks are for, planted in a tree of their own', () => {
  /**
   * Runs `check` on a temporary tree of the shape the checks read (a script in
   * each root, the OpenCode plugin and the question door it loads) with the
   * `planted` files besides, or in place of those: the tree, and the files it
   * holds.
   */
  function onTree(planted, check) {
    const files = {
      'tests/example.test.mjs': `import { ask } from '../hosts/lib/${DOOR}'\n`,
      'app/tests/page.spec.mjs': '',
      'app/scripts/build.mjs': '',
      'evals/run.mjs': '',
      'hosts/opencode-extension/consensflow-session.mjs': `import { ask } from '../lib/${DOOR}'\n`,
      [`hosts/lib/${DOOR}`]: 'export const ask = () => {}\n',
      ...planted,
    }
    const root = mkdtempSync(join(tmpdir(), 'cf-off-node-'))
    try {
      for (const [file, text] of Object.entries(files)) {
        mkdirSync(dirname(join(root, file)), { recursive: true })
        writeFileSync(join(root, file), text)
      }
      check(root, Object.keys(files))
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  }

  it('finds nothing in a tree that has none of it', () => {
    onTree({}, (root, files) => {
      assert.deepEqual(dangling(root), [])
      assert.deepEqual(revived(root), [])
      assert.deepEqual(named(files, root), [])
    })
  })

  it('finds a script that imports a file the deletion took away', () => {
    const planted = { 'tests/revived.mjs': "import { rosterPath } from '../src/roster.js'\n" }
    onTree(planted, (root, files) => {
      assert.deepEqual(dangling(root), ['tests/revived.mjs imports src/roster.js'])
      assert.deepEqual(named(files, root), ['tests/revived.mjs:1'])
    })
  })

  it('finds the plugin of OpenCode importing a library that is gone', () => {
    const plugin = 'hosts/opencode-extension/consensflow-session.mjs'
    onTree({ [plugin]: "import { quotaOf } from '../lib/quota.js'\n" }, (root) => {
      assert.deepEqual(dangling(root), [`${plugin} imports hosts/lib/quota.js`])
    })
  })

  // The hole of the dangling-import check: the file is there again, so an import
  // of it resolves.
  it('finds the deleted tree put back, even where an import now finds it', () => {
    const planted = { 'tests/revived.mjs': "import '../src/roster.js'\n", 'src/roster.js': '' }
    onTree(planted, (root) => {
      assert.deepEqual(dangling(root), [])
      assert.deepEqual(revived(root), ['src'])
    })
  })

  it('finds a host library put back, and any other beside the question door', () => {
    const planted = { 'hosts/lib/presets.js': '', 'hosts/lib/newcomer.js': '' }
    onTree(planted, (root) => {
      assert.deepEqual(revived(root), ['hosts/lib/presets.js', 'hosts/lib/newcomer.js'])
    })
  })

  it('finds a deleted path named in a string, where no import shows it', () => {
    const planted = { 'tests/revived.mjs': "export const cf = join(root, 'bin/cf.mjs')\n" }
    onTree(planted, (root, files) => {
      assert.deepEqual(dangling(root), [])
      assert.deepEqual(named(files, root), ['tests/revived.mjs:1'])
    })
  })

  it('leaves a recording that says a deleted path on purpose alone', () => {
    const planted = { 'crates/cf/tests/goldens/cli.json': '{"runs": "$REPO/bin/cf.mjs"}\n' }
    onTree(planted, (root, files) => {
      assert.deepEqual(named(files, root), [])
    })
  })
})
