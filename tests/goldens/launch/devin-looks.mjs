/**
 * What Devin's own wire log says of its window (`started` and `observe`, with
 * `src/devin-wire.js`): how the window names the conversation it opened, which
 * conversation it shows, and its word on its quota.
 */
import { readFileSync } from 'node:fs'
import {
  appended,
  devin,
  ENV,
  fresh,
  launch,
  line,
  OTHER,
  prompted,
  refusedInWords,
  resumed,
  SESSION,
  said,
  shows,
  snapshot,
  WIRE,
  wire,
} from './devin-fixtures.mjs'

/** What Rust says where V8 threw on a line of the wire log that is no record. */
const notARecord = (why) =>
  refusedInWords(
    `a sentence of Rust's own where V8 threw its TypeError, ${why}`,
    "Devin's wire log holds a line that cannot be read for the conversation it shows",
  )

/** How Devin's window names the conversation it opened. */
function started() {
  return [
    fresh('a window that names its conversation after some polls is followed there', [
      { started: {} },
      { advance: 249 },
      { advance: 1 },
      { advance: 250 },
      wire(shows(SESSION)),
      { advance: 250 },
      { observe: {} },
    ]),
    fresh('a conversation named at the first poll is returned with no wait', [
      wire(shows(SESSION)),
      { started: {} },
    ]),
    fresh('a window that never names one is given up on at its deadline, not before', [
      { started: {} },
      { advance: 59_749 },
      { advance: 1 },
      { advance: 249 },
      { advance: 1 },
    ]),
    fresh('a wire log that is no JSON, or is not there, is polled on', [
      wire('not json\n'),
      { started: {} },
      { advance: 250 },
      { remove: WIRE },
      { advance: 250 },
      wire(shows(OTHER)),
      { advance: 250 },
    ]),
    fresh('a line still being written names nothing yet', [
      wire(shows(SESSION).slice(0, 30)),
      { started: {} },
      appended(shows(SESSION).slice(30)),
      { advance: 250 },
    ]),
    fresh('a wire log with a line that is no JSON names no conversation, however many follow it', [
      wire(`\ufeff${shows(OTHER)}`),
      { started: {} },
      appended(shows(SESSION)),
      { advance: 60_000 },
    ]),
    fresh('the last conversation the log names is the one found', [
      wire(shows(OTHER), shows(SESSION)),
      { started: {} },
    ]),
    resumed('a resumed window has its conversation and polls nothing', [{ started: {} }]),
    fresh('a window that named its conversation is delivered to', [
      { started: {} },
      wire(shows(SESSION)),
      { advance: 250 },
      { ready: {}, answers: snapshot({}) },
      { deliver: 'Hello', answers: { 'pane.write_paste': [{ ok: true }] } },
    ]),
    {
      name: 'devin: a window of a launch closed while it polls goes on polling',
      harness: 'devin',
      env: ENV,
      steps: [devin(), { prepare: launch() }, { started: {} }, { close: {} }, { advance: 250 }],
      // Its wire log is gone with its launch's files, and nothing ends the wait.
      pendingAtEnd: [2],
    },
  ]
}

