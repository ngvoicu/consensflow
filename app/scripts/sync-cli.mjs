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
 * Nothing here touches the Mach-O binary, and the bundle is adhoc-signed with
 * no sealed resources, so the signature is not invalidated. Regenerating the
 * installed skill afterwards is a separate, deliberate step: `cf skills install`.
 */
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

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
execFileSync('rsync', ['-a', '--delete', `${STAGED}/`, `${BUNDLE}/`], { stdio: 'inherit' })

process.stdout.write(`cli → ${BUNDLE}\nthe skill is unchanged until you run: cf skills install\n`)
