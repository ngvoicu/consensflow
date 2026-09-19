/**
 * The page's half of the packaged smoke (TEST-PANE-45, on the new core).
 *
 * This module is only ever imported when Rust put `__CONSENSFLOW_SELFTEST__`
 * on the window, which it only does when the app was started with
 * `CONSENSFLOW_SELFTEST=1`. In an ordinary launch the file ships but nothing
 * loads it.
 *
 * It drives the REAL page: the same `project.open` a human's New project
 * runs, the same emulator registry that draws the docked window, the same
 * input path a keystroke takes, and the board's own way of giving the lead a
 * task. Nothing here reimplements a production path — a smoke that drove its
 * own copy of the page would prove only that the copy works.
 *
 * What it reports is what it could SEE: rows out of the live xterm buffer,
 * the child's own hex of what was typed, the ack count the page really sent.
 */

import { runUpdateSelftest } from './update-selftest.js'

const READY = /CFSMOKE-READY (\S+)/
const TOOLS = /CFSMOKE-TOOLS (\S+)/
const FLOODED = /CFSMOKE-FLOODED (\S+)/
const FLOOD = /CFSMOKE-FLOOD (\d+) /
const HEX = /CFSMOKE-HEX ([0-9a-f]+)/

const STEP_MS = 45_000

function sleep(ms) {
  return new Promise((wake) => setTimeout(wake, ms))
}

/**
 * Every line the emulator has, scrollback included. A line the terminal
 * wrapped over several rows is one line here: what the child printed is the
 * proof, not how many columns the window happened to have.
 */
function screen(emulator) {
  const buffer = emulator?.terminal?.buffer?.active
  if (buffer === undefined) return []
  const lines = []
  for (let row = 0; row < buffer.length; row += 1) {
    const line = buffer.getLine(row)
    const text = line?.translateToString(true) ?? ''
    if (line?.isWrapped && lines.length > 0) lines[lines.length - 1] += text
    else lines.push(text)
  }
  return lines
}

/**
 * Polls a condition against the live page rather than waiting on an event.
 *
 * The thing being proved is what the human would see on screen, and the only
 * honest source for that is the buffer the renderer is reading from.
 */
async function until(what, check, { timeoutMs = STEP_MS, note = null } = {}) {
  const deadline = Date.now() + timeoutMs
  let polls = 0
  for (;;) {
    const found = check()
    if (found !== null && found !== undefined && found !== false) return found
    if (Date.now() > deadline) throw new Error(`${what} did not happen within ${timeoutMs} ms`)
    polls += 1
    if (note !== null && polls % 30 === 0) await note(what, polls)
    await sleep(100)
  }
}

function toHex(text) {
  return [...new TextEncoder().encode(text)]
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('')
}

