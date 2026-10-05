/**
 * Pi's deliveries (`deliver`, `src/adapters/pi.js`, through `send`,
 * `src/channels/pi.js`): a claim of the pane, a record in the extension's
 * inbox, and the extension's verdict, which the scenario writes between
 * `advance` steps. A fresh window draws 4 bytes for its session, and each
 * message after it 16 for its id.
 */
import {
  ackFile,
  acknowledges,
  claim,
  claimed,
  FIRST,
  FOLDER,
  inboxFile,
  MESSY,
  OTHER,
  opened,
  SECOND_MESSAGE,
  THIRD,
} from './pi-scenes.mjs'

export function deliveryScenarios() {
  return [
    opened('a message goes through the extension: a claim, the inbox, and its verdict', [
      { deliver: 'hi', answers: claimed },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a message is written as a window takes text', [
      { deliver: MESSY, answers: claimed },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a message the extension refuses before it sends is refused', [
      { deliver: 'for another conversation', answers: claimed },
      acknowledges(FIRST, { admitted: false, bytesWritten: 0, reason: 'native session changed' }),
      { advance: 10 },
      { deliver: 'refused with no word of its bytes', answers: claimed },
      acknowledges(SECOND_MESSAGE, { admitted: false, reason: 'wrong-launch' }),
      { advance: 10 },
    ]),
    opened('a verdict that is no word, or no known word, leaves the message uncertain', [
      { deliver: 'not yet', answers: claimed },
      acknowledges(FIRST, { admitted: null, reason: 'admission-unknown' }),
      { advance: 10 },
      { deliver: 'something else', answers: claimed },
      acknowledges(SECOND_MESSAGE, { admitted: 'yes' }),
      { advance: 10 },
      { deliver: 'no word at all', answers: claimed },
      acknowledges(THIRD, { mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('what is no acknowledgement of this message is not taken for one', [
      { deliver: 'wait for it', answers: claimed },
      acknowledges('m-someone-else', { admitted: true }),
      { advance: 10 },
      { write: ackFile(FIRST), text: '{"id": "m-' },
      { advance: 10 },
      { write: ackFile(FIRST), text: '"a string"' },
      { advance: 10 },
      { write: ackFile(FIRST), text: 'null' },
      { advance: 10 },
      { write: ackFile(FIRST), text: '[]' },
      { advance: 10 },
      { write: ackFile(FIRST), text: '{"id":7,"admitted":true}' },
      { advance: 10 },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a verdict the system will not let be read leaves the message uncertain, in the inbox', [
      { deliver: 'unreadable', answers: claimed },
      { write: `${ackFile(FIRST)}/folder`, text: 'x' },
      { advance: 10 },
    ]),
    opened('a claim the host refuses leaves nothing in the inbox, in its words', [
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
    opened('a claim that is not words is written as a template writes it', [
      {
        deliver: 'a number',
        answers: claim({ ok: false, admitted: false, cause: 0 }),
        kept: {
          why: "the host's cause is its words, and a reason is text; Node passed any other value on as the reason itself",
          answer: { answer: { admitted: false, reason: '0' } },
        },
      },
    ]),
    opened('a claim the host never answered leaves nothing in the inbox, in its words', [
      { deliver: 'bridge down', answers: claim({ throws: 'bridge ended', error: 'eof' }) },
      { deliver: 'no word', answers: claim({ throws: 'bridge ended' }) },
    ]),
    opened('a claim held keeps the record beside the inbox until it is answered', [
      { deliver: 'wait for the claim', answers: claim({ held: true }) },
      { release: 'pane.claim', answer: { ok: true } },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a claim refused after it was held leaves nothing in the inbox', [
      { deliver: 'wait for the claim', answers: claim({ held: true }) },
      { release: 'pane.claim', answer: { ok: false, admitted: false, error: 'stale' } },
    ]),
    opened('a message with nothing in it is refused before anything is drawn or written', [
      { deliver: '' },
      { deliver: 'x', answers: claimed },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a window followed to no name is sent nothing', [{ follow: '' }, { deliver: 'x' }]),
    opened('a pane with no generation is refused after the record was written', [
      { deliver: 'to no pane', pane: { id: 'p1-zeus', generation: 0 } },
      { deliver: 'to a pane past the safe integers', pane: { id: 'p1-zeus', generation: 2 ** 53 } },
      {
        deliver: 'to the last safe one',
        pane: { id: 'p1-zeus', generation: 2 ** 53 - 1 },
        answers: claimed,
      },
      acknowledges(THIRD, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('an inbox that is a file cannot be written to', [
      { write: `${FOLDER}/inbox`, text: 'x' },
      { deliver: 'nowhere to put it' },
    ]),
    opened('an acknowledgements folder that is a file cannot be written to either', [
      { write: `${FOLDER}/ack`, text: 'x' },
      { deliver: 'nowhere to hear from it' },
    ]),
    opened('a record that cannot be written is told, and what could not be removed stays', [
      { write: `${inboxFile(FIRST)}.tmp/inside`, text: 'x' },
      { deliver: 'its own folder in the way' },
    ]),
    opened('a message nobody acknowledges is uncertain once its time and its grace are up', [
      { deliver: 'followup nobody admits', answers: claimed },
      { advance: 31000 },
      { advance: 1 },
    ]),
    opened('a verdict that lands within the grace past the expiry is still Pi’s own', [
      { deliver: 'answered late', answers: claimed },
      { advance: 30200 },
      acknowledges(FIRST, { admitted: null, reason: 'admission-unknown' }),
      { advance: 10 },
    ]),
    opened('a verdict that lands after the grace is missed', [
      { deliver: 'answered too late', answers: claimed },
      { advance: 31001 },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('two messages at once keep their own ids and their own verdicts', [
      { deliver: 'first', answers: claimed },
      { advance: 3 },
      { deliver: 'second', answers: claimed },
      acknowledges(SECOND_MESSAGE, { admitted: true, mode: 'tui' }),
      { advance: 10 },
      acknowledges(FIRST, { admitted: false, bytesWritten: 0, reason: 'expired-before-send' }),
      { advance: 10 },
    ]),
    opened('two messages one after the other draw an id each', [
      { deliver: 'first', answers: claimed },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
      { deliver: 'second', answers: claimed },
      acknowledges(SECOND_MESSAGE, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened('a message goes to the conversation the window was followed to', [
      { follow: OTHER },
      { deliver: 'to the new conversation', answers: claimed },
      acknowledges(FIRST, { admitted: true, mode: 'tui' }),
      { advance: 10 },
    ]),
    opened(
      'a window closed while its message waits for the verdict keeps waiting, its files gone',
      [{ deliver: 'in flight', answers: claimed }, { close: {} }, { advance: 100 }],
      {},
      { pendingAtEnd: [2] },
    ),
  ]
}
