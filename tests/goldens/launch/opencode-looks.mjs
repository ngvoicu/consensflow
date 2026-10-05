/**
 * OpenCode's looks and its readiness (`observe` and `ready`,
 * `src/adapters/opencode.js`): by what the plugin inside the window says it
 * shows (`sessionState`, `src/channels/opencode.js`, `GET /session` on the
 * plugin's port) and what OpenCode says it is doing. The window's own record
 * is not there (OpenCode's store is read where `records` hold it), so every
 * look reads an unknown record: settled, with nothing in it.
 */
import { KEPT, LAUNCH, NOW, OTHER, resumed, shows } from './opencode-scenes.mjs'

const STATE = 'GET /session'

/** The plugin is asked, and answers as `answer` says. */
const asked = (answer, step = {}) => ({ observe: {}, served: { [STATE]: [answer] }, ...step })

/** A conversation with OpenCode's own word of what it is doing. */
const busy = shows(KEPT, { type: 'busy' })
const retry = (fields) => shows(KEPT, { type: 'retry', attempt: 1, ...fields })

export function lookScenarios() {
  const hour = 3_600_000
  return [
    resumed('a plugin that does not answer leaves the window unnamed, and its message waiting', [
      { observe: {} },
    ]),
    resumed('a plugin that answers nobody in a second is given up on', [
      asked({ held: true }),
      { advance: 1000 },
    ]),
    resumed('a plugin whose body is held is given up on at a second', [
      asked({ status: 200, body: { held: true } }),
      { advance: 1000 },
    ]),
    resumed('a plugin whose body is released in time is read', [
      asked({ status: 200, body: { held: true } }),
      { advance: 999 },
      { releaseBody: STATE, body: JSON.stringify({ launchId: LAUNCH, sessionId: KEPT }) },
    ]),
    resumed('a plugin whose head is released in time is read', [
      asked({ held: true }),
      { advance: 500 },
      { release: STATE, answer: shows(KEPT) },
    ]),
    resumed('a plugin that answers with a redirect says nothing', [
      asked({
        status: 300,
        body: JSON.stringify({ launchId: LAUNCH, sessionId: KEPT, status: null }),
      }),
    ]),
    resumed('a window showing no conversation is on its home screen or session list', [
      asked(shows(null)),
    ]),
    resumed('a window that shows its own conversation is looked at there', [
      asked(shows(KEPT)),
      asked(shows(KEPT, null)),
    ]),
    resumed('what the plugin says that is no word of this window is no word', [
      asked({ status: 500, body: JSON.stringify({ launchId: LAUNCH, sessionId: KEPT }) }),
      asked({ status: 200, body: 'not json' }),
      asked({ status: 200, body: '' }),
      asked({ status: 204 }),
      asked({ status: 200, body: 'null' }),
      asked({ status: 200, body: '[]' }),
      asked(shows(KEPT, null, 'another-launch')),
      asked({ status: 200, body: JSON.stringify({ sessionId: KEPT }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: 'bad' }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: '' }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: 'ses_' }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: 'ses_a-b' }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: 5 }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: { id: KEPT } }) }),
      asked({ status: 200, body: JSON.stringify({ launchId: [LAUNCH], sessionId: KEPT }) }),
      asked({ noHead: true }),
    ]),
    resumed('a plugin that begins its answer with a byte order mark is read', [
      asked({
        status: 200,
        body: `﻿${JSON.stringify({ launchId: LAUNCH, sessionId: KEPT })}`,
      }),
    ]),
    resumed('a conversation id the plugin sends as a list is another conversation’s', [
      asked(
        { status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: [KEPT] }) },
        {
          kept: {
            why: 'a list is the text the id pattern reads it as; Node names the conversation by the list itself, which Rust cannot, and names the text',
            answer: {
              answer: {
                items: [],
                settled: false,
                waiting: null,
                failed: false,
                quota: null,
                switched: { nativeSession: KEPT },
              },
            },
          },
        },
      ),
    ]),
    resumed('what OpenCode says it is doing: busy, idle, nothing, and what is no status', [
      asked(busy),
      asked(shows(KEPT, { type: 'idle' })),
      asked(shows(KEPT, null)),
      asked(shows(KEPT, 'busy')),
      asked(shows(KEPT, ['retry'])),
      asked(shows(KEPT, { type: ['retry'] })),
    ]),
    resumed('a spent free tier is a quota with the reset it names', [
      asked(
        retry({
          message: 'Free usage exceeded, subscribe to Go',
          action: { reason: 'free_tier_limit' },
          next: NOW + 5 * hour,
        }),
      ),
    ]),
    resumed('a retry in seconds is backoff, and the window is at work', [
      asked(retry({ message: 'Rate limit exceeded. Please try again later.', next: NOW + 4_000 })),
      asked(retry({ message: 'Rate limit exceeded', next: NOW + 59_999 })),
      asked(retry({ message: 'Provider is overloaded', next: NOW + 4_000 })),
    ]),
    resumed('a retry a minute or more away is a limit waited out', [
      asked(retry({ message: 'Rate limit exceeded. Please try again later.', next: NOW + 60_000 })),
      asked(retry({ message: 'Usage limit reached', next: NOW + 20 * 60_000 })),
      asked(retry({ message: 'Provider is overloaded', next: NOW + 20 * 60_000 })),
    ]),
    resumed('a retry with no time, or a time that is no number, is a quota only by its words', [
      asked(retry({ message: 'Too Many Requests' })),
      asked(retry({ message: 'quota spent', next: 'soon' })),
      asked(retry({ message: 'x', action: { reason: 'QUOTA spent' } })),
      asked(retry({ message: 'rate limit hit', next: String(NOW + hour) })),
      asked(retry({ message: ['a limit'], next: NOW + hour })),
      asked(retry({ message: 'x', next: NOW + hour })),
    ]),
    resumed('a retry whose reset no date holds fails the look', [
      asked({
        status: 200,
        body: `{"launchId":"${LAUNCH}","sessionId":"${KEPT}","status":{"type":"retry","message":"limit","next":1e300}}`,
      }),
    ]),
    resumed('what another conversation’s status says is no word about this one', [
      asked(shows(OTHER, { type: 'retry', message: 'Rate limit exceeded', next: NOW + hour })),
    ]),
    resumed('a window the human switched is followed to the conversation it shows', [
      asked(shows(OTHER)),
      { follow: OTHER },
      asked(shows(OTHER, { type: 'idle' })),
      asked(shows(KEPT)),
    ]),
    resumed('a look waits for the record it reads, which is read after the plugin was heard', [
      { holdLooks: true },
      asked(shows(KEPT)),
      { release: 'look' },
    ]),
  ]
}

export function readyScenarios() {
  const ready = (answer) => ({ ready: {}, served: { [STATE]: [answer] } })
  return [
    resumed('a message waits until the plugin answers, and the window shows its conversation', [
      { ready: {} },
      ready(shows(null)),
      ready(shows(KEPT)),
      ready(shows(OTHER)),
      ready({ status: 500, body: '{}' }),
    ]),
    resumed('a window followed to another conversation is ready there', [
      { follow: OTHER },
      ready(shows(OTHER)),
      ready(shows(KEPT)),
    ]),
    resumed('a conversation id sent as a list is not the window’s', [
      ready({ status: 200, body: JSON.stringify({ launchId: LAUNCH, sessionId: [KEPT] }) }),
    ]),
    resumed('a plugin that answers late is waited for no longer than a second', [
      ready({ held: true }),
      { advance: 1000 },
    ]),
  ]
}
