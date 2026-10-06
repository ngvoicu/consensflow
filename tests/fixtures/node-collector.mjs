/**
 * Node's daemon taking a ledger the Rust daemon wrote, for the tests that hold
 * the way back to Node (`crates/cf-ledger/tests/node.rs`): it delivers what is
 * next for a window as its dispatcher does (the delivery begins, and is
 * confirmed once the paste shows in the window's record), and then runs its own
 * collector (`Deliveries.collect`, `src/core/deliveries.js`) on what that
 * window's record shows, which the plan says. Nothing else of the daemon runs.
 *
 *   node node-collector.mjs <the ledger module> <the ledger file> <the plan>
 *
 * The plan is a JSON object: the project's id, the window's participant (id
 * and handle), and what its turn after the paste wrote, if anything: `words`,
 * and whether the turn is over, `settled`. It prints one line of JSON: what
 * Node pasted (`text`, whole), the id of the message it was, the task's state
 * after the collector ran, and its result if the collector made one.
 */
import { pathToFileURL } from 'node:url'

const [ledgerModule, file, planText] = process.argv.slice(2)
const plan = JSON.parse(planText)
const ledgerUrl = pathToFileURL(ledgerModule)
const { openLedger } = await import(ledgerUrl.href)
const { Deliveries } = await import(new URL('../core/deliveries.js', ledgerUrl).href)
const { deliveryText } = await import(new URL('../core/delivery-text.js', ledgerUrl).href)

const ledger = openLedger(file)
const next = ledger.nextDelivery(plan.participant)
let said = { delivered: null }
if (next !== null) {
  ledger.beginDelivery(next.id)
  const text = deliveryText(next)
  ledger.confirmDelivery(next.id, { item: 'i-1' })
  const items = [{ id: 'i-1', role: 'user', text }]
  if (typeof plan.words === 'string') {
    items.push({ id: 'i-2', role: 'assistant', text: plan.words, complete: true })
  }
  // Only its collector needs more of the daemon than the ledger.
  const deliveries = new Deliveries({ ledger, changed: () => {} })
  deliveries.collect(
    { id: plan.project },
    { id: plan.participant, handle: plan.handle },
    { items, settled: plan.settled === true, failed: false },
  )
  const thread = ledger.task(plan.project, next.taskNumber)
  said = {
    delivered: next.id,
    text,
    task: thread.state,
    result: thread.messages.find((message) => message.kind === 'result')?.body ?? null,
  }
}
console.log(JSON.stringify(said))
ledger.close()
