/**
 * The page's half of the packaged smoke (TEST-PANE-45).
 *
 * This module is only ever imported when Rust put `__CONSENSFLOW_SELFTEST__`
 * on the window, which it only does when the app was started with
 * `CONSENSFLOW_SELFTEST=1`. In an ordinary launch the file ships but nothing
 * loads it.
 *
 * It drives the REAL page: the same `open_lead` a human's New conversation
 * runs, the same emulator registry that draws every pane, the same input path
 * a keystroke takes. Nothing here reimplements a production path — a smoke
 * that drove its own copy of the page would prove only that the copy works.
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

/** Every line the emulator has, scrollback included. */
function screen(emulator) {
  const buffer = emulator?.terminal?.buffer?.active
  if (buffer === undefined) return []
  const lines = []
  for (let row = 0; row < buffer.length; row += 1) {
    lines.push(buffer.getLine(row)?.translateToString(true) ?? '')
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
    // Say what it looked like while waiting. A smoke that only reports the
    // timeout makes the next person launch the app by hand to learn anything.
    if (note !== null && polls % 30 === 0) await note(what, polls)
    await sleep(100)
  }
}

export async function runSelftest({
  config,
  invoke,
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
    await runUpdateSelftest({ config, invoke, refresh })
    return
  }

  let acks = 0
  let arrivals = 0
  let arrivedBytes = 0
  onAck(() => {
    acks += 1
  })
  // Counted where the message LANDS, before the emulator sees it. "Nothing
  // arrived" and "something arrived and stalled" look identical from the
  // screen, and they have nothing in common as bugs.
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
      // Everything the document pulled in. The roster iframe is not here on
      // purpose: it is the one thing that legitimately comes over HTTP, from
      // the local `cf ui`, and it has no `src` until a tab exists.
      assets: [
        ...[...document.querySelectorAll('script[src]')].map((tag) => tag.src),
        ...[...document.querySelectorAll('link[rel="stylesheet"]')].map((tag) => tag.href),
      ],
    })

    // `claude-code` is the canonical kind `Tabs.create` takes; `claude` is
    // only the name of the binary on PATH. Passing the command name here is
    // a refusal, not a launch.
    const opened = await invoke('open_lead', { dir: config.dir, harness: 'claude-code' })
    await report('tab', opened)
    if (opened?.ok !== true) {
      throw new Error(`open_lead refused: ${JSON.stringify(opened)}`)
    }

    // Draw the tab that was just opened. A human gets this from the
    // `state-changed` event; asking for it directly means the smoke does not
    // depend on that event's timing to decide whether panes work.
    await refresh()

    // The pane identity comes from the page's own registry: its keys are
    // `id:generation`, and an emulator exists because the pane is on screen.
    const [emulator, pane] = await until('a pane emulator appeared', () => {
      for (const [key, entry] of registry.emulators) {
        const cut = key.lastIndexOf(':')
        const id = key.slice(0, cut)
        const generation = Number(key.slice(cut + 1))
        if (Number.isInteger(generation)) return [entry.emulator, { id, generation }]
      }
      return null
    })

    const banner = await until(
      'the harness banner rendered',
      () => {
        const line = screen(emulator).find((row) => READY.test(row))
        return line ?? null
      },
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

    // Only now the flood, and only because it is asked for. It is bigger than
    // the unacked-output window, so its last line can be on screen only if the
    // page kept returning credit through `pane_ack`. It also pushes far more
    // rows than xterm keeps, which is exactly why nothing printed BEFORE it
    // can be looked for afterwards — the banner and the echo are already read.
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

    const pm = await invoke('open_pm', { tab: opened.tab, harness: 'claude-code' })
    if (!pm.ok) throw new Error(pm.error ?? 'PM did not open')
    await refresh()
    document.querySelector(`[data-testid="pm-${pm.tab}"]`)?.click()
    const pmEmulator = await until(
      'PM emulator in main registry',
      () => registry.emulators.get(`${pm.pane.id}:${pm.pane.generation}`)?.emulator,
    )
    await runPmSelftest({
      emulator: pmEmulator,
      pane: pm.pane,
      enqueue: (_action, text) => sendInput(pm.pane, text),
      invoke,
    })
    await report('settled', { acks, pane })
  } catch (cause) {
    await report('failed', { error: cause instanceof Error ? cause.message : String(cause) })
  }
}

function toHex(text) {
  return [...new TextEncoder().encode(text)]
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('')
}

/** Exercise the PM through the main window's real emulator and input path. */
async function runPmSelftest({ emulator, pane, enqueue, invoke }) {
  try {
    await until('PM terminal banner', () => screen(emulator).some((row) => READY.test(row)))
    const typed = 'cfsmoke-pm-input'
    await enqueue('input', typed)
    await enqueue('input', '\r')
    const hex = await until('PM input echo', () => {
      for (const row of screen(emulator)) {
        const match = HEX.exec(row)
        if (match?.[1] === toHex(typed)) return match[1]
      }
      return null
    })
    await invoke('selftest_report', { event: 'pm-echo', data: { pane, typed, hex } })
  } catch (error) {
    await invoke('selftest_report', { event: 'failed', data: { error: String(error) } })
  }
}
