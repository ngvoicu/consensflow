/**
 * What the tables of plants are written with. A plant is `name`, which says
 * what is wrong with the code once planted; `edits`, the text replaced, each
 * in one file where it is found exactly once; `runs`, the arguments of the
 * `cargo test` that should fail, tried in order until one does; `meant`, the
 * test that was written for it.
 */
export const lines = (...text) => text.join('\n')
export const DAEMON = 'crates/cf-daemon/src'
export const unit = (crate, ...filter) => ['-p', crate, '--lib', ...filter]
export const daemon = (...filter) => unit('cf-daemon', ...filter)
export const stop = ['-p', 'cf', '--test', 'daemon_stop']
export const bridge = ['-p', 'cf-bridge', '--features', 'local', '--lib', 'local::']
