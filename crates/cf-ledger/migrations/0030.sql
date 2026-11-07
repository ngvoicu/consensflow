-- A member's tier the human chose (T-1143): Add member and Edit member take a tier the human
-- picks, for a model its harness lists that the bundled tier rules may not know. A member
-- whose tier the human chose keeps it: the start's following of the agents' tiers
-- (`refresh_member_tiers`) passes it by. 0 for a member whose tier follows its saved agent's,
-- as every member a ledger before this one wrote does; 1 for one whose tier is the human's.
-- Its sessions keep their member's tier as they always did. Staff chains move tiers to their
-- slots (CHN-02, CHN-03), which take this one over. One line, as 0026 added its own.
  ALTER TABLE participant ADD COLUMN tier_chosen INTEGER NOT NULL DEFAULT 0 CHECK (tier_chosen IN (0, 1));
