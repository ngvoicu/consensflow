/**
 * What a lead must DO, checked against a real lead.
 *
 * Every scenario here is a failure that actually happened on this machine on
 * 2026-08-24, in a live session. The skill was changed each time; nothing
 * measured whether the change worked, because the unit tests check what the
 * skill SAYS and no test can check what a lead DOES with it. This does.
 *
 * A scenario is a list of things the user says, in one lead session. After
 * each turn the commands that turn ran are checked — the lead's `cf` and
 * `cmux` are recording stubs, so its choices are the observation.
 */

const ran = (log, prefix) => log.some((line) => line.startsWith(prefix))
const sent = (log, text) =>
  log.some((line) => line.startsWith('cmux send') && line.includes(text))

export const SCENARIOS = [
  {
    id: 'consult-opens-a-pane',
    why: 'A lead ran `cf run` in its own pane, because it never opened the skill body at all.',
    turns: [
      {
        say: 'ask nyx for a joke',
        expect: [
          ['names the conversation first', (log) => ran(log, 'cf mint')],
          ['opens a pane', (log) => ran(log, 'cmux new-pane')],
          ['sends the consult into that pane', (log) => sent(log, 'cf run @nyx')],
          ['does NOT consult in its own pane', (log) => !ran(log, 'cf run')],
        ],
      },
    ],
  },
  {
    id: 'reading-is-not-writing',
    why: 'Asked "can you see what other jokes he said?", a lead SENT another request and invented a new answer instead of reading the existing one.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      {
        say: 'can you see what other jokes he said?',
        expect: [
          ['reads the conversation', (log) => ran(log, 'cf catchup')],
          ['asks for what is new', (log) => log.some((l) => l.startsWith('cf catchup') && l.includes('--unread'))],
          ['sends NOTHING into the pane', (log) => !ran(log, 'cmux send')],
          ['starts no new consult', (log) => !ran(log, 'cf run')],
        ],
      },
    ],
  },
  {
    id: 'look-before-you-send',
    why: 'A follow-up composed against a stale view asks the wrong question — the user may have moved the conversation in its pane.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      {
        say: 'ask him for another one',
        expect: [
          ['looks first', (log) => ran(log, 'cf catchup')],
          ['sends the follow-up into the pane', (log) => ran(log, 'cmux send')],
          ['does not restart the conversation', (log) => !log.some((l) => l.includes('--new'))],
        ],
      },
    ],
  },
  {
    id: 'answers-from-the-conversation',
    why: 'Asked "did you see her last answer?", a lead answered from its own memory while the user\'s pane turns sat unread.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      { say: 'thanks', expect: [] },
      {
        say: 'did nyx say anything else after that?',
        expect: [
          ['looks instead of remembering', (log) => ran(log, 'cf catchup')],
          ['sends nothing', (log) => !ran(log, 'cmux send')],
        ],
      },
    ],
  },
]
