/**
 * What Devin's windows say besides their wire log, and what is asked of them
 * (`ready`, `deliver` and `observe`, with `src/channels/devin.js`): the record of
 * a conversation Devin kept in its store, whether a window is ready for a
 * paste, and what becomes of a paste.
 */
import {
  answered,
  appended,
  asked,
  conversation,
  ended,
  fresh,
  NOT_YET,
  OTHER,
  questioned,
  replied,
  resumed,
  SESSION,
  said,
  shows,
  snapshot,
  streamed,
  WINDOWS,
  windowsDevin,
  wire,
} from './devin-fixtures.mjs'

/** A look at a window whose conversation Devin kept in its store. */
function records() {
  const settled = [asked(1, 'Write the parser'), replied(2, 'Done.', 1)]
  return [
    resumed('the record of a settled conversation is observed, its items and its end', [
      ...conversation(SESSION, settled),
      wire(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a conversation with a turn Devin has not ended is not settled', [
      ...conversation(SESSION, [
        asked(1, 'Write the parser'),
        replied(2, 'Working', 1, { metadata: {} }),
      ]),
      wire(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a question dialog of its own, still open, is a window waiting', [
      ...conversation(SESSION, [asked(1, 'Write the parser'), questioned(2, 1)]),
      wire(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a question answered is no longer waited on', [
      ...conversation(SESSION, [
        asked(1, 'Write the parser'),
        questioned(2, 1),
        answered(3, 2),
        replied(4, 'Done.', 3),
      ]),
      wire(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a window waiting that shows another conversation is waiting no more', [
      ...conversation(SESSION, [asked(1, 'Write the parser'), questioned(2, 1)]),
      wire(shows(OTHER)),
      { observe: {} },
    ]),
    resumed('a window that has said nothing yet is held, its record as it is', [
      ...conversation(SESSION, settled),
      { observe: {} },
    ]),
    resumed("the wire's quota is the look's, and the record says nothing of it", [
      ...conversation(SESSION, settled),
      wire(shows(SESSION), said('Usage limit reached. Resets in 1 day')),
      { observe: {} },
    ]),
    resumed('a look reads the wire log as it is when it begins, and the record when released', [
      ...conversation(SESSION, settled),
      wire(shows(SESSION)),
      { holdLooks: true },
      { observe: {} },
      { holdLooks: false },
      appended(shows(OTHER)),
      { release: 'look' },
      { observe: {} },
    ]),
    resumed('a turn that ended in an error is a look that failed, settled all the same', [
      ...conversation(SESSION, [
        asked(1, 'Write the parser'),
        replied(2, 'Half of it', 1, { metadata: {} }),
      ]),
      wire(shows(SESSION), streamed('Half of it'), ended('error')),
      { observe: {} },
    ]),
    resumed("a turn Devin ended for its quota is settled, and its words are the look's quota", [
      ...conversation(SESSION, [
        asked(1, 'Write the parser'),
        replied(2, 'Half of it', 1, { metadata: {} }),
      ]),
      wire(
        shows(SESSION),
        streamed('Half of it'),
        ended('quota_exhausted', {
          errorMessage: 'Your daily usage quota has been exhausted. Resets in 4h 30m',
        }),
      ),
      { observe: {} },
    ]),
    resumed(
      'a look held while its window is followed elsewhere is compared with the new conversation',
      [
        wire(shows(SESSION)),
        { holdLooks: true },
        { observe: {} },
        { holdLooks: false },
        { follow: OTHER },
        { release: 'look' },
      ],
    ),
  ]
}

/** Whether a window is ready for a paste. */
function readiness() {
  const stuck = (answers) => ({ ready: {}, answers, kept: NOT_YET })
  const pane = (...answers) => ({ 'pane.snapshot': answers })
  return [
    resumed('a window that has not said which conversation it shows is not ready', [{ ready: {} }]),
    resumed('a window that shows another conversation is not ready, its snapshot never asked', [
      wire(shows(OTHER)),
      { ready: {} },
    ]),
    resumed('a window that shows its conversation is ready when its snapshot says so', [
      wire(shows(SESSION)),
      { ready: {}, answers: snapshot({}) },
      stuck(snapshot({ pasteInFlight: true })),
      { ready: {}, answers: snapshot({ unsent: true }) },
      stuck(pane({ ok: false, error: 'stale' })),
      stuck(pane({ ok: false })),
      stuck(pane(null)),
      { ready: {}, answers: pane({ throws: 'bridge ended' }) },
    ]),
    resumed(
      "a paste on its way is said before what the human has not sent, by JavaScript's truth",
      [
        wire(shows(SESSION)),
        stuck(snapshot({ pasteInFlight: true, unsent: true })),
        stuck(snapshot({ pasteInFlight: [] })),
        { ready: {}, answers: snapshot({ pasteInFlight: 0, unsent: {} }) },
        { ready: {}, answers: snapshot({ pasteInFlight: '', unsent: 'x' }) },
        { ready: {}, answers: snapshot({ pasteInFlight: null, unsent: [] }) },
        { ready: {}, answers: snapshot({ unsent: 0 }) },
        stuck(pane({ ok: 1 })),
        stuck(pane({ ok: 'true' })),
      ],
    ),
    resumed('a window ready at one look and holding at the next follows its log', [
      wire(shows(SESSION)),
      { ready: {}, answers: snapshot({}) },
      appended(shows(OTHER)),
      { ready: {} },
      { follow: OTHER },
      { ready: {}, answers: snapshot({}) },
    ]),
  ]
}

/** What becomes of a paste. */
function deliveries() {
  const paste = (...answers) => ({ 'pane.write_paste': answers })
  return [
    resumed('a message is pasted as a window takes text, into the conversation it knows', [
      wire(shows(SESSION)),
      { deliver: 'Hello\r\nworld\u001b[31mred', answers: paste({ ok: true }) },
      { deliver: 'Refused', answers: paste({ ok: false, admitted: false, error: 'stale pane' }) },
      { deliver: 'Lost on the way', answers: paste({ throws: 'bridge ended', error: 'eof' }) },
      { deliver: 'Half written', answers: paste({ ok: false, error: 'deadline' }) },
      { deliver: 'Refused, no reason given', answers: paste({ ok: false, admitted: false }) },
      { deliver: 'Unanswered', answers: paste({ ok: false }) },
      { deliver: 'Answered with nothing', answers: paste(null) },
      {
        deliver: 'Taken, whatever else it says',
        answers: paste({ ok: true, admitted: false, error: 'odd' }),
      },
      {
        deliver: 'Refused in its own words',
        answers: paste({ ok: false, admitted: false, cause: 'stale', error: 'pane' }),
      },
    ]),
    resumed('a message is never pasted into another conversation than the one it is for', [
      wire(shows(OTHER)),
      { deliver: 'Hello' },
      appended(shows(SESSION)),
      { deliver: 'Hello', answers: paste({ ok: true }) },
    ]),
    resumed('a message is refused for a log that is not there, and for one that cannot be read', [
      { deliver: 'Hello' },
      wire('null\n'),
      { deliver: 'Hello' },
      wire('{"update": {"sessionUpdate": "config_option_update", "configOptions": [null]}}\n'),
      { deliver: 'Hello' },
      wire(shows(SESSION), 'not json\n', shows(OTHER)),
      { deliver: 'Hello' },
      wire(`\ufeff${shows(SESSION)}`),
      { deliver: 'Hello' },
    ]),
    resumed('a log ending in an unfinished line is read to its last whole one for a message', [
      wire(shows(SESSION), '{"sessionId":'),
      { deliver: 'Hello', answers: paste({ ok: true }) },
    ]),
    fresh('a message for a window that has not named its conversation yet is refused', [
      wire(shows(SESSION)),
      { deliver: 'Hello' },
    ]),
    resumed(
      'a message to a window whose environment says Windows goes as its console carries it',
      [
        wire(shows(SESSION)),
        {
          deliver:
            '[ConsensFlow m-3 · T-1 · answer from @chief]\nblue — not “red” → done… café ¿qué? 50%\r60% \u001b ✓ 😀',
          answers: paste({ ok: true }),
        },
      ],
      { env: WINDOWS, standIn: windowsDevin() },
    ),
    resumed('a message goes as a window takes it, and as the console carries it only on Windows', [
      wire(shows(SESSION)),
      { deliver: 'blue — not “red” → done… \u0001 \u007f \u0085', answers: paste({ ok: true }) },
    ]),
    resumed('a window closed while its paste is held keeps the paste going, its files gone', [
      wire(shows(SESSION)),
      { deliver: 'Hello', answers: { 'pane.write_paste': [{ held: true }] } },
      { close: {} },
      { release: 'pane.write_paste', answer: { ok: true } },
    ]),
  ]
}

/** Every look at a Devin window that its store and its panel say, and every paste. */
export function devinPanes() {
  return [...records(), ...readiness(), ...deliveries()]
}
