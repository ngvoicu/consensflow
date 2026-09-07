import assert from 'node:assert/strict'
import test from 'node:test'
import { createPacket } from '../../hosts/lib/packets.js'

const BASE = { cwd: '/tmp/ws', agent: { id: 'hyperion', kind: 'codex' }, task: 'another one' }

test('packet: the first turn of a conversation sets the scene', async () => {
  const packet = await createPacket({ ...BASE, brief: 'just a joke' })

  assert.match(packet, /# ConsensFlow Packet/)
  assert.match(packet, /Workspace: \/tmp\/ws/)
  assert.match(packet, /## How to work/)
  assert.match(packet, /## Your brief for this run/)
  assert.match(packet, /another one/)
})

test('packet: a later turn says only the new thing', async () => {
  // The agent is in the SAME session — it already has the workspace, the
  // how-to-work and the brief from turn one. Re-sending them every turn buries
  // the actual question, and in an attached window the user watches three
  // screens of boilerplate scroll past to reach one line of joke.
  const packet = await createPacket({ ...BASE, continuing: true })

  assert.doesNotMatch(packet, /## How to work/, 'it already knows how to work')
  assert.doesNotMatch(packet, /Workspace:/, 'it is already in the workspace')
  assert.doesNotMatch(packet, /# ConsensFlow Packet/, 'no ceremony for a follow-up')
  assert.match(packet, /another one/, 'the question survives')
})

test('packet: a later turn still carries a brief or handoff when given', async () => {
  // Those are per-spawn: a follow-up may well have a new brief, and a handoff
  // is the lead's to hand over whenever it wants.
  const packet = await createPacket({
    ...BASE,
    continuing: true,
    brief: 'now be terse',
    handoff: 'earlier the user said X',
  })

  assert.match(packet, /now be terse/)
  assert.match(packet, /earlier the user said X/)
  assert.doesNotMatch(packet, /## How to work/)
})

test('packet: in a conversation, the agent is told it may ask back', async () => {
  // One-shot, a question was useless: the next run was a stranger who had
  // never heard it. In a conversation the lead's reply reaches the same agent
  // with its memory intact, so asking is now the right move when the task is
  // ambiguous — and the agent has to be told that, or it will keep guessing.
  const packet = await createPacket({ ...BASE, conversational: true })

  assert.match(packet, /ask/i)
  assert.match(packet, /same conversation|come back to you|reach you/i)
})

test('packet: a one-shot is not invited to ask, because nobody would answer', async () => {
  const packet = await createPacket(BASE)

  assert.doesNotMatch(packet, /you may ask/i)
})

/**
 * The launch marker, in ONE place.
 *
 * `createWindowSeed` has carried a `nonce` since Phase 2; kimi opens no
 * window and takes its prompt in argv, so its first turn is a packet — and
 * for a while the marker was prepended by the caller instead, which meant
 * the rule for where launch evidence goes lived in two files and a kimi
 * worker could never bind at all. Same argument, same first line, one rule.
 */

/** A follow-up packet carries no timestamp, so it can be pinned to the byte. */
const CONTINUING =
  '## Message from the user\nanother one\n\nRespond directly and conversationally. There is no required format.\n'

test('packet: the launch nonce rides on the packet’s first line', async () => {
  const packet = await createPacket({ ...BASE, nonce: 'abc123' })

  assert.equal(packet.split('\n')[0], '[consensflow launch abc123]')
  assert.match(packet, /# ConsensFlow Packet/, 'the packet is still a packet')
  assert.match(packet, /another one/, 'the question survives')
})

test('packet: no nonce, no marker — byte for byte what it was', async () => {
  const packet = await createPacket({ ...BASE, continuing: true })

  assert.equal(packet, CONTINUING)
  assert.ok(!packet.includes('[consensflow launch'), 'no nonce, no marker')

  for (const nonce of [undefined, null, '', '   ']) {
    assert.equal(
      await createPacket({ ...BASE, continuing: true, nonce }),
      CONTINUING,
      `a ${JSON.stringify(nonce)} nonce is no nonce`,
    )
  }
})

test('packet: the nonce adds the marker line and touches nothing else', async () => {
  const marked = await createPacket({ ...BASE, continuing: true, nonce: 'abc123' })

  assert.equal(marked, `[consensflow launch abc123]\n${CONTINUING}`)

  // The same on a first turn, where only the created-at stamp moves between
  // two calls a millisecond apart.
  const stamp = (text) => text.replace(/^Created: .*$/m, 'Created: <at>')
  const first = await createPacket({ ...BASE, nonce: 'abc123' })
  const bare = await createPacket({ ...BASE })
  assert.equal(stamp(first), `[consensflow launch abc123]\n${stamp(bare)}`)
})

test('packet: a nonce that could not be a marker is refused, not smuggled', async () => {
  await assert.rejects(
    () => createPacket({ ...BASE, nonce: 'two\nlines' }),
    /single line without brackets/,
  )
})
