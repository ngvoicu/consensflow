//! A chief as a bare harness (`evals/bare.mjs`, which the landing that ports the
//! evals replaces here): no ConsensFlow, the same workspace, prompt, model and effort
//! as the other arms, the owner answering in its terminal. With nothing of
//! ConsensFlow in the window, its session is found in the harness's own store
//! ([`cf_e2e::live::ask::newest_session`]): Claude and Pi open on an id given to
//! them; Codex, Devin and OpenCode name theirs only in their stores, by folder.
//!
//! The numbers of a bare run are taken from the chief's own record, shaped as the other
//! arms' so that the same expectations read them: no board, so no tasks, notes or board
//! questions; what the owner was asked is its turns' ends.
