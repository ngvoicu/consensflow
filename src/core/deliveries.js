import { deliveryText, markerOf } from './delivery-text.js'

/** An Enter, pressed once more for a paste its window did not send. */
const ENTER = 13
/** How long a paste may go unshown in the record before its window gets that Enter. */
const ENTER_AGAIN_MS = 10_000
/** How long the window must have printed nothing first: one being typed into, or drawing, is left be. */
const ENTER_AGAIN_QUIET_MS = 3_000

/**
 * Delivery, for the dispatcher (`dispatcher.js`): a message goes into its
 * window through the harness's native path and counts once the harness's
 * own record shows its header; one refused, or never shown in time, is
 * tried again while attempts remain, then fails and says so to whoever
 * waits on it; and what was on its way when the daemon stopped is settled
 * at the next start. What comes back out is here too: a worker's answer to
 * its task's latest message is the task's result.
 *
 * It keeps the delivery's part of a participant's record
 * (`runtime.delivery`): the message on its way in, and the one held by a
 * window not ready for a paste. The window's part it only reads.
 */
export class Deliveries {
  #ledger
  #host
  #adapters
  #arrivalTimeoutMs
  #launchTimeoutMs
  #maxAttempts
  /** Tells the trace what happened at a window: here, a paste it was not ready for. */
  #traceWindow
  /** Closes a window as its owner does: here, one whose first message never showed. */
  #retire
  /** Whether a participant's record was forgotten while its delivery waited. */
  #forgotten
  #now
  #changed

  constructor({
    ledger,
    host,
    adapters,
    arrivalTimeoutMs,
    launchTimeoutMs,
    maxAttempts,
    traceWindow,
    retire,
    forgotten,
    now,
    changed,
  }) {
    this.#ledger = ledger
    this.#host = host
    this.#adapters = adapters
    this.#arrivalTimeoutMs = arrivalTimeoutMs
    this.#launchTimeoutMs = launchTimeoutMs
    this.#maxAttempts = maxAttempts
    this.#traceWindow = traceWindow
    this.#retire = retire
    this.#forgotten = forgotten
    this.#now = now
    this.#changed = changed
  }

