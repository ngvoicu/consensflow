/**
 * What the channels of `crates/cf-harness` do with a message sent to a window,
 * held against what they run on (`crates/cf-harness/tests/channels`: the
 * machine's own clock, sockets and processes): each bug is one a case of that
 * suite was written for. Every name here begins `channels`, so
 * `npm run plants:daemon -- channels` runs them.
 */
import { lines } from './kit.mjs'

const HARNESS = 'crates/cf-harness/src'
/** The suite, narrowed by a filter to the cases of one channel, so that a run does not wait for the slow ones. */
const suite = (filter) => ['-p', 'cf-harness', '--test', 'channels', filter]

/** Each bug: what it is, the text it replaces in a file, who must notice, and with which test. */
export const PLANTS = [
  {
    name: "channels, Codex: a refusal of the broker's is uncertain",
    edits: [
      [
        `${HARNESS}/codex/channel/send.rs`,
        'Self::zero_bytes(error, None)',
        'Self::uncertain(error)',
      ],
    ],
    runs: [suite('codex')],
    meant: 'the_broker_refuses_it',
  },
  {
    name: 'channels, OpenCode: a send the plugin gives no head to is refused',
    edits: [
      [
        `${HARNESS}/opencode/channel/send.rs`,
        lines(
          '    let Some(Ok(mut reply)) = timeout.bound(wires.loopback.send(request)).await else {',
          '        return Ok(uncertain());',
          '    };',
        ),
        lines(
          '    let Some(Ok(mut reply)) = timeout.bound(wires.loopback.send(request)).await else {',
          '        return Ok(refused("transport".to_owned()));',
          '    };',
        ),
      ],
    ],
    runs: [suite('opencode_through')],
    meant: 'the_plugin_drops_the_connection_after_the_post',
  },
  {
    name: 'channels, OpenCode: a conversation on its default effort is seeded with another',
    edits: [[`${HARNESS}/opencode/channel/seed.rs`, '.unwrap_or("default")', '.unwrap_or("max")']],
    runs: [suite('seed::sends_')],
    meant: 'sends_explicit_native_default_rather_than_falling_back_to_roster_or_agent_effort',
  },
  {
    name: 'channels, OpenCode: a session made in another folder is taken',
    edits: [
      [
        `${HARNESS}/opencode/channel/create.rs`,
        'body.get("directory").and_then(Value::as_str) != Some(canonical)',
        'body.get("directory").is_none()',
      ],
    ],
    runs: [suite('create::rejects_malformed')],
    meant: 'rejects_malformed_invalid_id_wrong_dir_and_oversized_responses',
  },
  {
    name: 'channels, Pi: a failure after the inbox rename is a refusal',
    edits: [
      [
        `${HARNESS}/pi/channel/send.rs`,
        'Answer::uncertain(Some(failed.to_string()), None)',
        'Answer::zero_bytes("transport", Some(failed.to_string()))',
      ],
    ],
    runs: [suite('pi_through')],
    meant: 'an_error_after_the_inbox_rename',
  },
]
