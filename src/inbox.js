import { randomUUID } from 'node:crypto'
import { answers } from '../hosts/lib/completion.js'
import {
  beginInsertion,
  claimNext,
  observeReceipt,
  registerReceiver,
  releaseClaim,
  resultStatus,
  retireReceiver,
} from '../hosts/lib/inbox.js'
import { effectivePolicy } from '../hosts/lib/policy.js'

const refusal = (message, status = 409) => Object.assign(new Error(message), { status })
const decision = (operation) => {
  try {
    return operation()
  } catch (error) {
    throw refusal(error.message)
  }
}

/** The app owns durable claims; native integrations own their actual selected conversation. */
export class Inbox {
  constructor({ store, tabs, env, now = Date.now }) {
    Object.assign(this, { store, tabs, env, now })
  }

  async receive(operation, scope, input) {
    if (!scope?.ops?.includes('receiver')) throw refusal('receiver capability required', 403)
    const tab = await this.tabs.get(scope.tab)
    if (!tab) throw refusal('receiver owner no longer exists', 403)
    return this.store.mutate(tab.directory, `receiver.${operation}`, async (io) => {
      const tabs = await io.readTabs()
      const owner = tabs.find((entry) => entry.id === scope.tab)
      const reservation = owner?.lead?.reserved
      if (
        owner?.closed ||
        !reservation ||
        reservation.launchId !== scope.launch ||
        reservation.generation !== scope.generation ||
        reservation.pane !== scope.pane ||
        owner.lead.harness !== scope.kind ||
        owner.deleting
      )
        throw refusal('receiver launch is no longer current', 403)
      const state = await io.readInbox()
      let output
      if (operation === 'state') return state.receivers[scope.tab] ?? null
      if (operation === 'register') {
        if (typeof input.session !== 'string' || !/^[A-Za-z0-9_-]{1,200}$/.test(input.session))
          throw refusal('invalid native session', 400)
        const previous = state.receivers[scope.tab]
        if (scope.kind === 'claude-code' && input.source === 'fork') {
          if (!previous) throw refusal('continuation has no registered predecessor')
          const native = await answers(scope.kind, previous.session, this.env)
          if (native.continuedInSessionId !== input.session)
            throw refusal('independent forks cannot take over the receiver')
        }
        output = decision(() =>
          registerReceiver(state, {
            owner: scope.tab,
            launch: scope.launch,
            pane: scope.pane,
            generation: scope.generation,
            kind: scope.kind,
            session: input.session,
            previous: input.previous ?? null,
            lease: randomUUID(),
            now: this.now(),
          }),
        )
        owner.lead.nativeSession = output.session
        delete owner.lead.nativeSelection
        delete owner.lead.replaced
        await io.writeInbox(state)
        await io.writeTabs(tabs)
        return output
      }
      if (operation === 'retire') {
        output = decision(() =>
          retireReceiver(state, { owner: scope.tab, lease: input.lease, now: this.now() }),
        )
      } else if (operation === 'claim' || operation === 'wake') {
        const threads = await io.readThreads()
        const eligible = (result) =>
          result.requested === true ||
          (!result.manualOnly &&
            effectivePolicy(
              owner,
              owner.panes?.find((pane) => pane.conversation === result.conversation) ?? {},
              threads[result.conversation] ?? {},
            ).mode === 'auto')
        const receiver = state.receivers[scope.tab]
        if (operation === 'wake') {
          if (receiver?.lease !== input.lease || receiver.retiredAt !== undefined)
            throw refusal('receiver changed')
          const wake =
            !(receiver.wakeAt > this.now() - 60_000) &&
            Object.values(state.results).some(
              (result) =>
                result.owner === scope.tab &&
                resultStatus(result) === 'waiting' &&
                eligible(result),
            )
          if (wake) receiver.wakeAt = this.now()
          await io.writeInbox(state)
          return { wake }
        }
        output = decision(() =>
          claimNext(state, {
            owner: scope.tab,
            lease: input.lease,
            id: randomUUID(),
            now: this.now(),
            eligible,
          }),
        )
        delete receiver.wakeAt
      } else if (['begin', 'release', 'receipt'].includes(operation)) {
        const result = state.results[input.result]
        const claim = result?.claims.find((entry) => entry.id === input.claim)
        if (
          result?.owner !== scope.tab ||
          claim?.receiver.launch !== scope.launch ||
          claim.receiver.lease !== input.lease
        )
          throw refusal('claim does not belong to this receiver', 403)
        const request = { ...input, owner: scope.tab, now: this.now() }
        if (operation === 'begin') output = decision(() => beginInsertion(state, request))
        if (operation === 'release') {
          decision(() => releaseClaim(state, request))
          output = { state: claim.state }
        }
        if (operation === 'receipt') {
          const native = await answers(claim.receiver.kind, claim.receiver.session, this.env)
          const received =
            !native.unknown &&
            !native.replaced &&
            observeReceipt(state, {
              result: result.id,
              claim: claim.id,
              kind: claim.receiver.kind,
              session: claim.receiver.session,
              items: native.items,
              now: this.now(),
            })
          output = { received: received === true, state: claim.state }
        }
      } else throw refusal('unknown receiver operation', 404)
      await io.writeInbox(state)
      return output
    })
  }
}
