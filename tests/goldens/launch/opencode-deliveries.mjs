/**
 * OpenCode's deliveries (`deliver`, `src/adapters/opencode.js`, through
 * `send`, `src/channels/opencode.js`): a claim of the pane, then a request
 * to the plugin inside the window (`POST /deliver` on its port), which says
 * whether OpenCode took the message. A message is handed over in 3 s at
 * most, counted from the moment it is sent.
 */
import { claim, claimed, MESSY, NOW, OTHER, resumed } from './opencode-scenes.mjs'

const DELIVER = 'POST /deliver'

/** The plugin answers a delivery with `body`, as JSON, under `status`. */
const says = (body, status = 200) => ({ status, body: JSON.stringify(body) })
const admitted = says({ ok: true, admitted: true })

/** A message delivered, the pane claimed and the plugin answering as `answer` says. */
const delivery = (text, answer, step = {}) => ({
  deliver: text,
  answers: claimed,
  served: { [DELIVER]: [answer] },
  ...step,
})

export function deliveryScenarios() {
  return [
    resumed('a message goes through the plugin: a claim, then the request', [
      delivery('hi', admitted),
    ]),
    resumed('a message is written as a window takes text', [delivery(MESSY, admitted)]),
    resumed('a message is sent to the conversation the window was followed to', [
      { follow: OTHER },
      delivery('to the new conversation', admitted),
    ]),
    resumed('a message the plugin refuses before it sends is refused, in its words', [
      delivery(
        'for another conversation',
        says({ ok: false, admitted: false, bytesWritten: 0, error: 'native-session-changed' }),
      ),
      delivery(
        'the status says no and the word is the same',
        says({ admitted: false, bytesWritten: 0, error: 'expired' }, 500),
      ),
      delivery(
        'written zero as the last is',
        says({ admitted: false, bytesWritten: -0, error: 'x' }),
      ),
    ]),
    resumed('an answer that is not a refusal before the send leaves the message uncertain', [
      delivery('bytes were written', says({ admitted: false, bytesWritten: 3, error: 'late' })),
      delivery('no word of its bytes', says({ admitted: false, error: 'late' })),
      delivery('bytes of text', says({ admitted: false, bytesWritten: '0', error: 'late' })),
      delivery('an error that is no text', says({ admitted: false, bytesWritten: 0, error: 7 })),
      delivery('no error', says({ admitted: false, bytesWritten: 0 })),
      delivery('not yet', says({ ok: true, admitted: null })),
      delivery('not a word of it', says({ ok: true })),
      delivery('ok and not admitted', says({ ok: false, admitted: true })),
      delivery(
        'admitted under a status that is no success',
        says({ ok: true, admitted: true }, 500),
      ),
      delivery('admitted under a redirect', says({ ok: true, admitted: true }, 300)),
      delivery('admitted as a string', says({ ok: true, admitted: 'true' })),
    ]),
    resumed('an answer that is no JSON, or no object, leaves the message uncertain', [
      delivery('not json', { status: 200, body: 'not json' }),
      delivery('nothing said', { status: 200, body: '' }),
      delivery('no content', { status: 204 }),
      delivery('null', { status: 200, body: 'null' }),
      delivery('a list', { status: 200, body: '[]' }),
      delivery('a text', { status: 200, body: '"ok"' }),
      delivery('a byte order mark first', {
        status: 200,
        body: `﻿${JSON.stringify({ ok: true, admitted: true })}`,
      }),
    ]),
    resumed('a plugin that cannot be reached leaves the message uncertain', [
      delivery('nobody home', { noHead: true }),
    ]),
    resumed('a plugin that does not answer in three seconds leaves the message uncertain', [
      delivery('slow', { held: true }),
      { advance: 3000 },
    ]),
    resumed('a plugin whose body does not end in three seconds leaves the message uncertain', [
      delivery('slower', { status: 200, body: { held: true } }),
      { advance: 3000 },
    ]),
    resumed('an answer released in time is the plugin’s own', [
      delivery('in time', { held: true }),
      { advance: 2999 },
      { release: DELIVER, answer: admitted },
    ]),
    resumed('a body released in time is read', [
      delivery('in time', { status: 200, body: { held: true } }),
      { advance: 2999 },
      { releaseBody: DELIVER, body: JSON.stringify({ ok: true, admitted: true }) },
    ]),
    resumed('a claim the host refuses sends nothing, in its words', [
      {
        deliver: 'stale',
        answers: claim({ ok: false, admitted: false, error: 'stale-generation', cause: 'gone' }),
      },
      { deliver: 'busy', answers: claim({ ok: false, error: 'paste-in-flight' }) },
      { deliver: 'no word', answers: claim({ ok: false }) },
      { deliver: 'not a verdict', answers: claim({ ok: 1 }) },
      { deliver: 'no answer', answers: claim(null) },
      { deliver: 'empty words', answers: claim({ ok: false, cause: '', error: 'x' }) },
      { deliver: 'null words', answers: claim({ ok: false, cause: null, error: 'later' }) },
    ]),
    resumed('a claim that is not words is written as a template writes it', [
      {
        deliver: 'a number',
        answers: claim({ ok: false, admitted: false, cause: 0 }),
        kept: {
          why: "the host's cause is its words, and a reason is text; Node passed any other value on as the reason itself",
          answer: { answer: { admitted: false, reason: '0' } },
        },
      },
    ]),
    resumed('a claim the host never answered sends nothing, in its words', [
      { deliver: 'bridge down', answers: claim({ throws: 'bridge ended', error: 'eof' }) },
      { deliver: 'no word', answers: claim({ throws: 'bridge ended' }) },
    ]),
    resumed('a claim held keeps the message waiting until it is answered', [
      {
        deliver: 'wait for the claim',
        answers: claim({ held: true }),
        served: { [DELIVER]: [admitted] },
      },
      { advance: 2000 },
      { release: 'pane.claim', answer: { ok: true } },
    ]),
    resumed('a claim answered after the time is up is refused as expired', [
      { deliver: 'too late', answers: claim({ held: true }) },
      { advance: 3000 },
      { release: 'pane.claim', answer: { ok: true } },
    ]),
    resumed('a claim answered with a second left gives the request that second', [
      {
        deliver: 'nearly late',
        answers: claim({ held: true }),
        served: { [DELIVER]: [{ held: true }] },
      },
      { advance: 2000 },
      { release: 'pane.claim', answer: { ok: true } },
      { advance: 1000 },
    ]),
    resumed('a pane that is not a pane is refused before the host is asked', [
      { deliver: 'to no pane', pane: { id: 'p1-zeus', generation: 0 } },
      { deliver: 'to a pane past the safe integers', pane: { id: 'p1-zeus', generation: 2 ** 53 } },
      {
        ...delivery('to the last safe one', admitted),
        pane: { id: 'p1-zeus', generation: 2 ** 53 - 1 },
      },
    ]),
    resumed('the deadline is three seconds from the clock, written in the request', [
      { advance: 1234 },
      delivery('later', admitted),
    ]),
    resumed('two messages one after the other are each claimed and sent', [
      delivery('first', admitted),
      delivery('second', admitted),
    ]),
    resumed('two messages at once are each claimed and sent', [
      {
        deliver: 'first',
        answers: claim({ held: true }),
        served: { [DELIVER]: [admitted, admitted] },
      },
      delivery('second', admitted, { served: {} }),
      { release: 'pane.claim', answer: { ok: true } },
    ]),
    resumed(
      'a window closed while its message waits keeps waiting, its files gone',
      [{ deliver: 'in flight', answers: claim({ held: true }) }, { close: {} }, { advance: 100 }],
      {},
      { pendingAtEnd: [3] },
    ),
    resumed('the clock moved to the second the deadline is written from', [
      { advance: NOW % 1000 },
      delivery('written', admitted),
    ]),
  ]
}
