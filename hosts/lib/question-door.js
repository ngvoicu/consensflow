/**
 * The door a member's harness question tool opens onto the board. The
 * window's plugin, hook or broker posts the questions as its participant,
 * waits for the chief to answer on the board, and hands the answer back
 * into the tool call, so nothing is typed into the window. When
 * the answer does not come in time, the door gives up and the harness's own
 * dialog takes over; when the window answered first, the board's copy of the
 * question gets that answer, so nobody answers it twice.
 */

/** How long a door waits for the board before the harness's own dialog takes over. */
const DOOR_WAIT_MS = 3_500_000
const POLL_WAIT_MS = 20_000

/** A request to the board's API as the window's participant; `null` outside a window. */
export function boardClient({
  url = process.env.CONSENSFLOW_URL,
  token = process.env.CONSENSFLOW_TOKEN,
  fetch: fetchImpl = fetch,
} = {}) {
  if (typeof url !== 'string' || url.length === 0 || typeof token !== 'string') return null
  return async (method, path, body, { signal } = {}) => {
    const response = await fetchImpl(`${url}${path}`, {
      method,
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      ...(signal === undefined ? {} : { signal }),
    })
    const value = await response.json().catch(() => ({}))
    if (!response.ok) {
      // Answered, and refused: not the same as a board that cannot be reached.
      throw Object.assign(new Error(value.message ?? `ConsensFlow answered ${response.status}`), {
        refused: true,
      })
    }
    return value
  }
}

/**
 * What a member's window tells its model when the board refuses its
 * question: nobody watches a member's window, so its own dialog would hold
 * the task for good.
 */
export const refusalReason = (cause) =>
  `ConsensFlow could not put this question to the chief (${cause.message}). Ask with cf ask "…" instead.`

/**
 * Puts the questions on the board and waits for their answer: `{ id, answer }`,
 * the answer null when the wait ran out or `signal` ended it. `questions` are
 * in the board's shape: question, header, options (label, description), multiple.
 */
export async function askTheBoard(client, questions, { waitMs = DOOR_WAIT_MS, signal } = {}) {
  const { message } = await client('POST', '/api/questions', { questions })
  const until = Date.now() + waitMs
  let answer = null
  while (answer === null && Date.now() < until && !signal?.aborted) {
    const wait = Math.min(POLL_WAIT_MS, until - Date.now())
    try {
      answer = (await client('GET', `/api/questions/${message.id}?wait=${wait}`, undefined, { signal }))
        .answer
    } catch (cause) {
      if (signal?.aborted) break
      throw cause
    }
  }
  return { id: message.id, answer }
}

/** The window answered first: the board's copy of the question takes that answer, from the asker. */
export const answerFromWindow = (client, id, choices) =>
  client('POST', '/api/answers', { question: id, choices })
