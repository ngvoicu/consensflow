import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { describe, it } from 'node:test'
import { admission } from '../src/adapters/shared.js'
import { Bridge } from '../src/bridge.js'
import { send as sendPi } from '../src/channels/pi.js'

/**
 * One contract for every channel's answer to a send (the owner's rule,
 * 2026-10-02): every harness treats a message that may have been sent the
 * same way. Each channel has a handover point (Pi's inbox rename, a paste's
 * first byte, Codex's and OpenCode's POST):
 * - refused before it, nothing reached the harness: `admitted: false` with
 *   `bytesWritten: 0`, and the dispatcher may send again at once;
 * - any error at or after it may have reached the harness: `admitted: null`,
 *   and the dispatcher waits for the harness's own record, sending again only
 *   if the record never shows the message in time;
 * - accepted: `admitted: true`.
 * Every channel runs every row through its real send function, against
 * stand-ins for the pane host and the harness side, and the adapters read
 * each answer the same way (`admission()`): a refusal read into an answer
 * that does not say one is a message delivered twice.
 */
const ROWS = {
  refused: { says: 'refused before its handover point', admitted: false },
  uncertain: { says: 'an error at or after its handover point', admitted: null },
  accepted: { says: 'accepted', admitted: true },
}

/**
 * The pane host at the other end of a real bridge: it admits a claim, or
 * answers it as told.
 */
function paneHost(t, { claim = { ok: true } } = {}) {
  const toHost = new PassThrough()
  const toDaemon = new PassThrough()
  const daemon = new Bridge({ input: toDaemon, output: toHost, defaultDeadlineMs: 5_000 })
  const host = new Bridge({ input: toHost, output: toDaemon, idPrefix: 'r-', peerIdPrefix: 'n-' })
  host.on('pane.claim', () => claim)
  t.after(() => {
    daemon.close()
    host.close()
  })
  return daemon
}

/**
 * Pi's side of its inbox: ConsensFlow's extension takes each record in turn
 * and answers it at its acknowledgement path, as `answer` says.
 */
async function piWindow(t, answer) {
  const root = await mkdtemp(join(tmpdir(), 'cf-contract-pi-'))
  const inbox = join(root, 'inbox')
  const ack = join(root, 'ack')
  let pass = Promise.resolve()
  let failed = null
  const extension = setInterval(() => {
    pass = pass
      .then(async () => {
        const names = await readdir(inbox).catch(() => [])
        for (const name of names.filter((n) => n.endsWith('.json'))) {
          const { id } = JSON.parse(await readFile(join(inbox, name), 'utf8'))
          await mkdir(ack, { recursive: true })
          await answer(join(ack, `${id}.json`), id)
          await rm(join(inbox, name), { force: true })
        }
      })
      .catch((cause) => {
        failed ??= cause
      })
  }, 5)
  t.after(async () => {
    clearInterval(extension)
    await pass
    await rm(root, { recursive: true, force: true })
    if (failed !== null) throw failed
  })
  return { root, inbox, ack }
}

async function sendThroughPi(
  t,
  { answer = async () => {}, claim, ackTimeoutMs = 5_000, inbox } = {},
) {
  const window = await piWindow(t, answer)
  return sendPi(
    {
      session: 'cf-1-zeus-0000abcd',
      pane: 'p1-zeus',
      generation: 3,
      bridge: paneHost(t, { claim }),
      launch: {
        channel: {
          kind: 'pi-extension',
          launchId: 'launch-pi',
          inbox: inbox === undefined ? window.inbox : await inbox(window),
          ack: window.ack,
          ackTimeoutMs,
        },
      },
    },
    'the cache key is per conversation',
  )
}

const acknowledge = (fields) => async (path, id) =>
  writeFile(path, JSON.stringify({ id, ...fields }))

const CHANNELS = {
  'Pi, through its extension inbox': {
    refused: {
      'the pane host refuses its claim': (t) =>
        sendThroughPi(t, { claim: { ok: false, error: 'pane p1-zeus is gone' } }),
      'its inbox cannot be written': (t) =>
        sendThroughPi(t, {
          inbox: async (window) => {
            const file = join(window.root, 'not-a-folder')
            await writeFile(file, '')
            return file
          },
        }),
      'the extension refuses it before sending': (t) =>
        sendThroughPi(t, {
          answer: acknowledge({ admitted: false, bytesWritten: 0, reason: 'chief busy' }),
        }),
    },
    uncertain: {
      // The case a loaded CI runner hit (2026-10-02): something threw while
      // the channel waited for the acknowledgement, after the record was in.
      'an error after the inbox rename': (t) =>
        sendThroughPi(t, { answer: (path) => mkdir(path, { recursive: true }) }),
      'no acknowledgement before the record expires': (t) =>
        sendThroughPi(t, { ackTimeoutMs: 100 }),
    },
    accepted: {
      'the extension shows it in Pi': (t) =>
        sendThroughPi(t, { answer: acknowledge({ admitted: true, mode: 'tui' }) }),
    },
  },
}

describe("one contract for every channel's send", () => {
  for (const [channel, rows] of Object.entries(CHANNELS)) {
    describe(channel, () => {
      it('expresses every row', () => {
        assert.deepEqual(Object.keys(rows), Object.keys(ROWS))
      })
      for (const [row, ways] of Object.entries(rows)) {
        for (const [way, send] of Object.entries(ways)) {
          it(`${ROWS[row].says}: ${way}`, async (t) => {
            const sent = await send(t)
            assert.equal(sent.admitted, ROWS[row].admitted, JSON.stringify(sent))
            if (row === 'refused') assert.equal(sent.bytesWritten, 0, JSON.stringify(sent))
            assert.equal(admission(sent, 'refused').admitted, ROWS[row].admitted)
          })
        }
      }
    })
  }
})
