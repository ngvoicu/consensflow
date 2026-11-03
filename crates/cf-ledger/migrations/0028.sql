-- A chief is a harness and a model (CHF-04): the model the human picked for it in New
-- project or Switch chief, or the one its harness's default gave it when the picked one
-- was gone, where a chief ran on a saved agent's before (`agent`, which a chief's row now
-- leaves NULL). What a Switch chief left, beside the harness it left. And the model the
-- chief's last launch asked for, which a reopen compares with what the harness's record
-- says it ran on last: a move the harness made by itself away from it is never taken for
-- the human's (MUS-24). And the model a launch found its harness no longer has, which a
-- reopen never asks for again though the record still names it, the chief starting on its
-- harness's default instead (CHF-06). A member's row holds none of the four. A row a
-- ledger before this one wrote holds NULL in each: a chief from then names a saved agent,
-- which nothing reads for it any more, and its reopen reads its record. The
-- `chief.switched` events logged from now on name a model on each side where those before
-- named an agent; the old ones stay as they were. Added in place, as 0024 added its own
-- column: nothing of the table moves. Each column is one line: the test players that hold
-- a database this ledger left to what Node's did read the lines to know what Node's schema
-- never had.
  ALTER TABLE participant ADD COLUMN model TEXT;
  ALTER TABLE participant ADD COLUMN switched_from_model TEXT;
  ALTER TABLE participant ADD COLUMN launched_model TEXT;
  ALTER TABLE participant ADD COLUMN gone_model TEXT;
