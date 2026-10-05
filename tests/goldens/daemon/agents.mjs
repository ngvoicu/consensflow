/**
 * The agents screens, recorded: `agentsUi` reports the environment whose
 * roster and harnesses it reads, the UI token it was given, and each request
 * it answered (one it does not own falls through to the API, and is that
 * exchange's own).
 */
import * as real from '../../../src/core/agents-server.js'
import * as session from './session.mjs'

export * from '../../../src/core/agents-server.js'

export function agentsUi(env, options = {}) {
  session.environment(env)
  session.uiMounted(options.token)
  const ui = real.agentsUi(env, options)
  return {
    ...ui,
    async handle(request, url) {
      const answered = await ui.handle(request, url)
      if (answered !== null) session.screened()
      return answered
    },
  }
}
