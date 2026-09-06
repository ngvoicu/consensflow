/**
 * Delivery policy — pure, no I/O, no env.
 *
 * Two scopes belong to the human and one to the lead, and the order between
 * them is fixed (Decision Log, 2026-09-06):
 *
 *   tab human `manual`  →  a veto: nothing under this tab is delivered
 *   pane human setting  →  `auto` or `manual` for this conversation
 *   tab human `auto`    →  the tab's own choice, for panes that said nothing
 *   lead preference     →  what `cf run --notify` recorded on the row
 *   default             →  `auto`
 *
 * The veto sits ABOVE the pane setting and the tab's `auto` BELOW it, which
 * is why the tab appears twice: turning a whole tab off is a stronger act
 * than turning it on, and a pane the human tuned should survive the second
 * but never the first.
 *
 * `inherit` at pane scope means "no pane setting" and falls through. So does
 * anything that is not exactly `'auto'` or `'manual'`: an absent field, a
 * `null`, or a value the store would never write. Only `src/store.js`
 * `policySet` writes the tab and pane scopes, and only from the page.
 *
 * The lead is heard through ONE field, `row.notifyPreference`. It has no
 * human field to write — the human's scopes are the tab and the pane, which
 * `cf run --notify` cannot address at all — so precedence, not a `setBy`
 * marker, is what keeps a lead from overriding a person.
 *
 * The result names its source because the page puts it in the pane title:
 * `name · @agent · policy (source)`.
 */

/** The only field `cf run --notify` may write. */
const LEAD_FIELD = "notifyPreference";

/**
 * The effective policy for one pane, and where it came from.
 *
 * @param {{policy?: string}} tab — the tab record; `policy` is `auto|manual`.
 * @param {{policy?: string}} pane — the pane record; `policy` is
 *   `auto|manual|inherit`.
 * @param {{notifyPreference?: string}} row — the conversation row.
 * @returns {{mode: 'auto'|'manual', source: 'tab-human'|'pane-human'|'lead'|'default'}}
 */
export function effectivePolicy(tab, pane, row) {
  const tabPolicy = setting(tab?.policy);
  if (tabPolicy === "manual") return { mode: "manual", source: "tab-human" };

  const panePolicy = setting(pane?.policy);
  if (panePolicy !== null) return { mode: panePolicy, source: "pane-human" };

  if (tabPolicy !== null) return { mode: tabPolicy, source: "tab-human" };

  const leadPolicy = setting(row?.[LEAD_FIELD]);
  if (leadPolicy !== null) return { mode: leadPolicy, source: "lead" };

  return { mode: "auto", source: "default" };
}

/**
 * May `cf run --notify` record a preference on this row, and in which field?
 *
 * Always `notifyPreference` when there is a row to write it on — the answer
 * names the field so the caller cannot reach for another one. A missing row
 * is the only refusal: a preference belongs to a conversation, and there is
 * nothing to record it against.
 */
export function leadMaySet(row) {
  if (row === null || typeof row !== "object" || Array.isArray(row)) {
    return {
      allowed: false,
      field: null,
      reason: "no conversation row: a lead preference is recorded on the conversation it is about",
    };
  }
  return { allowed: true, field: LEAD_FIELD };
}

/**
 * Apply `cf run --notify <value>` to a conversation row — the write itself,
 * so the rule and the write cannot drift apart.
 *
 * Pure: it returns a NEW row and never mutates the one it is given, so a
 * refused write cannot leave a half-applied record behind. Only
 * `notifyPreference` may differ. Every other field comes through untouched,
 * including any human-looking one a row happens to carry: the human's scopes
 * are the tab and the pane, which this function is never handed and could
 * not reach even if it tried.
 *
 * Answers `{ok: true, field, row}` with the new row, or `{ok: false, reason,
 * row}` with the row exactly as it came in (`null` when there was none).
 */
export function recordLeadPreference(row, value) {
  const may = leadMaySet(row);
  if (!may.allowed) return { ok: false, reason: may.reason, row: null };
  if (setting(value) === null) {
    return {
      ok: false,
      reason: `not a delivery preference: ${JSON.stringify(value)} — cf run --notify records 'auto' or 'manual'`,
      row,
    };
  }
  return { ok: true, field: may.field, row: { ...row, [may.field]: value } };
}

/** `'auto'` or `'manual'` exactly, or `null` — everything else is unset. */
function setting(value) {
  return value === "auto" || value === "manual" ? value : null;
}
