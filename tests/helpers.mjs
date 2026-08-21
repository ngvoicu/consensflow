import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * Every test runs against a throwaway CONSENSFLOW_HOME and throwaway harness
 * homes. The env object is passed explicitly to every module call — modules
 * never read process.env on their own, which is what makes this guard airtight.
 */
export function tempEnv() {
  const root = mkdtempSync(join(tmpdir(), 'cfv3-'))
  const env = {
    HOME: join(root, 'home'),
    CONSENSFLOW_HOME: join(root, 'consensflow'),
    CLAUDE_CONFIG_DIR: join(root, 'home', '.claude'),
    CODEX_HOME: join(root, 'home', '.codex'),
    XDG_CONFIG_HOME: join(root, 'home', '.config'),
    PATH: join(root, 'bin'),
    CONSENSFLOW_BIN_DIR: join(root, 'user-bin'),
  }
  return {
    root,
    env,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/** Fails a test that produced a path under the real home directory. */
export function assertOutsideRealHome(path) {
  const real = homedir()
  if (path === real || path.startsWith(`${real}/`)) {
    throw new Error(`test touched the real home directory: ${path}`)
  }
}

/**
 * Declares the precondition the generated skill needs: a machine that has
 * chosen the cmux path. Nothing installs before that choice — writing the
 * mode file is exactly what `applyMode` records, without dragging a host
 * payload and a git stub into a test about installing skills.
 */
export function chooseCmuxMode(t) {
  const root = t.env.CONSENSFLOW_HOME
  mkdirSync(root, { recursive: true })
  writeFileSync(
    join(root, 'mode.json'),
    `${JSON.stringify({ mode: 'cmux', at: '2026-08-21T00:00:00.000Z' }, null, 2)}\n`,
  )
}
