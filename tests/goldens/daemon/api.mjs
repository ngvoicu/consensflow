/**
 * The agents' API, recorded: `startApi` reports its address and its close,
 * the kicks it makes and the stand-in roster it asks; `Credentials` reports
 * each token it issues, by the window it is for, and each it revokes. The
 * server inside is the real one (`http.mjs` puts a listener on it).
 */
import * as real from '../../../src/core/api.js'
import * as seam from './seam.mjs'
import * as session from './session.mjs'

export * from '../../../src/core/api.js'

/** The tokens of the windows open now, each reported as it is issued or given back. */
export class Credentials extends real.Credentials {
  issue(window) {
    const token = super.issue(window)
    session.issued(token, window)
    return token
  }

  revoke(token) {
    session.revoked(token)
    super.revoke(token)
  }
}

export async function startApi(options) {
  const api = await real.startApi({
    ...options,
    changed: () => {
      session.kicked()
      options.changed?.()
    },
    ...(options.roster === undefined ? {} : { roster: seam.fn('roster', options.roster) }),
  })
  session.apiStarted(api.url)
  return {
    ...api,
    close() {
      if (session.recording() === null) return api.close()
      const step = { kind: 'api.close', id: session.next('close') }
      const interval = session.begin(step)
      return api.close().finally(() => session.end(interval, { close: step.id }))
    },
  }
}
