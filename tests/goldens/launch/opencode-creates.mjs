/**
 * The conversation a fresh OpenCode window opens on (`createSession`,
 * `src/channels/opencode.js`): made on a throwaway `serve` child before the
 * window opens, its health polled within 15 s and one session made with
 * what is left, the child stopped before the id returns and on every
 * failure. The child, and the server it plays, are scripted: its health is
 * answered by route (`GET /global/health`), and the session by `POST
 * /session`, a child that ends as its script says (`children`).
 */
import { ENV, HEALTHY, installed, launch, patient, WORK, workspace } from './opencode-scenes.mjs'

const HEALTH = 'GET /global/health'
const SESSION = 'POST /session'

/** A child that ends when asked, and one that goes on until it is forced, or for good. */
const asked = { opencode: [{ ends: 'asked' }] }
const forced = { opencode: [{ ends: 'forced' }] }
const never = { opencode: [{ ends: 'never' }] }

/** A server that does not answer: `times` polls, each held until it is given up on. */
const silent = (times) => Array.from({ length: times }, () => ({ held: true }))

/** A server nobody can reach: `times` polls that fail at once. */
const unanswered = (times) => Array.from({ length: times }, () => ({ noHead: true }))

/** The session as the throwaway server makes it, `fields` over the right ones. */
const ID = 'ses_abc123'
const made = (fields = {}) => ({
  status: 200,
  body: { json: { id: ID, directory: WORK, ...fields } },
})

/** A server answering `served`, its child ending as `children` says. */
const answering = (served, children = asked) => ({ served, children })

/** A server that is up and makes its session as `answer` says. */
const making = (answer = made(), children = asked) =>
  answering({ [HEALTH]: [HEALTHY], [SESSION]: [answer] }, children)

/** A fresh window's conversation made on `scripts`, then `steps`. */
const scenario = (name, scripts, steps = [], { before = [workspace], fields = {} } = {}) => ({
  name: `opencode: a fresh window's conversation: ${name}`,
  harness: 'opencode',
  env: ENV,
  steps: patient([installed, ...before, { prepare: launch(fields), ...scripts }, ...steps]),
})

export function createScenarios() {
  return [
    scenario('a conversation is made, and its child stopped before the id returns', making()),
    scenario(
      'the server answers unauthorized at its health check',
      answering({ [HEALTH]: [{ status: 401, body: 'unauthorized' }] }),
    ),
    scenario(
      'a server that never answers is given up on at fifteen seconds',
      answering({ [HEALTH]: silent(25) }),
      [{ advance: 15000 }],
    ),
    scenario(
      'a poll that is cut short by the deadline waits only what is left, at least a millisecond',
      answering({ [HEALTH]: [{ status: 503, body: '{}' }, ...silent(25)] }),
      [{ advance: 15001 }],
    ),
    scenario(
      'a server that exits at once says so once it has been polled',
      {
        children: { opencode: [{ ends: 'itself' }] },
      },
      [{ advance: 100 }],
    ),
    scenario('a server that cannot start says so once it has been polled', {}, [{ advance: 100 }]),
    scenario(
      'a server that is polled until it is up',
      answering({ [HEALTH]: [{ status: 503, body: '{}' }, HEALTHY], [SESSION]: [made()] }),
      [{ advance: 100 }],
    ),
    scenario(
      'a poll that stalls is given up on at half a second and tried again',
      answering({ [HEALTH]: [{ held: true }, HEALTHY], [SESSION]: [made()] }),
      [{ advance: 499 }, { advance: 101 }],
    ),
    scenario(
      'a poll whose body never ends is no failure once its head said the server is up',
      answering({ [HEALTH]: [{ status: 200, body: { held: true } }], [SESSION]: [made()] }),
      [{ advance: 500 }],
    ),
    scenario(
      'a poll whose body is too large or is cut off is read and let go',
      answering({
        [HEALTH]: [
          { status: 503, body: { repeat: 'x', times: 64 * 1024 + 1 } },
          { status: 503, body: { cut: true } },
          HEALTHY,
        ],
        [SESSION]: [made()],
      }),
      [{ advance: 100 }, { advance: 100 }],
    ),
    scenario(
      'a poll answered up whose body is too large is up',
      answering({
        [HEALTH]: [{ status: 200, body: { repeat: 'x', times: 64 * 1024 + 1 } }],
        [SESSION]: [made()],
      }),
    ),
    scenario(
      'a poll answered up whose body is cut off is up',
      answering({ [HEALTH]: [{ status: 200, body: { cut: true } }], [SESSION]: [made()] }),
    ),
    scenario(
      'an unauthorized poll whose body never ends is waited on, and given up on at half a second',
      answering({ [HEALTH]: [{ status: 401, body: { held: true } }] }),
      [{ advance: 500 }],
    ),
    scenario(
      'a server that goes on when asked to end is forced at two seconds',
      making(made(), forced),
      [{ advance: 2000 }],
    ),
    scenario(
      'a server that will not stop fails the creation, and is stopped a second time',
      making(made(), never),
      [{ advance: 4000 }, { advance: 4000 }],
    ),
    scenario(
      'a server that will not stop after a failure is stopped once, and the failure is the answer',
      making(made({ id: 'bad' }), never),
      [{ advance: 4000 }],
    ),
    scenario(
      'a conversation in another folder than the window’s is refused',
      making(made({ directory: '$ROOT/elsewhere' })),
    ),
    scenario('an id that is no OpenCode id is refused', making(made({ id: 'bad' }))),
    scenario('an answer with no id is refused', making({ status: 200, body: '{}' })),
    scenario(
      'an answer that is no object is read as one with no id',
      making({ status: 200, body: 'null' }),
    ),
    scenario('an answer that is no JSON is refused', making({ status: 200, body: 'not json{' })),
    scenario(
      'an answer past a megabyte is refused as oversized',
      making({ status: 200, body: { repeat: 'x', times: 1024 * 1024 + 1 } }),
    ),
    scenario(
      'an answer of a megabyte is not oversized, and is no JSON',
      making({ status: 200, body: { repeat: ' ', times: 1024 * 1024 } }),
    ),
    scenario('an answer cut off is unreadable', making({ status: 200, body: { cut: true } })),
    scenario(
      'a request refused as unauthorized says so',
      making({ status: 401, body: 'unauthorized' }),
    ),
    scenario('a request rejected with a status says it', making({ status: 500, body: 'oops' })),
    scenario(
      'a request answered with a redirect is no success either',
      making({ status: 300, body: '{}' }),
    ),
    scenario(
      'a poll answered with a redirect is not up, and is tried again',
      answering({ [HEALTH]: [{ status: 300, body: '{}' }, HEALTHY], [SESSION]: [made()] }),
      [{ advance: 100 }],
    ),
    scenario('a request nobody answers is a transport failure', making({ noHead: true })),
    scenario('a request held past the time left is timed out', making({ held: true }), [
      { advance: 15000 },
    ]),
    scenario(
      'an answer whose body is held past the time left is unreadable',
      making({ status: 200, body: { held: true } }),
      [{ advance: 15000 }],
    ),
    scenario(
      'the session is made with the time the polls left',
      answering({ [HEALTH]: [...unanswered(144), HEALTHY], [SESSION]: [{ held: true }] }),
      [{ advance: 14400 }, { advance: 599 }, { advance: 1 }],
    ),
    scenario('a folder that is not there is refused', {}, [], {
      before: [],
      fields: { directory: '$ROOT/missing' },
    }),
  ]
}
