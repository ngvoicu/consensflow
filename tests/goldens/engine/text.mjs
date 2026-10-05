/**
 * The engine's texts, as tables of what Node answers (`crates/cf-engine` is
 * held to them):
 * - how a message reads in its recipient's pane, as the window takes it
 *   (`delivery.mjs`) and the marker that proves it arrived;
 * - the new chief's handoff, the human's last words, `cf history`'s pages
 *   (`handoff.mjs`, over the histories and messages of `histories.mjs`), and
 *   `toLowerCase`, which its search reads by (`lower.mjs`);
 * - each role's instructions, the staff table and the work tiers
 *   (`roles.mjs`).
 *
 * `common.mjs` says how a long text is written and how a half of a surrogate
 * pair reads.
 */
import { deliveryTable, markerTable } from './delivery.mjs'
import { handoffTables } from './handoff.mjs'
import { histories, MESSAGES } from './histories.mjs'
import { lowerCaseTable } from './lower.mjs'
import { roleTables } from './roles.mjs'

/** The tables, as one golden. */
export function text() {
  return {
    messages: MESSAGES,
    histories: histories(),
    deliveryText: deliveryTable(),
    marker: markerTable(),
    lowerCase: lowerCaseTable(),
    ...handoffTables(),
    ...roleTables(),
  }
}
