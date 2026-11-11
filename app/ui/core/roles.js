/**
 * The staff's roles, as the daemon's one table of them says (`cf_ledger::roles`, which
 * `harnesses.missing` and `board.get` answer as `roles`): each one's id, its name and the article it takes said
 * alone, the label a picker and a staff table show, whether its tasks have a tier, and whether
 * only an image agent holds it. The page reads them with every board it draws, and with the
 * harnesses as it starts and as a dialog of the staff opens, and holds no list of roles of its own: a role is one row of the
 * daemon's table, in its order.
 */

/** The roles as the daemon said them last; none before it answered. */
let table = []

/** Keeps `roles`, the `roles` of a `harnesses.missing` or `board.get` answer; one without them keeps the last. */
export function readRoles(roles) {
  if (Array.isArray(roles)) table = roles
}

/** The roles' ids, in the table's order: the order a picker offers them and a staff reads. */
export const roleIds = () => table.map((role) => role.id)

/** The role of id `id`; none for a word that is no role. */
const roleOf = (id) => table.find((role) => role.id === id)

/** What a picker and a staff table call role `id`: "Worker", "Image designer". */
export const roleLabel = (id) => roleOf(id)?.label

/** Whether role `id` is one only an image agent holds: the image designer. */
export const isImageAgentRole = (id) => roleOf(id)?.imageAgent === true

/** Whether role `id` is one whose tasks have no tier: the image designer's. */
export const untiered = (id) => roleOf(id)?.tiered === false

/**
 * Whether a member that holds `roles` takes no tier: it holds a role, and none of its roles has
 * tiers (`cf_ledger::roles::takes_no_tier`). One with a role with tiers beside them keeps its tier.
 */
export const takesNoTier = (roles) => roles.length > 0 && roles.every(untiered)

/** "standard worker", "image designer": who a task given by tier waits for. */
export const poolName = (task) =>
  untiered(task.pool)
    ? roleOf(task.pool).name
    : `${task.tier} ${roleOf(task.pool)?.name ?? task.pool}`

/** The same with its article: "a light worker", "an image designer". */
export const aPool = (task) =>
  untiered(task.pool) ? `${roleOf(task.pool).article} ${poolName(task)}` : `a ${poolName(task)}`
