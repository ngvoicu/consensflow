//! The pure part of an eval run (`evals/plan.mjs`, which the landing that ports the
//! evals replaces here): which agents make up the staff for a set of harnesses, what
//! environment gives a chief its model where the harness takes one from the
//! environment, and how the scripted human answers a question. Everything here is
//! tested without spending a token.
//!
//! What the evals share with the live tests of `crates/cf-e2e` lives there once
//! (`cf_e2e::live`): the machine's harnesses and the environment their windows get,
//! the program Claude Code is started through, and the flags that shut Codex's MCP
//! servers out ([`cf_e2e::live::codex::isolation`], with the URL a server reached by
//! URL is given: a command on it kills Codex 0.160.1). The cheap model of each harness
//! here is the evals' own (the staff's and the chief's), not the live tests'
//! ([`cf_e2e::live::agents`]).
