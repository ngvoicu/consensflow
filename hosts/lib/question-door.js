/**
 * The door a member's harness question tool opens onto the board, for
 * OpenCode's plugin, which loads it into OpenCode's own runtime (the native
 * cf has its own, in crates/cf-board). The plugin posts the questions as its
 * window's participant, waits for the chief to answer on the board, and
 * hands the answer back into the tool call, so nothing is typed into the
 * window. When the answer does not come in time, the door gives up and the
 * harness's own dialog takes over; when the window answered first, the
 * board's copy of the question gets that answer, so nobody answers it twice.
 *
 * The answer a poll finds is claimed for the door, and is received only once
 * the door says it handed it over (`acknowledge`): a door that never says so
 * leaves the answer to arrive a second time as text, and loses none. A poll
 * whose reply was lost is asked again, and gets the same claimed answer. A door
 * the board shut (the task was stopped) is refused with the words its model
 * is to hear, which are handed over as they are.
 */

/** How long a door waits for the board before the harness's own dialog takes over. */
const DOOR_WAIT_MS = 3_500_000
const POLL_WAIT_MS = 20_000
/**
 * How long a door waits before it asks again after a poll that got no answer,
 * once for each poll that failed in a row: the poll's reply may have been lost
 * on its way back, and the board gives the same claimed answer to the same
 * question again. A board that stays out of reach through all of them is
 * gone, and the harness's own dialog takes over.
 */
const POLL_RETRIES_MS = [250, 500, 1_000, 2_000]

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
        code: value.error,
      })
    }
    return value
  }
}

/**
 * What a member's window tells its model when the board refuses its
 * question: nobody watches a member's window, so its own dialog would hold
 * the task for good. A door the board shut says in its own words that its
 * answer comes as a message, and those words go as they are.
 */
export const refusalReason = (cause) =>
  cause.code === 'door-closed'
    ? cause.message
    : `ConsensFlow could not put this question to the chief (${cause.message}). Ask with cf ask "…" instead.`

/**
 * Puts the questions on the board and waits for their answer: `{ id, answer }`,
 * the answer null when the wait ran out or `signal` ended it. `questions` are
 * in the board's shape: question, header, options (label, description), multiple.
 * A poll that gets no answer is asked again (`retries`, the pauses between
 * them); the question itself is put once, whatever comes of it.
 */
export async function askTheBoard(client, questions, { signal, retries = POLL_RETRIES_MS } = {}) {
  const { message } = await client('POST', '/api/questions', { questions })
  const until = Date.now() + DOOR_WAIT_MS
  let answer = null
  let lost = 0
  while (answer === null && Date.now() < until && !signal?.aborted) {
    const wait = Math.min(POLL_WAIT_MS, until - Date.now())
    try {
      answer = (
        await client('GET', `/api/questions/${message.id}?wait=${wait}`, undefined, { signal })
      ).answer
      lost = 0
    } catch (cause) {
      if (signal?.aborted) break
      // Answered, and refused: final. No answer at all: asked again.
      if (cause?.refused || lost >= retries.length) throw cause
      await new Promise((resolve) => setTimeout(resolve, retries[lost++]))
    }
  }
  return { id: message.id, answer }
}

/**
 * The door handed the answer it claimed to its harness, or could not: the board
 * is told so, which makes the answer received, or gives the claim back. What
 * the board says to it is of no use to a door that has done what it was for,
 * so a failure (a board of Node's, which knows no such route, included) is
 * not raised.
 */
export const acknowledge = (client, answer, received) =>
  client('POST', `/api/answers/${answer.id}/receipt`, { received }).catch(() => {})

/** The window answered first: the board's copy of the question takes that answer, from the asker. */
export const answerFromWindow = (client, id, choices) =>
  client('POST', '/api/answers', { question: id, choices })