/** What a look at a window says, by its wire log alone: the record of a conversation that is not kept is none. */
function looks() {
  return [
    fresh('a window that has not said which conversation it opened says nothing yet', [
      { observe: {} },
    ]),
    resumed('a window that has said nothing yet is held, its record settled and empty', [
      { observe: {} },
    ]),
    resumed('a window that shows its conversation is observed as it is', [
      wire(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a window that shows another conversation is followed there, nothing settled', [
      wire(shows(OTHER)),
      { observe: {} },
    ]),
    resumed('the wire log appended between two looks is read from where it was left', [
      wire(shows(SESSION)),
      { observe: {} },
      appended(shows(OTHER)),
      { observe: {} },
      { follow: OTHER },
      { observe: {} },
      appended(shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a line cut in the middle is carried to the look that finds its end', [
      wire(shows(SESSION).slice(0, 40)),
      { observe: {} },
      appended(shows(SESSION).slice(40)),
      { observe: {} },
    ]),
    resumed('a line is carried across three looks, a piece at a time', [
      wire(shows(SESSION).slice(0, 10)),
      { observe: {} },
      appended(shows(SESSION).slice(10, 60)),
      { observe: {} },
      appended(shows(SESSION).slice(60)),
      { observe: {} },
    ]),
    resumed('a wire log replaced by a shorter one is read from its start, nothing carried', [
      wire(shows(OTHER), shows(SESSION), '{"sessionId":"mild'),
      { observe: {} },
      wire(shows(OTHER)),
      { observe: {} },
    ]),
    resumed('a wire log replaced by one of the same size is not seen to change', [
      wire(shows(SESSION)),
      { observe: {} },
      wire(shows(SESSION).replace('mild-coin', 'wild-coin')),
      { observe: {} },
    ]),
    resumed('a wire log replaced by a longer one is read from where the old one ended', [
      wire(shows(SESSION)),
      { observe: {} },
      wire(shows(SESSION), shows(OTHER)),
      { observe: {} },
    ]),
    resumed('a wire log emptied and written again is read on from where the old one ended', [
      wire(shows(SESSION)),
      { observe: {} },
      wire(),
      { observe: {} },
      wire(shows(OTHER)),
      { observe: {} },
    ]),
    resumed('a wire log that is gone says what it said before', [
      wire(shows(SESSION)),
      { observe: {} },
      { remove: WIRE },
      { observe: {} },
    ]),
    resumed('lines that are no JSON are passed over, and so are lines that name nothing', [
      wire(
        'not json\n',
        '\n',
        '   \n',
        '{"sessionId": "x"\n',
        '5\n',
        '"text"\n',
        '[]\n',
        'true\n',
        line({ update: null }),
        line({ update: { sessionUpdate: 'config_option_update' }, sessionId: OTHER }),
        line({
          update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'model' }] },
          sessionId: OTHER,
        }),
        line({
          update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
          sessionId: 7,
        }),
        shows(SESSION),
      ),
      { observe: {} },
    ]),
    resumed('a line ended in CRLF is read all the same, the last conversation named winning', [
      wire(shows(OTHER).replace('\n', '\r\n'), shows(SESSION).replace('\n', '\r\n')),
      { observe: {} },
    ]),
    resumed('a wire log opening on a byte order mark has its first line passed over', [
      wire(`\ufeff${shows(OTHER)}`, shows(SESSION)),
      { observe: {} },
    ]),
    resumed('a null line stops the look, and the lines after it are not read again', [
      wire(shows(OTHER), 'null\n', shows(SESSION)),
      {
        observe: {},
        kept: notARecord('on a line that is null'),
      },
      { observe: {} },
    ]),
    resumed('a configuration of a mode that is no list stops the look as well', [
      wire(
        line({
          sessionId: OTHER,
          update: { sessionUpdate: 'config_option_update', configOptions: 'mode' },
        }),
      ),
      { observe: {}, kept: notARecord('on a list that is no list') },
    ]),
    resumed('a null option in a configuration stops the look as well', [
      wire(
        line({
          sessionId: OTHER,
          update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'model' }, null] },
        }),
      ),
      { observe: {}, kept: notARecord('on an option that is null') },
    ]),
    resumed('a null line stops the readiness as well', [
      wire(shows(SESSION), 'null\n'),
      { ready: {}, kept: notARecord('on a line that is null') },
    ]),
  ]
}