  async deliver(runtime, message) {
    // A participant forgotten while its delivery waits (it left, or its
    // project was deleted) is handed nothing, and what its harness did with
    // the message settles nothing: its window goes with the record, and the
    // message may be gone with its project.
    if (runtime.window.adapter.ready !== undefined) {
      const ready = await runtime.window.adapter.ready({
        launch: runtime.window.launch,
        pane: runtime.window.pane,
        host: this.#host,
      })
      if (this.#forgotten(runtime)) return
      // Withdrawn while the window got ready (its task cancelled or paused
      // meanwhile): it is handed nothing, and nothing fails.
      if (this.#ledger.message(message.id)?.state !== 'queued') return
      if (ready !== true) {
        // Said once per message, so a wait is in the trace, not a mystery.
        if (runtime.delivery.held !== message.id) {
          runtime.delivery.held = message.id
          this.#traceWindow(runtime, 'delivery.held', {
            message: message.id,
            reason: `the window is not ready for a paste: ${typeof ready === 'string' ? ready : 'a paste is on its way'}`,
          })
        }
        return
      }
    }
    runtime.delivery.held = null
    this.#ledger.beginDelivery(message.id)
    const { pane } = runtime.window
    let outcome
    try {
      outcome = await runtime.window.adapter.deliver({
        launch: runtime.window.launch,
        pane,
        host: this.#host,
        text: deliveryText(message),
      })
    } catch (cause) {
      // Uncertain is for a harness that may have taken it; an adapter that
      // throws never handed it over, and its error must show.
      outcome = { admitted: false, reason: `the delivery failed: ${cause.message}` }
    }
    if (this.#forgotten(runtime)) return
    const delivering = {
      messageId: message.id,
      marker: markerOf(message.id),
      since: this.#now(),
      launch: false,
      // The harness's own queue took it (a peer inbox, a broker, a plugin):
      // it shows when the harness gets to it, and sending it again would only
      // make a duplicate, which Claude even drops as a repeat.
      queued: outcome.queued === true,
    }
    if (outcome.admitted === false) {
      this.settleFailure(delivering, outcome.reason ?? 'the harness refused it', { retry: true })
      return
    }
    // A window that closed while its harness took the message had nothing
    // on its way to settle when it went: the message goes again.
    if (runtime.window.pane !== pane) {
      this.settleFailure(delivering, 'its window closed while it was handed over', {
        retry: true,
      })
      return
    }
    runtime.delivery.delivering = delivering
    this.#changed()
  }

  /**
   * Whether the message on its way shows in the window's record; it is
   * confirmed if so. Only what the window was given counts: a tool's output
   * that prints a header (cf inbox read, a log) proves nothing arrived. One
   * withdrawn on its way (its task cancelled, or taken back from the window)
   * is waited for no longer, and stays withdrawn whether it shows or not.
   */
  confirmArrival(runtime, observed) {
    const { delivering } = runtime.delivery
    if (this.#ledger.message(delivering.messageId)?.state !== 'delivering') {
      runtime.delivery.delivering = null
      return true
    }
    const arrived = observed.items.find(
      (item) => item.role === 'user' && item.text.includes(delivering.marker),
    )
    if (arrived === undefined) return false
    this.#ledger.confirmDelivery(delivering.messageId, { item: arrived.id })
    runtime.delivery.delivering = null
    this.#changed()
    return true
  }

  async watchArrival(runtime, observed) {
    if (this.confirmArrival(runtime, observed)) return
    const { delivering } = runtime.delivery
    const waited = this.#now() - delivering.since
    if (delivering.launch) {
      // The lead's window is the human's: however long it takes to show its
      // first message (the handoff), it is never closed for that.
      if (delivering.chief || waited <= this.#launchTimeoutMs) return
      runtime.delivery.delivering = null
      const closing = this.#retire(runtime)
      this.settleFailure(delivering, 'the window never showed its first message', { retry: false })
      await closing
      return
    }
    if (delivering.queued) return
    if (waited <= this.#arrivalTimeoutMs) {
      if (waited > ENTER_AGAIN_MS && !delivering.enteredAgain) {
        await this.#enterAgain(runtime, delivering)
      }
      return
    }
    runtime.delivery.delivering = null
    // A paste the harness record never showed after the whole window did not
    // land: sending it again is how it reaches the reader, and the header
    // would show a late duplicate. (A message the harness queued itself waits
    // for the record, or for the window to close.)
    this.settleFailure(delivering, 'the harness record never showed it', { retry: true })
  }

  /**
   * One more Enter, for a paste its window has not sent: an Enter that came
   * before the window had read the paste finds nothing to send (Devin draws a
   * long paste's placeholder only once it has read all of it), and the text
   * waits in the input, where a second paste would only stack a copy beside
   * it. An empty input takes an Enter as nothing. Pressed once, and only into
   * a window quiet for a while; the next look tries again.
   */
  async #enterAgain(runtime, delivering) {
    const { pane } = runtime.window
    if (pane === null) return
    delivering.enteredAgain = true
    const snapshot = await this.#host.request('pane.snapshot', pane).catch(() => null)
    if (snapshot?.ok !== true || !(snapshot.outputQuietMs >= ENTER_AGAIN_QUIET_MS)) {
      delivering.enteredAgain = false
      return
    }
    await this.#host.request('pane.input', { ...pane, bytes: [ENTER] }).catch(() => {})
    this.#traceWindow(runtime, 'delivery.enter_again', { message: delivering.messageId })
  }

  /** A worker's answer to its task's latest message finishes the task. */
  collect(project, participant, observed) {
    const task = this.#ledger.activeTask(participant.id)
    if (task === null || task.state !== 'working') return
    const latest = task.messages
      .filter(
        (message) =>
          message.recipient === participant.handle &&
          message.state === 'delivered' &&
          (message.kind === 'task' || message.kind === 'answer'),
      )
      .at(-1)
    if (latest === undefined) return
    const start = observed.items.findIndex((item) => item.text.includes(markerOf(latest.id)))
    if (start === -1) return
    if (observed.failed) {
      this.#failTask(project, task, `@${participant.handle}'s harness reported a failure`)
      return
    }
    if (!observed.settled) return
    // The turn is over once its last message is complete (the one that ended
    // it; a message that ended in a tool call never is). The result is
    // everything the member wrote in it, in order: a report written before a
    // last command, then "Committed.", is not the last word alone. A tool's
    // output is not the member's words, and nor are the progress notes a
    // harness marks as its commentary (Codex's "I'll read the diff…"): its
    // final answer is the report.
    const written = observed.items.slice(start + 1).filter((item) => item.role === 'assistant')
    if (written.at(-1)?.complete !== true) return
    const body =
      written
        .filter((item) => item.commentary !== true)
        .map((item) => item.text.trim())
        .filter((text) => text !== '')
        .join('\n\n') || '(the agent ended its turn without a written answer)'
    this.#ledger.recordResult(project.id, task.number, { body })
    this.#changed()
  }

  /**
   * What a window was to receive goes back to its queue with its attempt:
   * the window went before it had the chance to land.
   */
  giveBack(delivering, reason) {
    if (this.#ledger.message(delivering.messageId)?.state !== 'delivering') return
    this.#ledger.retryDelivery(delivering.messageId, reason, { refund: true })
  }

  /** A delivery that did not arrive: try again while attempts remain, or give up. */
  settleFailure(delivering, reason, { retry }) {
    const message = this.#ledger.message(delivering.messageId)
    if (message === null || message.state !== 'delivering') return
    if (retry && message.attempts < this.#maxAttempts) {
      this.#ledger.retryDelivery(message.id, reason)
    } else this.#failDelivery(message, reason)
    this.#changed()
  }

  /**
   * A message that will not be delivered. A brief fails its task, and whoever
   * gave it hears why. Whoever waits on any other kind (a worker on its
   * answer, the chief on a result) would wait forever, so the human hears,
   * once, what it was, for whom and why.
   */
  #failDelivery(message, reason) {
    this.#ledger.failDelivery(message.id, reason)
    const project = this.#ledger.project(message.projectId)
    if (message.kind === 'task' && message.taskNumber !== null) {
      const task = this.#ledger.task(project.id, message.taskNumber)
      if (task.state === 'failed') {
        this.#tellRequester(project, task, reason)
        // A task the human gave: that note was theirs.
        if (task.requester === 'human') return
      }
    }
    const kind = message.kind === 'answer' ? 'an answer' : `a ${message.kind}`
    const from = message.sender === null ? 'ConsensFlow' : `@${message.sender}`
    const on = message.taskNumber === null ? '' : ` on T-${message.taskNumber}`
    this.#ledger.note(project.id, {
      to: 'human',
      ...(message.taskNumber === null ? {} : { task: message.taskNumber }),
      body: `m-${message.id}, ${kind} from ${from}${on}, did not reach @${message.recipient}: ${reason}.`,
    })
  }

  #failTask(project, task, reason) {
    this.#ledger.failTask(project.id, task.number, { reason })
    this.#tellRequester(project, task, reason)
  }

  #tellRequester(project, task, reason) {
    this.#ledger.note(project.id, {
      to: task.requester,
      task: task.number,
      body: `T-${task.number} failed: ${reason}. Reopen it with: cf task reopen T-${task.number} "…"`,
    })
  }

  /**
   * What was on its way to a window when the previous process ended: no
   * window survives a restart, so none will show it now. A message whose
   * header ConsensFlow's copy of the window shows had arrived, and so had one
   * the harness's own record shows: the copy can lag the record, and a
   * message the harness took must not go again. Any other goes back to its
   * queue with its attempt, for the window that comes back, as does one
   * whose record cannot be read.
   */
  async settleInFlight() {
    for (const message of this.#ledger.inFlight()) {
      const marker = markerOf(message.id)
      const item =
        this.#ledger.copiedItemWith(message.recipientId, marker) ??
        (await this.#recordedItemWith(message.recipientId, marker))
      if (item === null) {
        this.#ledger.retryDelivery(message.id, 'the daemon stopped before it arrived', {
          refund: true,
        })
      } else this.#ledger.confirmDelivery(message.id, { item })
    }
  }

  /**
   * The first item a participant's harness recorded it was given (a user
   * item) that holds `marker`, read with no window open: null when none does,
   * or when its record cannot be read.
   */
  async #recordedItemWith(participantId, marker) {
    const conversation = this.#ledger.currentConversation(participantId)
    if (!conversation?.nativeSession) return null
    const adapter = this.#adapters[conversation.harness]
    if (adapter?.record === undefined) return null
    const record = await adapter.record({ conversation }).catch(() => null)
    const items = Array.isArray(record?.items) ? record.items : []
    return items.find((item) => item.role === 'user' && item.text.includes(marker))?.id ?? null
  }
}
