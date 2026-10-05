/**
 * Codex's looks, its readiness and the thread it opens on (`observe`, `ready`
 * and `started`, `src/adapters/codex.js`, through `sessionState`,
 * `src/channels/codex.js`): by what the broker of its supervisor says the
 * window shows, and by Codex's own record of the thread. A broker that does
 * not answer is asked last in a scenario here: the recorder keeps the
 * request's timeout armed after a request with no head, where it clears it
 * after any other, and a timer left over would be due with the next sleep.
 */
import {
  answered,
  asked,
  fresh,
  LAUNCH,
  OTHER,
  opened,
  rollout,
  SECOND,
  session,
  settledTurn,
  shows,
  THREAD,
  tokens,
  turnCompleted,
  turnStarted,
} from './codex-scenes.mjs'

/** A broker's answer that names its thread in a list. */
const wrapped = {
  status: 200,
  body: JSON.stringify({ launchId: LAUNCH, sessionId: [THREAD], available: true }),
}
/** An answer of the broker's with `body`, whatever it says. */
const body = (text, status = 200) => ({ 'GET /session': [{ status, body: text }] })

export function lookScenarios() {
  const working = rollout(THREAD, [turnStarted('t1'), asked('u1', 'Write the parser')])
  const looked = (steps) => steps.map((answer) => ({ observe: {}, served: answer }))
  return [
    fresh('a window whose thread the broker has not named is looked at as nothing, asking nobody', [
      { observe: {} },
    ]),
    opened('a window that shows its own thread is looked at there', [
      settledTurn(THREAD),
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a window with no messages and nothing in flight is idle and settled', [
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a turn that started and did not end is in flight', [
      working,
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a turn that ended in an error is failed, and settled', [
      rollout(THREAD, [
        turnStarted('t1'),
        asked('u1', 'Write the parser'),
        turnCompleted('t1', null, { error: 'boom' }),
      ]),
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('the quota Codex reports passes through', [
      rollout(THREAD, [
        turnStarted('t1'),
        asked('u1', 'Write the parser'),
        answered('a1', 'Done.'),
        turnCompleted('t1', 'Done.'),
        tokens({ primary: { used_percent: 96, resets_at: 1790000000 } }),
      ]),
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a record that cannot be read is settled and empty', [
      rollout(THREAD, ['{not json}\n']),
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a window a /new left on another thread is followed there', [
      settledTurn(THREAD),
      { observe: {}, served: shows(OTHER) },
      rollout(OTHER, [
        turnStarted('t1'),
        asked('u1', 'fresh start'),
        answered('a1', 'Ready'),
        turnCompleted('t1', 'Ready'),
      ]),
      { follow: OTHER },
      { observe: {}, served: shows(OTHER) },
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a switch keeps the old thread’s look, and nothing in it settled', [
      settledTurn(THREAD),
      { observe: {}, served: shows(OTHER, { available: false }) },
    ]),
    opened(
      'a thread said in capitals is not the thread of the window, whose id is in small letters',
      [settledTurn(THREAD), { observe: {}, served: shows(THREAD.toUpperCase()) }],
    ),
    opened('a window in the middle of a switch shows no thread, and its look is unnamed', [
      settledTurn(THREAD),
      { observe: {}, served: shows(null, { available: false }) },
      { observe: {}, served: shows(null) },
    ]),
    opened('what is not the broker’s word on this launch names no thread', [
      settledTurn(THREAD),
      ...looked([
        shows(THREAD, { launchId: SECOND }),
        shows(THREAD, { launchId: '' }),
        body(JSON.stringify({ sessionId: THREAD, available: true })),
        body(JSON.stringify({ launchId: 7, sessionId: THREAD })),
        body(JSON.stringify({ launchId: LAUNCH, available: true })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: 'not-a-uuid', available: true })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: `${THREAD}\n`, available: true })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD.replace('-469f-', '-969f-') })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD.replace('-a165-', '-c165-') })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: 7 })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: [THREAD, null] })),
        body('', 200),
        body('', 204),
        body('not json'),
        body('null'),
        body('[]'),
        body('"text"'),
        body('{"launchId":'),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD }), 500),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD }), 300),
      ]),
    ]),
    opened(
      'a thread in a list is read as the thread by JavaScript’s test, and is no thread of the window',
      [settledTurn(THREAD), { observe: {}, served: { 'GET /session': [wrapped] } }],
    ),
    opened('an answer that begins with a byte order mark is read, as a web reply is', [
      settledTurn(THREAD),
      {
        observe: {},
        served: body(`﻿${JSON.stringify({ launchId: LAUNCH, sessionId: THREAD, available: true })}`),
      },
    ]),
    opened('an answer that is not exactly true is no word that the window is available', [
      settledTurn(THREAD),
      {
        observe: {},
        served: body(JSON.stringify({ launchId: LAUNCH, sessionId: OTHER, available: 1 })),
      },
    ]),
    opened('a broker that answers too late is no word: the request has a second, with its body', [
      settledTurn(THREAD),
      { observe: {}, served: { 'GET /session': [{ held: true }] } },
      { advance: 1000 },
      { observe: {}, served: { 'GET /session': [{ status: 200, body: { held: true } }] } },
      { advance: 999 },
      { advance: 1 },
    ]),
    opened('a head held is answered in time, and its body held is ended in time', [
      settledTurn(THREAD),
      { observe: {}, served: { 'GET /session': [{ held: true }] } },
      { advance: 400 },
      {
        release: 'GET /session',
        answer: { status: 200, body: { held: true } },
      },
      { advance: 400 },
      {
        releaseBody: 'GET /session',
        body: JSON.stringify({ launchId: LAUNCH, sessionId: OTHER, available: true }),
      },
    ]),
    opened('a body the broker broke off is no word', [
      settledTurn(THREAD),
      { observe: {}, served: { 'GET /session': [{ status: 200, body: { cut: true } }] } },
      { observe: {}, served: { 'GET /session': [{ status: 200, body: { held: true } }] } },
      { releaseBody: 'GET /session', body: { cut: true } },
    ]),
    opened('a look reads what the broker says when it starts, and the record when it is released', [
      settledTurn(THREAD),
      { holdLooks: true },
      { observe: {}, served: shows(THREAD) },
      { holdLooks: false },
      working,
      { release: 'look' },
      { observe: {}, served: shows(THREAD) },
    ]),
    opened('a window followed while its look is held is compared to where it was followed', [
      settledTurn(THREAD),
      { holdLooks: true },
      { observe: {}, served: shows(OTHER) },
      { holdLooks: false },
      { follow: OTHER },
      { release: 'look' },
    ]),
    opened('a broker that does not answer leaves the look unnamed', [
      settledTurn(THREAD),
      { observe: {} },
    ]),
  ]
}

