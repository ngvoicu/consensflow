/**
 * Plants in what the agents screens do, in Node's daemon and in the native one:
 * the catalog, an agent saved with its profile, the screens behind the UI token,
 * and the deletion. The proof of the agents screens (tests/agents-proof.mjs),
 * which the packaged smoke runs against the daemon a built app chose, must
 * catch each.
 */
import { BOTH, PROOF } from './kit.mjs'

const AGENTS_SERVER = 'src/core/agents-server.js'
const ROSTER = 'src/roster.js'
const node = (name, edits) => ({
  name: `agents, node: ${name}`,
  edits,
  runs: [PROOF],
  meant: 'serves the agents as the packaged smoke holds the built app to',
})

const EDIT_RS = 'crates/cf-catalog/src/roster/edit.rs'
const ADD_RS = 'crates/cf-catalog/src/roster/add.rs'
const SAVE_RS = 'crates/cf-catalog/src/roster/save.rs'
const ROWS_RS = 'crates/cf-catalog/src/roster/rows.rs'
const PROFILE_RS = 'crates/cf-catalog/src/profile.rs'
const SCREENS_RS = 'crates/cf-daemon/src/screens/mod.rs'
const native = (name, edits) => ({
  name: `agents, native: ${name}`,
  edits,
  runs: [BOTH],
  meant: 'serves the agents as the packaged smoke holds the built app to',
})

export const PLANTS = [
  node('the screens open with no token', [
    [
      AGENTS_SERVER,
      'if (presented.length === 0 || !tokenMatches(presented, token)) {',
      'if (presented.length > 0 && !tokenMatches(presented, token)) {',
    ],
  ]),
  node('a deleted agent is answered and kept', [
    [AGENTS_SERVER, 'removeAgent(named[1], env)', 'void named[1]'],
  ]),
  node('an agent saved has not the effort it was given', [
    [
      ROSTER,
      '...(input.effort ? { [effortKey(HARNESS_TO_KIND[input.harness])]: input.effort } : {}),',
      '...{},',
    ],
  ]),
  node('an agent saved keeps its profile in the file', [
    [
      ROSTER,
      "const STALE_FIELDS = ['skillsPolicy', 'skillPaths', 'skills', 'skillPath', 'profile']",
      "const STALE_FIELDS = ['skillsPolicy', 'skillPaths', 'skills', 'skillPath']",
    ],
    [
      ROSTER,
      '...(input.description ? { description: input.description } : {}),\n  }\n  document.agents.push(row)',
      '...(input.description ? { description: input.description } : {}),\n    profile: { stored: true },\n  }\n  document.agents.push(row)',
    ],
  ]),
  node('the catalog is short of an agent', [
    [
      ROSTER,
      '...AGENT_PRESETS.filter((preset) => !hidden.has(preset.id)).map(catalogRow),',
      '...AGENT_PRESETS.filter((preset) => !hidden.has(preset.id)).slice(1).map(catalogRow),',
    ],
  ]),

  native('a deleted agent is answered and kept', [
    [EDIT_RS, 'document.agents_mut().remove(at);', 'let _kept = at;'],
  ]),
  native('a catalog agent can be deleted', [
    [
      EDIT_RS,
      'let at = self.own_row(&document, name, || {\n            format!("{name} is a catalog agent: it is not yours to remove")\n        })?;',
      'let Ok(at) = self.own_row(&document, name, || {\n            format!("{name} is a catalog agent: it is not yours to remove")\n        }) else {\n            return Ok(());\n        };',
    ],
  ]),
  native('an agent saved has not the effort it was given', [
    [
      ADD_RS,
      'row.insert(effort_key(Some(harness.kind())).to_owned(), effort.clone());',
      'let _ = effort;',
    ],
  ]),
  native('an agent saved keeps its profile in the file', [
    [
      SAVE_RS,
      'pub(super) const STALE_FIELDS: [&str; 5] = [',
      'pub(super) const STALE_FIELDS: [&str; 4] = [',
    ],
    [SAVE_RS, '    "skillPath",\n    "profile",\n];', '    "skillPath",\n];'],
    [
      ADD_RS,
      'row.insert("model".to_owned(), Value::from(model));',
      'row.insert("model".to_owned(), Value::from(model));\n        row.insert("profile".to_owned(), Value::Null);',
    ],
  ]),
  native('an agent on gpt-6-astra at low effort is standard work', [
    [
      PROFILE_RS,
      'Some("medium") => WorkTier::Standard,\n                    _ => WorkTier::Light,\n                },\n                "claude-opus-5.5"',
      'Some("medium") => WorkTier::Standard,\n                    _ => WorkTier::Standard,\n                },\n                "claude-opus-5.5"',
    ],
  ]),
  native('the screens open to an empty token', [
    [
      SCREENS_RS,
      '!presented.is_empty() && token_matches(presented, &self.token)',
      'presented.is_empty() || token_matches(presented, &self.token)',
    ],
  ]),
  native('the catalog is short of an agent', [
    [
      ROWS_RS,
      '            .map(catalog_row)\n            .collect();',
      '            .skip(1)\n            .map(catalog_row)\n            .collect();',
    ],
  ]),
]
