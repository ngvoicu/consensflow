import { handoffText, historyPages, isHandoff, lastWords } from './handoff.js'

/**
 * The chief switch, for the dispatcher (`dispatcher.js`): the human's Switch
 * chief moves the chief to a fresh window on another saved agent, at once or
 * once the chief's turn is over (after it wrote down where things stand,
 * when the human asks for that), and the new window's first message is the
 * handoff. A chief whose saved agent is gone does not open; the human hears
 * why and switches it.
 *
 * It keeps the chief switch's part of a participant's record
 * (`runtime.pendingSwitch`): a switch waiting for the chief's turn to end. A
 * switch also starts the new chief clean of the old one's marks in the
 * record's other parts.
 */
export class ChiefSwitch {
  #ledger
  /** The old window's last look and close, and the new window's launch. */
  #windows
  /** What the old window was still receiving, and the note that asks where things stand. */
  #deliveries
  /** The old window's last words, copied before it closes. */
  #transcripts
  /** Starts a launch or a delivery that goes on apart from whoever holds the chief (`dispatcher.js`). */
  #act
  /** Whether the chief's record was forgotten while the switch waited. */
  #forgotten
  #changed

  constructor({ ledger, windows, deliveries, transcripts, act, forgotten, changed }) {
    this.#ledger = ledger
    this.#windows = windows
    this.#deliveries = deliveries
    this.#transcripts = transcripts
    this.#act = act
    this.#forgotten = forgotten
    this.#changed = changed
  }