/** The refusals Devin's own words and codes make of its quota. */
function quotas() {
  return [
    resumed('a refusal in its words reads as exhausted, with the reset it names', [
      wire(
        shows(SESSION),
        prompted(),
        said('Reached overall message rate limit. Your limit will reset in 35 minutes.'),
      ),
      { observe: {} },
      { advance: 60_000 },
      { observe: {} },
    ]),
    resumed('every word of Devin for a refusal counts, in any case', [
      wire(shows(SESSION), said('Usage limit reached')),
      { observe: {} },
      appended(prompted(3)),
      { observe: {} },
      appended(said('QUOTA EXHAUSTED. resets in 2 hours')),
      { observe: {} },
      appended(prompted(4)),
      { observe: {} },
      appended(said('Your quota has been exhausted.')),
      { observe: {} },
      appended(prompted(5)),
      { observe: {} },
      appended(said('You hit the Rate Limit')),
      { observe: {} },
    ]),
    resumed("words that are not Devin's refusal are not one", [
      wire(
        shows(SESSION),
        said('rate  limit'),
        said('the usage was limited'),
        said('quota'),
        said('exhausted'),
        said('uſage limit'),
        line({
          update: {
            sessionUpdate: 'agent_thought_chunk',
            content: { type: 'text', text: 'rate limit' },
          },
        }),
        line({
          update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 7 } },
        }),
        line({ update: { sessionUpdate: 'agent_message_chunk', content: 'rate limit' } }),
        line({ update: { sessionUpdate: 'agent_message_chunk' } }),
      ),
      { observe: {} },
    ]),
    resumed('a refusal is cleared by the next prompt and not before', [
      wire(shows(SESSION), said('Usage limit reached')),
      { observe: {} },
      appended(said('On it.')),
      { observe: {} },
      appended(prompted()),
      { observe: {} },
    ]),
    resumed('the prompt itself refused, in the code Devin gives, is exhausted', [
      wire(
        shows(SESSION),
        prompted(),
        line({
          jsonrpc: '2.0',
          id: 2,
          error: {
            code: -32011,
            message: 'Quota exhausted.',
            data: {
              'cognition.ai/errorKind': 'resource_exhausted',
              'cognition.ai/retryable': true,
            },
          },
        }),
      ),
      { observe: {} },
    ]),
    resumed('a refusal is told by its code alone, by its kind alone, or by its words alone', [
      wire(shows(SESSION), line({ error: { code: -32011 } })),
      { observe: {} },
      appended(prompted()),
      { observe: {} },
      appended(
        line({
          error: {
            data: { 'cognition.ai/errorKind': 'resource_exhausted' },
            message: 'resets in 1 hour',
          },
        }),
      ),
      { observe: {} },
      appended(prompted(3)),
      { observe: {} },
      appended(line({ error: { message: 'Rate limit reached, resets in 3 days' } })),
      { observe: {} },
      appended(prompted(4)),
      { observe: {} },
      // The code as the number it is, however Devin spells it: JSON.stringify writes none of these.
      appended('{"error": {"code": -32011.0, "message": 7}}\n'),
      { observe: {} },
      appended(prompted(5)),
      { observe: {} },
      appended('{"error": {"code": -3.2011e4}}\n'),
      { observe: {} },
      // Its words as `String` makes them of whatever they are: a list is its items.
      appended(prompted(6)),
      { observe: {} },
      appended(line({ error: { code: -32011, message: ['resets in 2 hours'] } })),
      { observe: {} },
    ]),
    resumed('an error that is not a refusal is none', [
      wire(
        shows(SESSION),
        line({ error: { code: -32000, message: 'Something else' } }),
        line({ error: { code: '-32011', message: 'text' } }),
        line({ error: { data: { 'cognition.ai/errorKind': 'other' } } }),
        line({ error: 'rate limit' }),
        line({ error: null }),
        line({ error: { message: 7 } }),
      ),
      { observe: {} },
    ]),
    resumed('a turn Devin ended for its quota is exhausted, with the words it gives', [
      wire(
        shows(SESSION),
        prompted(),
        line({
          cause: 'quota_exhausted',
          errorMessage:
            'Your daily usage quota has been exhausted. Resets in 4h 30m (trace ID: c27105417b16)',
          sessionId: SESSION,
        }),
      ),
      { observe: {} },
    ]),
    resumed('a turn ended for its quota with no words is exhausted with no reset', [
      wire(shows(SESSION), line({ cause: 'quota_exhausted' })),
      { observe: {} },
      appended(line({ cause: 'quota_exhausted', errorMessage: null })),
      { observe: {} },
      appended(line({ cause: 'complete' })),
      { observe: {} },
    ]),
    resumed('a reset named by a time of day in its zone is read in that zone', [
      wire(shows(SESSION), said('Usage limit reached. Resets 7:30pm (Europe/Bucharest)')),
      { observe: {} },
    ]),
    resumed('a refusal read at a later look is dated at that look', [
      wire(shows(SESSION)),
      { observe: {} },
      { advance: 90_000 },
      appended(said('Usage limit reached. Resets in 2 hours.')),
      { observe: {} },
    ]),
    resumed('a reset too far to be a date fails the look', [
      wire(shows(SESSION), said('Usage limit reached. Resets in 99999999999999999 days')),
      { observe: {} },
    ]),
    resumed(
      'a refusal is kept by a window that has not said where it shows, and by one that shows another',
      [wire(said('Usage limit reached')), { observe: {} }, appended(shows(OTHER)), { observe: {} }],
    ),
  ]
}

/**
 * Wire logs Devin wrote itself (the records' fixtures, `tests/engine/fixtures/completion/devin`),
 * read as they grew: a third of each, then to the middle of a line, then the rest.
 */
function real() {
  return ['native-tui.json', 'worker-tui.json'].map((name) => {
    const { session, wire: events } = JSON.parse(
      readFileSync(
        new URL(`../../engine/fixtures/completion/devin/${name}`, import.meta.url),
        'utf8',
      ),
    )
    const whole = events.map((event) => `${JSON.stringify(event)}\n`).join('')
    // Cuts that leave no half a character: a line's end, and ten characters into the next.
    const cut = (at) => {
      let end = whole.indexOf('\n', at) + 11
      while (!whole.slice(0, end).isWellFormed()) end += 1
      return end
    }
    const [first, second] = [Math.floor(whole.length / 3), Math.floor((2 * whole.length) / 3)].map(
      cut,
    )
    return resumed(
      `a wire log Devin wrote itself (${name}) is read as it grew, in pieces cut in the middle of a line`,
      [
        wire(whole.slice(0, first)),
        { observe: {} },
        appended(whole.slice(first, second)),
        { observe: {} },
        appended(whole.slice(second)),
        { observe: {} },
        { ready: {}, answers: snapshot({}) },
      ],
      { fields: { resume: session.id } },
    )
  })
}

/** Every look at a Devin window by its wire log: how it names its conversation, and what it says. */
export function devinLooks() {
  return [...started(), ...looks(), ...quotas(), ...real()]
}
