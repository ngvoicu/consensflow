/**
 * Plants in the product that the updater smoke (`npm run smoke:updater`) must
 * fail on, each rebuilding what it plants and running the smoke on the result:
 * the daemon's readiness taken out (the app says nothing of its choice; the daemon
 * logs no start), the ledger's check taken out (a second daemon opens the ledger
 * the first holds; an update empties it), the refused update's check taken out
 * (the installed app takes a bundle with no `cf`, and one whose seal is broken),
 * and the update's own work taken out (no repair of the terminal command, a
 * repair of another home's, a default that is Node's). They need the smoke's
 * builds to have been made once (`npm run smoke:updater`): the installed app and the
 * update it kept, and the bridge's source it exported, which the check of an
 * update is planted in, as the installed app is the one that judges an update.
 * They are built into a folder of their own, so what a run kept is not replaced
 * by a plant's.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { smoke } from './kit.mjs'

// What the product plants run against: the builds a run of the smoke kept, and
// the source of the bridge it exported, which the installed app's check of an
// update is planted in. What they build goes in a folder of their own.
const KEPT = 'app/src-tauri/target/updater-smoke'
const INSTALLED = `${KEPT}/installed/ConsensFlow.app`
const UPDATE = `${KEPT}/update/ConsensFlow.app`
const BRIDGE = `${KEPT}/bridge-source`
const BRIDGE_UPDATE_INSTALL = `${BRIDGE}/app/src-tauri/src/update_install.rs`
const PLANTED = 'app/src-tauri/target/updater-smoke-plants'

/** A case of the smoke on the update built from the planted tree, taking the installed app a run kept. */
const planted = (only) =>
  smoke('--from-app', INSTALLED, '--cache', PLANTED, '--only', only, '--timeout', '60000')
/** A case of the smoke on the installed app built from the planted bridge source, taking the update a run kept. */
const plantedBridge = (only) =>
  smoke(
    '--bridge',
    BRIDGE,
    '--to-app',
    UPDATE,
    '--cache',
    PLANTED,
    '--only',
    only,
    '--timeout',
    '60000',
  )

const UPDATES = 'the update installs and restarts'
// The bridge's source is exported where the text of a plant in it is wanted, a
// static check of the plants included; the build a plant's run takes as given is
// required when it is run, which `npm run smoke:updater` makes once.
const product = (name, edits, run, meant, { bridge = false } = {}) => ({
  name: `updater product: ${name}`,
  prepare: bridge ? [smoke('--export-bridge')] : [],
  requires: [bridge ? UPDATE : INSTALLED],
  edits,
  runs: [run],
  meant,
  builds: true,
})

const DAEMON_COMMAND_RS = 'app/src-tauri/src/daemon_command.rs'
const START_RS = 'crates/cf-daemon/src/start.rs'
const LEDGER_RS = 'crates/cf-ledger/src/ledger.rs'
const LIB_RS = 'app/src-tauri/src/lib.rs'
const REPAIR_RS = 'crates/cf-launcher/src/repair.rs'

const PRODUCT = [
  // Readiness: what the smoke reads of an app that chose its daemon and started it.
  product(
    'the app does not say which daemon it chose',
    [[DAEMON_COMMAND_RS, '    eprintln!("consensflow: {}", chosen(&choice));\n', '']],
    planted(UPDATES),
    UPDATES,
  ),
  product(
    'the daemon logs no start line',
    [
      [
        START_RS,
        '    log.info(&format!(\n        "start pid {} rust {VERSION} home {}",\n        std::process::id(),\n        home.display()\n    ));\n',
        '',
      ],
    ],
    planted(UPDATES),
    UPDATES,
  ),
  // The ledger's check: one holder, and what it holds.
  product(
    'a second daemon opens the ledger the first holds',
    [
      [LEDGER_RS, '        db.execute_batch("PRAGMA locking_mode = EXCLUSIVE")?;\n', ''],
      [LEDGER_RS, '        db.execute_batch("BEGIN EXCLUSIVE; COMMIT")?;\n', ''],
    ],
    planted(UPDATES),
    UPDATES,
  ),
  product(
    'the update empties the events of the ledger it opens',
    [
      [
        LEDGER_RS,
        '        migrate(&db, &MIGRATIONS)\n    })();',
        '        migrate(&db, &MIGRATIONS)?;\n        db.execute_batch("DELETE FROM event")?;\n        Ok(())\n    })();',
      ],
    ],
    planted(UPDATES),
    UPDATES,
  ),
  // The refused update's check: the installed app's, which is the bridge's.
  product(
    'the installed app takes a bundle with no cf',
    [
      [
        BRIDGE_UPDATE_INSTALL,
        '    if !app.join("Contents/Resources/cli/bin/cf").is_file() {\n        return Err("The archive must include cf, the command a window runs".into());\n    }\n',
        '',
      ],
    ],
    plantedBridge('refused'),
    'without-cf',
    { bridge: true },
  ),
  product(
    'the installed app takes a bundle whose seal is broken',
    [
      [
        BRIDGE_UPDATE_INSTALL,
        '    if !signature.status.success() {',
        '    if false && !signature.status.success() {',
      ],
    ],
    plantedBridge('refused'),
    'tampered',
    { bridge: true },
  ),
  // What the update does of its own.
  product(
    'the app does not repair the terminal command at its start',
    [[LIB_RS, '            launcher::repair_in_background(app.handle());\n', '']],
    planted(UPDATES),
    UPDATES,
  ),
  product(
    'the app repairs a command that serves another home',
    [
      [
        REPAIR_RS,
        '    if !serves(&text, windows, env) {\n        return Repair::Elsewhere;\n    }\n',
        '',
      ],
    ],
    planted(UPDATES),
    UPDATES,
  ),
  // The app's log says the native daemon, which is what the home chose, and Node's is what it starts:
  // only the process the machine shows tells. (Planted in the choice itself, Node's `cf.mjs` and the
  // native `cf`, which each ask it, would hand a command to each other without end.)
  product(
    'the app starts Node’s daemon where its log says the native one',
    [
      [
        DAEMON_COMMAND_RS,
        '    (command_for(node, cli, choice.node), choice)',
        '    (command_for(node, cli, true), choice)',
      ],
    ],
    planted(UPDATES),
    UPDATES,
  ),
]

export const PLANTS = PRODUCT
