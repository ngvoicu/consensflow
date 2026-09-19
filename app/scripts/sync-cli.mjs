#!/usr/bin/env node
/**
 * Mirror this checkout's CLI resources into the built apps and the copy named by
 * the cf launcher. Deleted source files are removed from those bundles too.
 * It never writes into a bundle with the installed release's identity: that is
 * the live app, and its panes run the files this would replace. Point `cf` at
 * the candidate (CONSENSFLOW_HOME=~/.consensflow-candidate) to sync into it.
 * Role context refreshes at the next pane start/resume.
 */
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { terminalRuntime } from '../../src/terminal.js'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const STAGED = join(APP, 'src-tauri', 'resources', 'cli')
const BUILT = join(APP, 'src-tauri', 'target', 'release', 'bundle', 'macos')
const BUNDLES = ['ConsensFlow.app', 'ConsensFlow Candidate.app']
  .map((name) => join(BUILT, name, 'Contents', 'Resources', 'cli'))
  .filter((bundle) => existsSync(dirname(bundle)))

execFileSync(process.execPath, [join(APP, 'scripts', 'prepare-sidecar.mjs')], { stdio: 'inherit' })

if (BUNDLES.length === 0) {
  process.stderr.write(
    `no built app in ${BUILT}\nthere is nothing to sync into — run \`npm run build\` or \`npm run candidate\` first\n`,
  )
  process.exit(1)
}

// Trailing slashes matter to rsync: contents of, into.
const mirror = (into) =>
  execFileSync('rsync', ['-a', '--delete', `${STAGED}/`, `${into}/`], { stdio: 'inherit' })

for (const bundle of BUNDLES) mirror(bundle)
const written = [...BUNDLES]

/**
 * Where `cf` on PATH keeps its CLI. The launcher names the cf.mjs it runs, so
 * the copy is its grandparent — checked rather than assumed, because a path
 * that does not hold a CLI is not one to rsync `--delete` over.
 */
const onPath = () => {
  const runtime = terminalRuntime(process.env)
  if (runtime === null) return null
  if (runtime.live) {
    process.stdout.write(
      `the command on PATH runs the installed ConsensFlow (${runtime.entry}) — not syncing into the live app\n`,
    )
    return null
  }
  const root = dirname(dirname(runtime.entry))
  return existsSync(join(root, 'bin', 'cf.mjs')) ? root : null
}

const launcher = onPath()
if (launcher !== null && !BUNDLES.includes(launcher)) {
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
