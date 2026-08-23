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
