#!/usr/bin/env node
/**
 * Puts THIS repo's CLI into the .app that is already built.
 *
 * `cf` on PATH runs the bundle's copy, not the repo — that is what makes the
 * app self-contained, and it is also a trap for whoever is developing it. An
 * edit to `bin/ src/ hosts/ skill/` reaches nothing on this machine until the
 * bundle is refreshed, and a skill regenerated in between is written from the
 * OLD template. That cost a day on 2026-08-25: the fix was in the repo, the
 * lead was reading a skill the bundle had written.
 *
 * A full `npm run build` also fixes it and takes a Rust compile. This is the
 * short way round for a CLI-only change: re-stage the resources, mirror them
 * into the bundle, done. Mirror rather than copy — a deleted file has to
 * disappear from the bundle too, which a copy-over would leave behind.
 *
 * It writes to TWO places, because on 2026-09-02 writing to one looked exactly
 * like success: the repo's bundle was refreshed, `cf skills install` printed
 * five `unchanged` lines, and every one of them was correct — the launcher on
 * PATH runs /Applications, which this script had never heard of. The blind
 * spot was the one the paragraph above describes, in the script that exists to
 * fix it. `terminalRuntime` already knew; nothing had asked it.
 *
 * On the seal: the comment here used to claim the bundle is adhoc-signed with
 * no sealed resources, so the signature survives. That is true of a bundle
 * `tauri build` left in this repo and FALSE of one installed from a release
 * DMG, which carries `Sealed Resources version=2` — mirroring into it breaks
 * `codesign --verify` (proven, same day). Nothing here touches the Mach-O
 * binary, so the app still runs; but a sealed bundle comes out unverifiable,
 * and only `npm run build` puts a real signature back. This says so when it
 * happens. Regenerating the installed skill is still separate and deliberate:
 * `cf skills install`.
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
    'the skill is unchanged until you run: cf skills install\n',
)
