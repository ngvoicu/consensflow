/**
 * How `cf` talks to the app (Phase 2, IMPL-PANE-20).
 *
 * Every pane operation is one POST, and the same two rules hold for all of
 * them: the credential decides who you are, and the body must repeat the
 * dimensions that credential was scoped to — a lead its `tab`, a controller
 * its `launch` and `generation`. `src/ui.js` refuses a body that omits them
 * and refuses one that names a different one, so they are added HERE, from
 * the credential, and a caller that tries to name one itself is a bug this
 * throws on rather than sends.
 *
 * This module reads no environment. `bin/cf.mjs` is the entry point and the
 * only place `process.env` is read; what it found arrives here as arguments,
 * which is what makes this testable without a process.
 */

/** The app is not answering where we were told it is. */
export class AppUnreachable extends Error {
  constructor(url, cause) {
    super(
      `ConsensFlow's app is not answering at ${url} — this pane's commands go through it. ` +
        'Is the app still running?',
    )
    this.name = 'AppUnreachable'
    this.url = url
    this.cause = cause
  }
}

/** The app answered, and the answer was no. */
export class AppRefused extends Error {
  constructor(status, body) {
    const code = typeof body?.error === 'string' ? body.error : null
    super(String(body?.reason ?? code ?? `the app refused with ${status}`))
    this.name = 'AppRefused'
    this.status = status
    this.code = code
    this.body = body
  }
}

const DIMENSIONS = ['tab', 'launch', 'generation']

/**
 * @param {object} options
 * @param {string} options.url — where the app is listening.
 * @param {string} [options.token] — the bearer this process was given.
 * @param {string} [options.tab] — a lead's tab, repeated in every body.
 * @param {string} [options.launch] — a controller's launch.
 * @param {number} [options.generation] — and its generation.
 * @param {typeof fetch} [options.fetch] — injected only for tests.
 */
export function appRequester({ url, token, tab, launch, generation, fetch: send = fetch } = {}) {
  const base = String(url ?? '').replace(/\/$/, '')
  if (base.length === 0) throw new Error('the app url is required')

  const scoped = {
    ...(tab === undefined ? {} : { tab }),
    ...(launch === undefined ? {} : { launch }),
    ...(generation === undefined ? {} : { generation }),
  }

  const call = async (path, body, bearer) => {
    let response
    try {
      response = await send(`${base}${path}`, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          ...(bearer === undefined ? {} : { authorization: `Bearer ${bearer}` }),
        },
        body: JSON.stringify(body),
      })
    } catch (cause) {
      throw new AppUnreachable(base, cause)
    }
    const text = await response.text()
    const answer = text.length === 0 ? null : JSON.parse(text)
    if (!response.ok) throw new AppRefused(response.status, answer)
    return answer
  }

  return {
    url: base,
    /**
     * One pane operation, under this credential and its dimensions.
     *
     * `async` so that a caller naming a dimension itself fails the same way
     * a refusal does — one `catch` per verb, never a synchronous throw past
     * the one that handles the app's answer.
     */
    async post(op, body = {}) {
      for (const field of DIMENSIONS) {
        if (Object.hasOwn(body, field)) {
          throw new Error(`the credential owns the ${field}; a request body never names one`)
        }
      }
      return call(`/api/panes/${op}`, { ...scoped, ...body }, token)
    },
    /**
     * A launch ticket for ownership and a controller capability. No bearer:
     * the ticket IS the credential, and it is spent by asking.
     */
    redeem(ticket) {
      return call('/api/launch/redeem', { ticket }, undefined)
    },
  }
}
