/**
 * Transcript copying, for the dispatcher (`dispatcher.js`): every look at a
 * window copies into the ledger what is new in its harness's record, and a
 * window the human switched to another conversation is followed there.
 *
 * It keeps the transcript part of a participant's record (`runtime.copied`):
 * which conversation was copied, and how many of its items. Following a
 * conversation also points the window's launch at it.
 */
export class Transcripts {
  #ledger
  #changed
  #wrote

  constructor({ ledger, changed, wrote }) {
    this.#ledger = ledger
    this.#changed = changed
    this.#wrote = wrote
  }

  /**
   * ConsensFlow's own copy of the window's conversation, kept in the home:
   * what the agent was told, wrote and got back from its tools, readable on
   * the card once the window is gone. Each look copies what is new and the
   * item still being written; a record that shrank (a resumed window rewrote
   * it) is copied over from the start. A look that wrote anything says so
   * (`wrote`): a view of what the window did reads it again.
   */
  copy(participant, runtime, observed) {
    const conversation = this.#ledger.currentConversation(participant.id)
    if (conversation === null) return
    const items = observed.items
    const copied = runtime.copied?.conversation === conversation.id ? runtime.copied.count : 0
    const from = items.length < copied ? 0 : Math.max(0, copied - 1)
    if (
      items.length > from &&
      this.#ledger.copyTranscript(conversation.id, items.slice(from), { from }) > 0
    ) {
      this.#wrote()
    }
    runtime.copied = { conversation: conversation.id, count: items.length }
  }

  /**
   * A window the human switched to another conversation (/clear, /new,
   * /resume) is followed: the participant's conversation is the one it shows
   * now, and its launch names it, so later looks, deliveries and the
   * transcript copy go there. A delivery still on its way counts once its
   * header shows in the record the window now writes.
   */
  follow(participant, runtime, nativeSession) {
    this.#ledger.followConversation(participant.id, {
      harness: participant.harness,
      nativeSession,
    })
    runtime.window.launch.nativeSession = nativeSession
    runtime.copied = null
    this.#changed()
  }
}
