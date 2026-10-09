/**
 * Plants in what the agents screens do, in the native daemon: the catalog, an
 * agent saved with its profile, the screens behind the UI token, and the
 * deletion. The proof of the agents screens (tests/agents-proof.mjs), which the
 * packaged smoke runs against the daemon a built app started, must catch each:
 * here it is run against the daemon from the checkout, by the `agents_proof`
 * cases of the daemon test of crates/cf-e2e (`cargo xtask test agents`).
 */
import { BUILT_PROOF } from './kit.mjs'

const EDIT_RS = 'crates/cf-catalog/src/roster/edit.rs'
const ADD_RS = 'crates/cf-catalog/src/roster/add.rs'
const SAVE_RS = 'crates/cf-catalog/src/roster/save.rs'
const ROWS_RS = 'crates/cf-catalog/src/roster/rows.rs'
const PROFILE_RS = 'crates/cf-catalog/src/profile.rs'
const SCREENS_RS = 'crates/cf-daemon/src/screens/mod.rs'
const native = (name, edits) => ({
  name: `agents, native: ${name}`,
  edits,
  runs: [BUILT_PROOF],
  meant: 'serves_the_agents_as_the_packaged_smoke_holds_the_built_app_to',
})

export const PLANTS = [
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
