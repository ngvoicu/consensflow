import { prepareOpenCodeExtension } from './opencode-install.js'
import { preparePiExtension } from './pi-install.js'
import { installTerminalCommand } from './terminal.js'

/** Opening the standalone app prepares its private launcher and its Pi and OpenCode integrations. */
export function prepareApp(env) {
  const report = []
  try {
    installTerminalCommand(env)
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause)
    report.push(`The cf launcher could not be installed: ${message}`)
  }
  return {
    report,
    piExtension: preparePiExtension(env),
    opencodeExtension: prepareOpenCodeExtension(env),
  }
}
