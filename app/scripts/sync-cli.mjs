#!/usr/bin/env node
/**
 * Mirror this checkout's CLI resources into the built app and the copy named by
 * the cf launcher. Deleted source files are removed from those bundles too.
 * This is a development command that can modify the installed app and break its
 * resource signature. Role context refreshes at the next pane start/resume.
 */
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { terminalRuntime } from '../../src/terminal.js'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const STAGED = join(APP, 'src-tauri', 'resources', 'cli')
const BUNDLE = join(
  APP,
  'src-tauri',
  'target',
  'release',
  'bundle',
  'macos',
  'ConsensFlow.app',
  'Contents',
  'Resources',
  'cli',
)

execFileSync(process.execPath, [join(APP, 'scripts', 'prepare-sidecar.mjs')], { stdio: 'inherit' })

if (!existsSync(dirname(BUNDLE))) {
  process.stderr.write(
    `no built app at ${BUNDLE}\nthere is nothing to sync into — run \`npm run build\` first\n`,
  )
  process.exit(1)
}

// Trailing slashes matter to rsync: contents of, into.
const mirror = (into) =>
  execFileSync('rsync', ['-a', '--delete', `${STAGED}/`, `${into}/`], { stdio: 'inherit' })

mirror(BUNDLE)
const written = [BUNDLE]

/**
 * Where `cf` on PATH keeps its CLI. The launcher names the cf.mjs it runs, so
 * the copy is its grandparent — checked rather than assumed, because a path
 * that does not hold a CLI is not one to rsync `--delete` over.
 */
const onPath = () => {
  const entry = terminalRuntime(process.env)?.entry
  if (entry === undefined) return null
  const root = dirname(dirname(entry))
  return existsSync(join(root, 'bin', 'cf.mjs')) ? root : null
}

const launcher = onPath()
if (launcher !== null && launcher !== BUNDLE) {
  // Say it BEFORE writing: this is the line that would have saved the day,
  // and a seal broken silently is how a `codesign --verify` failure gets
  // discovered by somebody else's Mac.
  process.stdout.write(`the command on PATH runs another copy — syncing that one too\n`)
  mirror(launcher)
  written.push(launcher)
  const sealed = launcher.includes('.app/Contents/')
  if (sealed) {
    process.stdout.write(
      'that bundle is signed: mirroring into it breaks `codesign --verify`.\n' +
        'It still runs; `npm run build` is what puts a real signature back.\n',
    )
  }
}

process.stdout.write(
  `${written.map((path) => `cli → ${path}`).join('\n')}\n` +
    'updated role instructions load when a pane starts or resumes\n',
)
