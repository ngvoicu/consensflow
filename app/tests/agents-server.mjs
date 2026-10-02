import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { agentsUi } from '../../src/core/agents-server.js'
import { Credentials, startApi } from '../../src/core/api.js'
import { openLedger } from '../../src/ledger/index.js'

const INSTALLED = ['claude', 'codex', 'pi', 'opencode', 'devin']

/**
 * The agents screens the way the daemon serves them: Agents, one list of the
 * catalog's agents (with the human's edits on them) and the agents defined by
 * hand, and Harnesses, behind the API, opened with the UI token.
 */
export async function agentsServer(env, { installed = INSTALLED, ...options } = {}) {
  mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
  // The pickers offer only agents on an installed harness: stand-ins on PATH.
  mkdirSync(env.PATH, { recursive: true })
  for (const command of installed) {
    writeFileSync(join(env.PATH, command), '#!/bin/sh\necho 1.2.3\n')
    chmodSync(join(env.PATH, command), 0o755)
  }
  const ledger = openLedger(join(env.CONSENSFLOW_HOME, 'consensflow.db'))
  const token = 'ui-token'
  const server = await startApi({
    ledger,
    credentials: new Credentials(),
    ui: agentsUi(env, { token, ...options }),
  })
  return {
    url: server.url,
    token,
    async close() {
      await server.close()
      ledger.close()
    },
  }
}
