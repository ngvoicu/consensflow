import assert from 'node:assert/strict'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * What the tests of tiered dispatch share: four fake agents on the fake
 * `claude`, and a project opened as the New project dialog opens one.
 */

/** Four fake agents on the fake `claude`: the chief, two workers on one model, a reviewer on another. */
export function staff(app) {
  writeFileSync(
    join(app.env.CONSENSFLOW_HOME, 'agents.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      agents: [
        { id: 'chief', kind: 'claude-code', model: 'fake-chief' },
        { id: 'worker', kind: 'claude-code', model: 'fake' },
        { id: 'worker2', kind: 'claude-code', model: 'fake' },
        { id: 'checker', kind: 'claude-code', model: 'fake-2' },
      ],
    })}\n`,
  )
}

/** A project opened as the New project dialog opens one: the staff and the approval setting together. */
export async function project(app, { gate, members }) {
  const opened = await app.requestNode('project.open', {
    directory: app.workspace,
    agent: 'chief',
    ...(gate === undefined ? {} : { gate }),
    staff: members.map(([agent, role]) => ({ agent, roles: [role] })),
  })
  assert.equal(opened.ok, true, JSON.stringify(opened))
  const id = opened.project.id
  const board = async () => (await app.requestNode('board.get', { project: id })).board
  const tiers = Object.fromEntries(
    (await board()).lanes
      .filter((lane) => lane.participant.agent !== null)
      .map((lane) => [lane.participant.agent, lane.participant.tier]),
  )
  const task = async (number) =>
    (await app.requestNode('task.get', { project: id, task: number })).task
  // A member's work runs in a session of its own: its lane is the session's,
  // the newest one when it has had several.
  const lane = async (handle) =>
    (await board()).lanes.findLast(
      (candidate) =>
        candidate.participant.member === handle || candidate.participant.handle === handle,
    )
  const inbox = async (participant) =>
    (await app.requestNode('inbox.get', { project: id, participant })).messages
  return { id, tiers, board, task, lane, inbox }
}
