import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { busyProject, deliver, staff, withLedger } from './ledger-fixtures.mjs'

/** Projects and who is in them (src/ledger/projects.js, src/ledger/staff.js). */

describe('deleting a project', () => {
  it('deletes a closed project with everything in it, and refuses an open one', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const { message } = ledger.createTask(project.id, {
        from: 'chief',
        to: 'zeus',
        body: 'Parser',
      })
      deliver(ledger, message)
      ledger.ask(project.id, { from: 'zeus', to: 'chief', task: 1, body: 'Which?' })
      const other = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        chief: { harness: 'pi' },
      })
      const leadId = id('chief')
      assert.throws(() => ledger.deleteProject(project.id), { code: 'project-open' })
      ledger.setProjectState(project.id, 'suspended')
      const gone = ledger.deleteProject(project.id)
      assert.deepEqual(
        [
          gone.id,
          gone.name,
          gone.directory,
          gone.members,
          gone.sessions,
          gone.tasks,
          gone.messages,
        ],
        [project.id, 'app', '/work/app', 2, 0, 1, 2],
        'what went, for the trace',
      )
      assert.match(gone.createdAt, /^\d{4}-/)
      assert.deepEqual(
        ledger.projects().map((p) => p.name),
        ['other'],
        'the other project is untouched',
      )
      assert.equal(ledger.project(project.id), null)
      assert.throws(() => ledger.deleteProject(project.id), { code: 'unknown-project' })
      assert.equal(ledger.events(other.id).length > 0, true)
      assert.deepEqual(ledger.events(project.id), [], 'nothing of it is left')
      assert.equal(ledger.task(project.id, 1), null)
      assert.equal(ledger.inbox(leadId).length, 0, 'its messages went with it')
    })
  })

  it("gives none of a deleted project's ids to the next one, though they were the highest", async () => {
    await withLedger((ledger) => {
      busyProject(ledger, '/work/app')
      // The highest ids of every table: the ones SQLite alone would give again.
      const gone = busyProject(ledger, '/work/site')
      ledger.setProjectState(gone.project[0], 'suspended')
      ledger.deleteProject(gone.project[0])
      const next = busyProject(ledger, '/work/api')
      for (const [table, ids] of Object.entries(next)) {
        assert.deepEqual(
          ids.filter((id) => id <= Math.max(...gone[table])),
          [],
          `every ${table} id of the new project is past the deleted one's`,
        )
      }
    })
  })
})

