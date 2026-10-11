//! Watch a live project from outside (`evals/watch.mjs`, which the landing that ports
//! the evals replaces here): what its tasks and windows are doing, read from copies of
//! the app's ledger and trace (the live files are never opened). It says the tasks not
//! finished and who holds each, the messages still on their way, each window's changes
//! of state in the last minutes (one that flips between working and idle is flagged),
//! the windows that stopped on a prompt, deliveries held, retried or failed, members
//! out of quota; and with `--tokens`, what each Claude window's own record says it
//! used. `cargo xtask watch` runs it.