  /**
   * A switch the human asked for after the chief's turn, with a note that
   * first asks the chief where things stand when they want one: it waits in
   * the record (`awaitSwitch`).
   */
  afterTurn(projectId, runtime, { harness, agent, note }) {
    // A switch asked again replaces the one waiting, with a note not yet sent.
    const replaced = runtime.pendingSwitch?.note ?? null
    if (replaced !== null && this.#ledger.message(replaced).state === 'queued') {
      this.#ledger.cancelMessage(replaced, 'the human asked for the switch again')
    }
    runtime.pendingSwitch = {
      harness,
      agent,
      note: note
        ? this.#ledger.note(projectId, {
            to: 'chief',
            body: `The human is moving this project's chief to ${harness} (${agent}) once you answer. Write down where things stand, for the chief after you: what you and the human decided, what you promised, what you were about to do, and what is unresolved. Do not start anything new.`,
          }).id
        : null,
    }
    this.#changed()
  }

  /**
   * A switch the human asked for after the chief's turn: once the turn is
   * over (and the note asking where things stand came and was answered, when
   * they asked for one), the chief goes. Until then nothing else is delivered
   * to it, so its turn can end.
   */
  async awaitSwitch(project, chief, runtime, observed, idle) {
    if (!idle) return
    const { note } = runtime.pendingSwitch
    const asked = note === null ? null : this.#ledger.message(note)
    if (asked?.state === 'queued') {
      this.#act(runtime, () => this.#deliveries.deliver(runtime, asked))
      return
    }
    if (asked !== null && asked.state === 'delivered') {
      const at = observed.items.findIndex((item) => item.id === asked.receipt?.item)
      const answered = observed.items
        .slice(at + 1)
        .some((item) => item.role === 'assistant' && item.complete === true)
      if (at === -1 || !answered) return
    }
    await this.performSwitch(project, chief, runtime, runtime.pendingSwitch)
  }

  /**
   * The switch itself, holding the chief's turn: one last look at the old
   * window (its words become history; a delivery whose header shows there
   * arrived), what it was still receiving goes back to the queue with its
   * attempt, the window closes without suspending the project, the ledger
   * moves the chief, and the new window opens with the handoff. A project
   * deleted on the way (its chief forgotten) stops it there: its rows are
   * gone, so there is no delivery to confirm and no chief to move, and its
   * old window closes with the record.
   */
  async performSwitch(project, chief, runtime, { harness, agent }) {
    const asked = runtime.pendingSwitch?.note ?? null
    runtime.pendingSwitch = null
    let cut = false
    const { pane } = runtime.window
    if (pane !== null) {
      const observed = await this.#windows.observe(chief, runtime).catch(() => null)
      if (this.#forgotten(runtime)) return
      if (observed !== null) {
        this.#transcripts.copy(chief, runtime, observed)
        if (runtime.delivery.delivering !== null) this.#deliveries.confirmArrival(runtime, observed)
        cut = !observed.settled
      }
      const { delivering } = runtime.delivery
      runtime.delivery.delivering = null
      if (delivering !== null)
        this.#deliveries.giveBack(delivering, 'the chief was switched before it arrived')
      if (!(await this.#windows.closeOwn(runtime, pane))) {
        // The old window would not close: the switch waits, as one asked
        // for after a turn does, and the chief's next step tries it again.
        runtime.pendingSwitch = { harness, agent, note: asked }
        this.#changed()
        return
      }
    }
    if (this.#forgotten(runtime)) return
    // A handoff still on its way is an earlier switch's: this one writes its
    // own. The note asking the old chief where things stand was for it alone,
    // however the switch came (now, or the chief out of quota).
    for (const message of this.#ledger.pending(chief.id)) {
      if (isHandoff(message)) this.#ledger.cancelMessage(message.id, 'the chief was switched again')
      else if (message.id === asked) {
        this.#ledger.cancelMessage(message.id, 'the chief was switched before it came')
      }
    }
    this.#ledger.switchChief(project.id, { harness, agent, cut })
    Object.assign(runtime.quota, { reported: null, lowUntil: null })
    runtime.copied = null
    Object.assign(runtime.window, { interrupted: null, relaunch: null })
    runtime.delivery.held = null
    this.#changed()
    const current = this.#ledger.project(project.id)
    if (current.state !== 'open') return
    const moved = current.participants.find((participant) => participant.role === 'chief')
    this.#act(runtime, () => this.#windows.launch(runtime, current, moved, null))
  }

  /**
   * The handoff a fresh window of the chief starts with when there is a
   * history to hand over, written from the ledger (see `handoff.js`), so it
   * needs no turn of the old chief's; null for a project's first chief.
   */
  #handoff(project, chief) {
    const history = this.#ledger.chiefHistory(project.id)
    if (!history.some((conversation) => conversation.items.length > 0)) return null
    const switched = this.#ledger.lastSwitch(project.id)
    // The page count is cf history's: a line names only this project's messages.
    const message = (id) => {
      const found = this.#ledger.message(id)
      return found?.projectId === project.id ? found : null
    }
    return this.#ledger.note(project.id, {
      to: 'chief',
      body: handoffText({
        from: switched?.from ?? { harness: history.at(-1).harness, agent: null },
        to: { harness: chief.harness, agent: chief.agent },
        open: this.#ledger.chiefOpenWork(project.id),
        last: lastWords(history),
        cut: switched?.cut === true,
        pages: historyPages(history, { message }),
      }),
    })
  }

  /**
   * A chief's first message. A handoff still on its way goes first: the
   * window it was written for never came up, or closed before showing it. A
   * chief that starts fresh with earlier conversations behind it (the human
   * switched it) gets a handoff. Either way, what was queued for it waits
   * for its next turn.
   */
  chiefFirst(project, chief, conversation, message) {
    const handoff = this.#ledger
      .pending(chief.id)
      .find((pending) => isHandoff(pending) && pending.state === 'queued')
    if (handoff !== undefined) return handoff
    if (conversation === null) return this.#handoff(project, chief) ?? message
    return message
  }

  /**
   * A chief whose agent is gone does not open: the human hears why, and what
   * it was to receive waits for the chief they switch in, its attempt given back.
   */
  chiefWithoutAgent(project, chief, delivering) {
    this.#ledger.note(project.id, {
      to: 'human',
      body: `The chief runs on ${chief.agent}, which is no longer among your agents: add it back under Agents, or switch the chief.`,
    })
    if (delivering !== null) {
      this.#deliveries.giveBack(delivering, `${chief.agent} is no longer among your agents`)
    }
    this.#changed()
  }
}