describe('projects and participants', () => {
  it('starts a project with the human and its chief, each with a lane', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'claude-code' },
      })
      assert.equal(project.state, 'open')
      assert.equal(project.resumeOnStart, false)
      assert.deepEqual(
        project.participants.map((p) => [p.handle, p.role, p.harness]),
        [
          ['human', 'human', null],
          ['chief', 'chief', 'claude-code'],
        ],
      )
      // A chief runs where a Switch lead could take it: a harness with a terminal.
      for (const harness of ['kimi', 'image', 'nope', undefined]) {
        assert.throws(
          () => ledger.createProject({ directory: '/work/site', name: 'site', chief: { harness } }),
          { code: 'invalid-harness' },
          String(harness),
        )
      }
      assert.equal(ledger.projects().length, 1)
    })
  })

  it('starts a project whose lead runs on the saved agent it is given, and refuses what is no agent id', async () => {
    await withLedger((ledger) => {
      const project = ledger.createProject({
        directory: '/work/app',
        name: 'app',
        chief: { harness: 'codex', agent: 'astraeus' },
      })
      const chief = project.participants.find((p) => p.handle === 'chief')
      assert.deepEqual([chief.harness, chief.agent], ['codex', 'astraeus'])
      for (const agent of ['no such!', '', 42]) {
        assert.throws(
          () =>
            ledger.createProject({
              directory: '/work/site',
              name: 'site',
              chief: { harness: 'pi', agent },
            }),
          { code: 'invalid-agent' },
          String(agent),
        )
      }
      assert.equal(ledger.projects().length, 1)
    })
  })

  it('lets a member hold several roles, and asks for members by any of them', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const both = ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        roles: ['worker', 'reviewer'],
        tier: 'standard',
      })
      assert.deepEqual(
        [both.role, both.roles],
        ['worker', ['worker', 'reviewer']],
        'the first role leads',
      )
      assert.ok(ledger.members(project.id, 'reviewer').some((m) => m.handle === 'hera'))
      assert.ok(ledger.members(project.id, 'worker').some((m) => m.handle === 'hera'))
      assert.deepEqual(
        ledger.project(project.id).participants.find((p) => p.handle === 'zeus').roles,
        ['worker'],
        'one role given as before reads as a set of one',
      )
      const changed = ledger.setRoles(project.id, 'hera', ['reviewer'])
      assert.deepEqual([changed.role, changed.roles], ['reviewer', ['reviewer']])
      assert.ok(!ledger.members(project.id, 'worker').some((m) => m.handle === 'hera'))
      assert.throws(() => ledger.setRoles(project.id, 'hera', []), { code: 'invalid-role' })
      assert.throws(() => ledger.setRoles(project.id, 'hera', ['chief']), { code: 'invalid-role' })
      assert.throws(() => ledger.setRoles(project.id, 'chief', ['worker']), {
        code: 'not-a-member',
      })
      assert.deepEqual(ledger.lastStaff().find((m) => m.agent === 'hera').roles, ['reviewer'])
    })
  })

  it("refreshes each member's tier from its saved agent, sessions included, and leaves an unknown agent alone", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body: 'P' })
      ledger.assignTask(project.id, 1, id('zeus'))
      const changed = ledger.refreshMemberTiers((agent) => ({ zeus: 'complex' })[agent] ?? null)
      assert.deepEqual(changed, [
        { project: project.id, handle: 'zeus', from: 'standard', to: 'complex' },
      ])
      const tiers = Object.fromEntries(
        ledger.project(project.id).participants.map((p) => [p.handle, p.tier]),
      )
      assert.deepEqual(
        [tiers.zeus, tiers['zeus-amber-pine'], tiers.diana],
        ['complex', 'complex', 'standard'],
        "the session follows its member; diana's agent is unknown to the roster here",
      )
      assert.deepEqual(ledger.events(project.id).findLast((e) => e.kind === 'member.tier').data, {
        handle: 'zeus',
        from: 'standard',
        to: 'complex',
      })
      assert.deepEqual(
        ledger.refreshMemberTiers(() => 'complex'),
        [{ project: project.id, handle: 'diana', from: 'standard', to: 'complex' }],
      )
    })
  })

  it('tells no coordinator about a member joining, only a requester about work cancelled by one leaving', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const chief =
        ledger.currentConversation(id('chief')) ??
        ledger.startConversation(id('chief'), { harness: 'claude-code' })
      assert.ok(chief)
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      assert.equal(ledger.inbox(id('chief')).length, 0, 'no joining note')
      ledger.removeMember(project.id, 'hera')
      assert.equal(ledger.inbox(id('chief')).length, 0, 'nothing cancelled, nothing to say')
      ledger.addMember(project.id, {
        agent: 'hera',
        harness: 'pi',
        role: 'worker',
        tier: 'standard',
      })
      const given = ledger.createTask(project.id, { from: 'chief', to: 'hera', body: 'Lexer' })
      deliver(ledger, given.message)
      ledger.removeMember(project.id, 'hera')
      const notes = ledger.inbox(id('chief')).filter((m) => m.kind === 'note')
      assert.equal(notes.length, 1)
      assert.match(
        notes[0].body,
        /^@hera left the staff; it takes no more tasks\. Cancelled with it: T-1\.$/,
      )
    })
  })

  it('adds members once each', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      assert.throws(
        () =>
          ledger.addMember(project.id, {
            agent: 'zeus',
            harness: 'pi',
            role: 'worker',
            tier: 'standard',
          }),
        { code: 'member-exists' },
      )
      assert.throws(
        () => ledger.addMember(project.id, { agent: 'hera', harness: 'pi', role: 'chief' }),
        { code: 'invalid-role' },
      )
      // Kimi left ConsensFlow (2026-10): no member runs on it.
      for (const harness of ['emacs', 'kimi']) {
        assert.throws(
          () =>
            ledger.addMember(project.id, {
              agent: 'hera',
              harness,
              role: 'worker',
              tier: 'standard',
            }),
          { code: 'invalid-harness' },
          harness,
        )
      }
      ledger.addMember(project.id, {
        agent: 'athena',
        harness: 'opencode',
        role: 'advisor',
        tier: 'standard',
      })
      assert.deepEqual(
        ledger.project(project.id).participants.map((p) => p.handle),
        ['human', 'chief', 'zeus', 'diana', 'athena'],
      )
    })
  })

  it('reuses the previous project staff for the next project', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const iris = { agent: 'iris', harness: 'image', role: 'designer', tier: 'standard' }
      ledger.addMember(project.id, iris)
      assert.deepEqual(ledger.lastStaff(), [
        { agent: 'zeus', harness: 'claude-code', role: 'worker', roles: ['worker'] },
        { agent: 'diana', harness: 'codex', role: 'worker', roles: ['worker'] },
        { agent: 'iris', harness: 'image', role: 'designer', roles: ['designer'] },
      ])
      // A project whose staff is its image designer alone is the last staff too.
      ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'claude-code' },
        staff: [iris],
      })
      assert.deepEqual(
        ledger.lastStaff().map((member) => member.agent),
        ['iris'],
      )
    })
  })

  it('marks the projects that were open for resume after a restart, once', async () => {
    await withLedger((ledger) => {
      const open = staff(ledger).project
      const suspended = ledger.createProject({
        directory: '/work/other',
        name: 'other',
        chief: { harness: 'pi' },
      })
      ledger.setProjectState(suspended.id, 'suspended')

      assert.deepEqual(
        ledger.suspendForRestart().map((project) => project.id),
        [open.id],
      )
      assert.deepEqual(
        ledger.projects().map((s) => [s.id, s.state, s.resumeOnStart]),
        [
          [open.id, 'suspended', true],
          [suspended.id, 'suspended', false],
        ],
      )
      ledger.setProjectState(open.id, 'open')
      assert.equal(ledger.project(open.id).resumeOnStart, false)
      ledger.setProjectState(open.id, 'suspended')
      assert.equal(ledger.project(open.id).resumeOnStart, false)
    })
  })

  it('opens or suspends a project by hand and nothing else, and forgets a resume once it was tried', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      assert.throws(() => ledger.setProjectState(project.id, 'closed'), { code: 'invalid-state' })
      ledger.suspendForRestart()
      assert.equal(ledger.project(project.id).resumeOnStart, true)
      ledger.forgetResume(project.id)
      const after = ledger.project(project.id)
      assert.deepEqual(
        [after.state, after.resumeOnStart],
        ['suspended', false],
        'the mark goes; the project stays as it was',
      )
      assert.throws(() => ledger.forgetResume(99), { code: 'unknown-project' })
    })
  })
})
