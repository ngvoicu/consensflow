/**
 * Pi's looks and its readiness (`observe` and `ready`, `src/adapters/pi.js`):
 * by the conversation the extension says the window shows, and by Pi's own
 * record of it.
 */
import {
  assistant,
  DRAWN,
  header,
  LAUNCH,
  OTHER,
  opened,
  record,
  SECOND,
  SETTLED,
  SHOWN,
  settled,
  shows,
  transcript,
  user,
  working,
} from './pi-scenes.mjs'

/** A look Rust answers as a window that has not named its conversation, where Node answered otherwise, and why. */
const unnamed = (why) => ({
  why,
  answer: {
    answer: {
      items: [],
      settled: true,
      waiting: { reason: 'Pi has not said yet which conversation its window shows' },
      failed: false,
      quota: null,
      unnamed: true,
    },
  },
})

export function lookScenarios() {
  return [
    opened('a window that has not said which conversation it shows is unnamed, and waits', [
      { observe: {} },
    ]),
    opened('a window that shows its own conversation is looked at there', [
      record,
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a window with no messages and nothing in flight is idle and settled', [
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a turn the extension saw start and not settle is in flight', [
      working(DRAWN),
      shows(DRAWN),
      { observe: {} },
    ]),
    opened("the extension's settled marker settles the turn it names", [
      record,
      shows(DRAWN),
      { observe: {} },
      settled(DRAWN, 'a1'),
      { observe: {} },
      settled(DRAWN, 'a0'),
      { observe: {} },
    ]),
    opened('a record that cannot be read is settled and empty', [
      transcript(DRAWN, ['{not json}\n']),
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a refused request is exhausted quota with the failure Pi recorded', [
      transcript(DRAWN, [
        header(DRAWN),
        user('u1', 'Write the parser'),
        assistant('a1', 'error', '', {
          errorMessage: '429: usage limit reached, resets in 2 hours',
          timestamp: 1789819201000,
        }),
      ]),
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a window a /new left on another conversation is followed there', [
      record,
      shows(OTHER),
      { observe: {} },
      transcript(OTHER, [
        header(OTHER),
        user('u1', 'fresh start'),
        assistant('a1', 'stop', 'Ready'),
      ]),
      { follow: OTHER },
      { observe: {} },
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a switch keeps the old conversation, and nothing in it settled', [
      record,
      settled(DRAWN, 'a1'),
      shows(OTHER),
      { observe: {} },
    ]),
    opened('a window that shows an empty name is on another conversation than its own', [
      shows(''),
      { observe: {} },
    ]),
    opened('a file that is no JSON, or no object with a launch and a text, names no conversation', [
      { write: SHOWN, text: '{"launchId": ' },
      { observe: {} },
      { write: SHOWN, text: '' },
      { observe: {} },
      { write: SHOWN, text: 'null' },
      { observe: {} },
      { write: SHOWN, text: '[]' },
      { observe: {} },
      { write: SHOWN, text: `"${DRAWN}"` },
      { observe: {} },
      { write: SHOWN, text: '﻿{}' },
      { observe: {} },
      { write: SHOWN, text: `{"launchId":"${LAUNCH}"}` },
      { observe: {} },
      { write: SHOWN, text: `{"launchId":"${LAUNCH}","sessionId":7}` },
      { observe: {} },
      { write: SHOWN, text: `{"launchId":7,"sessionId":"${DRAWN}"}` },
      { observe: {} },
    ]),
    opened("another launch's file names no conversation of this one", [
      shows(DRAWN, SECOND),
      { observe: {} },
      shows(DRAWN, ''),
      { observe: {} },
      shows(DRAWN),
      { observe: {} },
    ]),
    opened('a shown file the system refuses to read fails the look', [
      { write: `${SHOWN}/folder`, text: 'x' },
      { observe: {} },
    ]),
    opened('a shown file under a file fails the look too', [
      { write: SETTLED, text: 'x' },
      { observe: {} },
    ]),
    opened(
      'a look reads what the window shows when it starts, and the record when it is released',
      [
        record,
        shows(DRAWN),
        { holdLooks: true },
        { observe: {} },
        { holdLooks: false },
        shows(OTHER),
        settled(DRAWN, 'a1'),
        { release: 'look' },
        { observe: {} },
      ],
    ),
    opened('a window followed while its look is held is compared to where it was followed', [
      record,
      shows(DRAWN),
      { holdLooks: true },
      { observe: {} },
      { holdLooks: false },
      { follow: OTHER },
      { release: 'look' },
    ]),
    opened('a shown file JSON writes past what a double or a depth holds names nothing', [
      {
        write: SHOWN,
        text: `{"launchId":"${LAUNCH}","sessionId":"${DRAWN}","big":1e400}`,
      },
      {
        observe: {},
        kept: unnamed('Node read 1e400 as Infinity; JSON here holds no such number'),
      },
      {
        write: SHOWN,
        text: `{"launchId":"${LAUNCH}","sessionId":"${DRAWN}","deep":${'['.repeat(200)}${']'.repeat(200)}}`,
      },
      {
        observe: {},
        kept: unnamed('Node read JSON at any depth; it is read here 127 levels deep'),
      },
    ]),
    opened('a shown name holding half a surrogate pair is read as Node read it but for that half', [
      { write: SHOWN, text: `{"launchId":"${LAUNCH}","sessionId":"\\ud800"}` },
      {
        observe: {},
        kept: {
          why: 'a Rust text holds no half of a pair: JSON read here writes U+FFFD for it',
          answer: {
            answer: {
              items: [],
              settled: false,
              waiting: null,
              failed: false,
              quota: null,
              switched: { nativeSession: '�' },
            },
          },
        },
      },
    ]),
  ]
}

export function readyScenarios() {
  return [
    opened('a window is ready once the extension names its own conversation', [
      { ready: {} },
      shows(OTHER),
      { ready: {} },
      shows(DRAWN),
      { ready: {} },
      { follow: OTHER },
      { ready: {} },
      shows(OTHER),
      { ready: {} },
    ]),
    opened('a window that names a conversation of another launch, or none, is not ready', [
      shows(DRAWN, SECOND),
      { ready: {} },
      { write: SHOWN, text: 'not json' },
      { ready: {} },
    ]),
    opened('a shown file the system refuses to read fails the readiness', [
      { write: `${SHOWN}/folder`, text: 'x' },
      { ready: {} },
    ]),
  ]
}
