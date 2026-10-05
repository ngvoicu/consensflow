/**
 * Devin's scenarios (`src/adapters/devin.js`, with `src/devin-install.js`,
 * `src/channels/devin.js`, `src/devin-wire.js` and the Devin branch of
 * `src/role-skills.js`): its launches, fresh, resumed and refused, with the
 * files each leaves (`devin-launches.mjs`); how it names its session, and its
 * looks by its own wire log (`devin-looks.mjs`); its looks by its store,
 * whether it is ready for a paste, and its deliveries (`devin-panes.mjs`).
 * The cases of `tests/adapter-devin.test.mjs`, and more.
 */
import { devinLaunches } from './devin-launches.mjs'
import { devinLooks } from './devin-looks.mjs'
import { devinPanes } from './devin-panes.mjs'

export const devinScenarios = () => [...devinLaunches(), ...devinLooks(), ...devinPanes()]
