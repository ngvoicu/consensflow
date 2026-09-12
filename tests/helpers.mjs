import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { roleConfiguration } from '../src/role-skills.js'

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
    CONSENSFLOW_BIN_DIR: join(root, 'consensflow', 'bin'),
  }
  return {
    root,
    env,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/** Native config resolution is a subprocess boundary, covered in role-skills.test. */
export const testRoleConfiguration = (kind, options) =>
  roleConfiguration(kind, {
    ...options,
    readInstructions: async () => '',
  })
