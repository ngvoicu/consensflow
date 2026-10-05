/**
 * Codex's deliveries (`deliver`, `src/adapters/codex.js`, through `send`,
 * `src/channels/codex.js`): a claim of the pane, then the broker of the
 * supervisor, which takes the text for the thread the window shows within
 * three seconds. A broker that gives no head is the last step of its
 * scenario: the recorder keeps the request's timeout armed after one, and a
 * timer left over would be due with the next.
 */
import {
  claim,
  claimed,
  fresh,
  MESSY,
  OTHER,
  opened,
  replies,
  THREAD,
  takes,
} from './codex-scenes.mjs'

/** The broker's refusal of a message for a thread the window no longer shows. */
const CHANGED = '{"ok":false,"admitted":false,"bytesWritten":0,"error":"native-session-changed"}'

export function deliveryScenarios() {
  const sent = (text, answers, served) => ({ deliver: text, answers, served })
  /** A message the broker answers with `body`. */
  const answered = (what, body, status = 200) => sent(what, claimed, replies(body, status))
  return [
    opened('a message goes through the broker: a claim, then the thread’s queue', [
      sent('hi', claimed, takes),
    ]),
    opened('a message is written as a window takes text', [sent(MESSY, claimed, takes)]),
    opened('a message goes as it is, the space around it kept', [
      sent('  \n a message with space around it \n\n', claimed, takes),
    ]),
    opened('a message goes to the thread the window was followed to', [
      { follow: OTHER },
      sent('to the new thread', claimed, takes),
      { follow: THREAD.toUpperCase() },
      sent('to a thread in capitals', claimed, takes),
    ]),
    opened('a broker that refuses a message for another thread has written nothing', [
      answered('a thread changed under it', CHANGED),
      answered('refused in any status', CHANGED, 409),
      answered(
        'with zero bytes written as a double',
        '{"admitted":false,"bytesWritten":0.0,"error":"x"}',
      ),
      answered(
        'with zero bytes written as minus zero',
        '{"admitted":false,"bytesWritten":-0,"error":"x"}',
      ),
    ]),
    opened('a refusal that is not exactly that leaves the message uncertain', [
      answered('bytes written', '{"admitted":false,"bytesWritten":3,"error":"x"}'),
      answered('bytes in a text', '{"admitted":false,"bytesWritten":"0","error":"x"}'),
      answered('no bytes said', '{"admitted":false,"error":"x"}'),
      answered('an error that is no text', '{"admitted":false,"bytesWritten":0,"error":7}'),
      answered('no error', '{"admitted":false,"bytesWritten":0}'),
      answered('a refusal that is not false', '{"admitted":null,"bytesWritten":0,"error":"x"}'),
    ]),
    opened('a verdict that is not exactly a yes leaves the message uncertain', [
      answered('ok but not admitted', '{"ok":true,"admitted":false}'),
      answered('admitted but not ok', '{"ok":false,"admitted":true}'),
      answered('admitted in a text', '{"ok":true,"admitted":"yes"}'),
      answered('a yes in a failing status', '{"ok":true,"admitted":true}', 500),
      answered('a yes in a status that is none', '{"ok":true,"admitted":true}', 300),
      answered('a yes in any success status', '{"ok":true,"admitted":true}', 202),
    ]),
    opened('a reply that is no verdict leaves the message uncertain', [
      answered('an empty object', '{}'),
      answered('a list', '[]'),
      answered('a number', '7'),
      answered('a text', '"admitted"'),
      answered('a flag', 'true'),
    ]),
    opened('a reply that is no JSON, or null, is a failure of the transport', [
      answered('null', 'null'),
      answered('nothing', ''),
      answered('words', 'not json'),
      answered('half a document', '{"ok":'),
      sent('no content', claimed, { 'POST /deliver': [{ status: 204 }] }),
    ]),
    opened('a verdict that begins with a byte order mark is read, as a web reply is', [
      answered('a bom', `﻿${'{"ok":true,"admitted":true}'}`),
    ]),
    opened('a body the broker broke off, or never ended, is a failure of the transport', [
      sent('cut', claimed, { 'POST /deliver': [{ status: 200, body: { cut: true } }] }),
      sent('held', claimed, { 'POST /deliver': [{ status: 200, body: { held: true } }] }),
      { advance: 3000 },
    ]),
    opened('a broker that answers within the deadline, head and body, is heard', [
      sent('slow broker', claimed, { 'POST /deliver': [{ held: true }] }),
      { advance: 1200 },
      {
        release: 'POST /deliver',
        answer: { status: 200, body: { held: true } },
      },
      { advance: 1000 },
      { releaseBody: 'POST /deliver', body: '{"ok":true,"admitted":true}' },
    ]),
    opened('a broker that gives no head by the deadline is a failure of the transport', [
      sent('too slow', claimed, { 'POST /deliver': [{ held: true }] }),
      { advance: 2999 },
      { advance: 1 },
    ]),
    opened('a claim the host refuses writes nothing, and says so in its words', [
      {
        deliver: 'stale',
        answers: claim({ ok: false, admitted: false, error: 'stale-generation', cause: 'gone' }),
      },
      { deliver: 'busy', answers: claim({ ok: false, error: 'paste-in-flight' }) },
      { deliver: 'no word', answers: claim({ ok: false }) },
      { deliver: 'not a verdict', answers: claim({ ok: 1 }) },
      { deliver: 'no answer', answers: claim(null) },
      { deliver: 'a word', answers: claim('stale') },
      { deliver: 'a list', answers: claim([]) },
      { deliver: 'empty words', answers: claim({ ok: false, cause: '', error: 'x' }) },
      { deliver: 'null words', answers: claim({ ok: false, cause: null, error: 'later' }) },
    ]),
    opened('a claim that is not words is written as a template writes it', [
      {
        deliver: 'a number',
        answers: claim({ ok: false, admitted: false, cause: 0 }),
        kept: {
          why: 'the host’s cause is its words, and a reason is text; Node passed any other value on as the reason itself',
          answer: { answer: { admitted: false, reason: '0' } },
        },
      },
    ]),
    opened('a claim the host never answered writes nothing, in its words', [
      { deliver: 'bridge down', answers: claim({ throws: 'bridge ended', error: 'eof' }) },
      { deliver: 'no word', answers: claim({ throws: 'bridge ended' }) },
    ]),
    opened('a claim held leaves the broker to be asked with the time that is left', [
      {
        deliver: 'wait for the claim',
        answers: claim({ held: true }),
        served: { 'POST /deliver': [{ held: true }] },
        optional: ['POST /deliver'],
      },
      { advance: 1000, optional: ['POST /deliver'] },
      { release: 'pane.claim', answer: { ok: true } },
      { release: 'POST /deliver', answer: { status: 200, body: '{"ok":true,"admitted":true}' } },
    ]),
    opened('a claim held until the last millisecond leaves one for the broker', [
      {
        deliver: 'just in time',
        answers: claim({ held: true }),
        served: { 'POST /deliver': [{ held: true }] },
        optional: ['POST /deliver'],
      },
      { advance: 2999, optional: ['POST /deliver'] },
      { release: 'pane.claim', answer: { ok: true } },
      { advance: 1 },
    ]),
    opened('a claim held past the deadline leaves the message unsent, expired', [
      { deliver: 'too late', answers: claim({ held: true }) },
      { advance: 3000 },
      { release: 'pane.claim', answer: { ok: true } },
    ]),
    opened('a claim refused after it was held writes nothing', [
      { deliver: 'wait for the claim', answers: claim({ held: true }) },
      { release: 'pane.claim', answer: { ok: false, admitted: false, error: 'stale' } },
    ]),
    opened('a pane the host cannot name is refused before the host is asked', [
      { deliver: 'to no pane', pane: { id: 'p1-zeus', generation: 0 } },
      { deliver: 'to a pane with no id', pane: { id: '', generation: 1 } },
      { deliver: 'to a pane past the safe integers', pane: { id: 'p1-zeus', generation: 2 ** 53 } },
      {
        deliver: 'to the last safe one',
        pane: { id: 'p1-zeus', generation: 2 ** 53 - 1 },
        answers: claimed,
        served: takes,
      },
    ]),
    opened('a window followed to a thread that is no id is sent nothing', [
      { follow: '' },
      { deliver: 'x' },
      { follow: 'not-a-uuid' },
      { deliver: 'x' },
      { follow: `${THREAD}\n` },
      { deliver: 'x' },
      { follow: THREAD.replace('-469f-', '-969f-') },
      { deliver: 'x' },
    ]),
    fresh('a window whose thread the broker has not named is sent nothing', [
      { deliver: 'x' },
      { deliver: 'to no pane', pane: { id: '', generation: 0 } },
    ]),
    opened('two messages one after the other are each given the time they have', [
      sent('first', claimed, takes),
      { advance: 700 },
      sent('second', claimed, takes),
    ]),
    opened('a window closed while its message waits keeps waiting, and the broker is heard', [
      { deliver: 'in flight', answers: claimed, served: { 'POST /deliver': [{ held: true }] } },
      { close: {} },
      { release: 'POST /deliver', answer: { status: 200, body: '{"ok":true,"admitted":true}' } },
    ]),
    opened('a broker that gives no head is a failure of the transport', [
      sent('nobody home', claimed),
    ]),
  ]
}