export async function runSelftest({
  config,
  invoke,
  core,
  refresh,
  registry,
  sendInput,
  onAck,
  onOutput,
}) {
  if (
    typeof config?.updaterExpectedVersion === 'string' &&
    config.updaterExpectedVersion.length > 0
  ) {
    await runUpdateSelftest({ config, invoke, core, refresh })
    return
  }

  let acks = 0
  let arrivals = 0
  let arrivedBytes = 0
  onAck(() => {
    acks += 1
  })
  onOutput((message) => {
    arrivals += 1
    arrivedBytes += message?.bytes?.length ?? 0
  })

  const report = async (event, data) => {
    await invoke('selftest_report', { event, data })
  }

  try {
    await report('boot', {
      protocol: location.protocol,
      // Everything the document pulled in. The agents screens are not here on
      // purpose: they are the one thing that legitimately comes over HTTP,
      // from the daemon, and they have no `src` until opened.
      assets: [
        ...[...document.querySelectorAll('script[src]')].map((tag) => tag.src),
        ...[...document.querySelectorAll('link[rel="stylesheet"]')].map((tag) => tag.href),
      ],
    })

    // A project on the smoke's folder, its lead on the fake `claude`.
    const opened = await core('project.open', {
      directory: config.dir,
      harness: 'claude-code',
      review: 'none',
    })
    await report('project', opened)
    if (opened?.ok !== true) throw new Error(`project.open refused: ${JSON.stringify(opened)}`)
    await refresh()

    // The lead's window is the one docked beside the board; its emulator is
    // in the page's own registry, keyed `id:generation`.
    const [emulator, pane] = await until('the lead window appeared', () => {
      for (const [key, entry] of registry.emulators) {
        const cut = key.lastIndexOf(':')
        const id = key.slice(0, cut)
        const generation = Number(key.slice(cut + 1))
        if (id.endsWith('-lead') && Number.isInteger(generation)) {
          return [entry.emulator, { id, generation }]
        }
      }
      return null
    })
    const banner = await until(
      'the harness banner rendered',
      () => screen(emulator).find((row) => READY.test(row)) ?? null,
      {
        note: async (what, polls) => {
          const rows = screen(emulator)
          await report('waiting', {
            what,
            polls,
            acks,
            emulators: registry.emulators.size,
            cols: emulator?.terminal?.cols ?? null,
            terminalRows: emulator?.terminal?.rows ?? null,
            bufferLength: rows.length,
            filled: rows.filter((row) => row.trim().length > 0).slice(0, 5),
            hidden: document.hidden,
            channel: typeof window.__TAURI__?.core?.Channel,
            arrivals,
            arrivedBytes,
          })
        },
      },
    )
    const tools = await until('the pane reported its PATH', () => {
      for (const row of screen(emulator)) {
        const match = TOOLS.exec(row)
        if (match !== null) return match[1]
      }
      return null
    })
    await report('rendered', {
      banner: banner.trim(),
      rows: screen(emulator).length,
      cols: emulator?.terminal?.cols ?? null,
      tools,
      pane,
    })

    // Typed the way a human types it: the page's own input path, the text
    // and then the return, and the child's hex is the only proof it arrived.
    const typed = `cfsmoke-${config.tag}`
    await sendInput(pane, typed)
    await sendInput(pane, '\r')
    const hex = await until(
      'the child echoed the typed line',
      () => {
        for (const row of screen(emulator)) {
          const match = HEX.exec(row)
          if (match !== null && match[1] === toHex(typed)) return match[1]
        }
        return null
      },
      {
        note: async (what, polls) => {
          const rows = screen(emulator).filter((row) => row.trim().length > 0)
          await report('waiting', {
            what,
            polls,
            acks,
            arrivals,
            wanted: toHex(typed),
            hexLines: rows.filter((row) => HEX.test(row)).slice(-3),
            tail: rows.slice(-4),
          })
        },
      },
    )
    await report('echo', { typed, hex })

    // A task from the board: the human's composer, the core, the dispatcher,
    // the pane host's paste, the child. The child's hex of the header line is
    // the proof that the board reaches a window; the core's own confirmation,
    // read back from the record the fake harness keeps, is the proof that it
    // knows it did.
    const given = await core('task.add', { project: opened.project.id, to: 'lead', body: 'SMOKE' })
    if (given?.ok !== true) throw new Error(`task.add refused: ${JSON.stringify(given)}`)
    const header = `[ConsensFlow m-${given.message?.id ?? given.task.number} ·`
    const delivered = await until(
      'the task reached the lead window',
      () => {
        for (const row of screen(emulator)) {
          const match = HEX.exec(row)
          if (match?.[1].startsWith(toHex(header))) return match[1]
        }
        return null
      },
      {
        note: async (what, polls) => {
          await report('waiting', {
            what,
            polls,
            wanted: toHex(header),
            hexLines: screen(emulator)
              .filter((row) => HEX.test(row))
              .slice(-3),
          })
        },
      },
    )
    const confirmedBy = Date.now() + STEP_MS
    let state = null
    while (state !== 'delivered') {
      if (Date.now() > confirmedBy) {
        throw new Error(`the core did not confirm the delivery (the message is ${state})`)
      }
      await sleep(200)
      const { task } = await core('task.get', {
        project: opened.project.id,
        task: given.task.number,
      })
      state = task.messages.find((message) => message.kind === 'task')?.state ?? null
    }
    await report('board', { task: given.task.number, hex: delivered, delivered: true })

    // Exercise a large Unicode paste through WebKit, IPC and the real PTY.
    await sendInput(pane, 'BIGPASTE\r')
    await until('raw paste reader ready', () =>
      screen(emulator).some((row) => row.includes('CFSMOKE-PASTE-READY')),
    )
    await sendInput(pane, `\x1b[200~${'漢字 résumé 🙂\r'.repeat(30_000)}\x1b[201~`)
    const pasted = await until('complete large paste reached the child', () => {
      for (const row of screen(emulator)) {
        const match = /CFSMOKE-PASTE (\d+) ([A-Za-z0-9+/=]+)/.exec(row)
        if (match) return { bytes: Number(match[1]), hash: match[2] }
      }
      return null
    })
    await report('large-paste', pasted)

    // Only now the flood, and only because it is asked for: it is bigger than
    // the unacked-output window, so its last line can be on screen only if
    // the page kept returning credit through `pane_ack`.
    await sendInput(pane, 'FLOOD')
    await sendInput(pane, '\r')
    const flood = await until(
      'the flood drained',
      () => {
        const rows = screen(emulator)
        if (!rows.some((row) => FLOODED.test(row))) return null
        let last = 0
        for (const row of rows) {
          const match = FLOOD.exec(row)
          if (match !== null) last = Math.max(last, Number(match[1]))
        }
        return { last }
      },
      {
        note: async (what, polls) => {
          await report('waiting', { what, polls, acks, arrivals, arrivedBytes })
        },
      },
    )
    await report('drained', { lastFloodLine: flood.last, acks })
    await report('settled', {
      acks,
      pane,
      terminalPreserved:
        registry.emulators.get(`${pane.id}:${pane.generation}`)?.emulator === emulator,
    })
  } catch (cause) {
    await report('failed', { error: cause instanceof Error ? cause.message : String(cause) })
  }
}