export function readyScenarios() {
  const answers = (...words) => words.map((word) => ({ ready: {}, served: word }))
  return [
    opened('a window is ready once the broker says it can take a message, on its own thread', [
      ...answers(shows(THREAD, { available: false }), shows(THREAD), shows(OTHER), shows(null)),
      { follow: OTHER },
      ...answers(shows(OTHER), shows(THREAD)),
    ]),
    fresh(
      'a window whose thread is not named yet is ready when the broker shows none and can take one',
      [...answers(shows(null), shows(THREAD), shows(null, { available: false }))],
    ),
    opened('what is not the broker’s word on this launch holds the message', [
      ...answers(
        shows(THREAD, { launchId: SECOND }),
        body('not json'),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD, available: true }), 500),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: 'not-a-uuid', available: true })),
        body('null'),
        body('', 204),
      ),
    ]),
    opened('a thread in a list holds the message as another conversation’s', [
      { ready: {}, served: { 'GET /session': [wrapped] } },
    ]),
    opened('an answer that is available in anything but true holds the message', [
      ...answers(
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD, available: 1 })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD, available: 'true' })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD, available: [true] })),
        body(JSON.stringify({ launchId: LAUNCH, sessionId: THREAD })),
      ),
    ]),
    opened('a broker that answers too late holds the message', [
      { ready: {}, served: { 'GET /session': [{ held: true }] } },
      { advance: 1000 },
    ]),
    opened('a broker that does not answer holds the message', [{ ready: {} }]),
  ]
}

/** A thread named at the last poll before the minute is up, and none in the minute. */
export function startedScenarios() {
  const unnamed = session(null, { available: false })
  const polls = 240
  return [
    fresh('a thread named at the last poll before a minute is up is the thread', [
      {
        started: {},
        served: { 'GET /session': [...Array(polls - 1).fill(unnamed), session(THREAD)] },
        optional: ['GET /session'],
      },
      { advance: 250 * (polls - 1) },
    ]),
    fresh('a thread never named in a minute is an error', [
      {
        started: {},
        served: { 'GET /session': Array(polls).fill(unnamed) },
        optional: ['GET /session'],
      },
      { advance: 250 * polls },
    ]),
    fresh('a thread named while the broker cannot take a message yet is the thread', [
      { started: {}, served: { 'GET /session': [session(OTHER, { available: false })] } },
    ]),
    fresh('answers that name no thread of this launch are waited out', [
      {
        started: {},
        served: {
          'GET /session': [
            session('not-a-uuid'),
            session(THREAD, { launchId: SECOND }),
            wrapped,
            { status: 500, body: JSON.stringify({ launchId: LAUNCH, sessionId: THREAD }) },
            { status: 200, body: 'not json' },
            session(THREAD),
          ],
        },
        optional: ['GET /session'],
      },
      ...Array(4).fill({ advance: 250, optional: ['GET /session'] }),
      { advance: 250 },
    ]),
    fresh('a thread named is the thread of the window from then on', [
      { started: {}, served: shows(THREAD) },
      { observe: {}, served: shows(THREAD) },
      { ready: {}, served: shows(THREAD) },
      { started: {} },
    ]),
  ]
}
