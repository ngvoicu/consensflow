-- The role text a message tells: the replacement ConsensFlow writes to a
-- conversation whose harness keeps its first instructions (Codex) when the role
-- changed since the conversation began. The ledger knows such a message by this
-- column and by nothing in its words: every other message holds a NULL. What
-- the replacement says is recorded for the conversation in the very write that
-- confirms its delivery, from this text. Added in place, as 0016 added its own
-- column: nothing of the table moves, and no view of a message holds it.
-- The column is one line: the test players that hold a database this ledger left
-- to what Node's did read the line to know what Node's schema never had.
  ALTER TABLE message ADD COLUMN role_text TEXT;
