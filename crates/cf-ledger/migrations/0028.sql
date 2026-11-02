-- What the history draws of an item besides its words (chief-harness, CHF-14 to CHF-16):
-- its kind (prompt, text, thinking, tool, compaction, notice), and for a tool's call its
-- name, a header of 256 characters at most, how it stands (running, ok, failed), the time
-- it took and the change it made; for a compaction the tokens it compacted from. The copy
-- writes it as each harness's reader says it, as JSON in the item's row (`detail`), which
-- the copy compares with what a look reads again, as it compares the item's words: an item
-- whose call ended with its words unchanged is written again. NULL for an item its reader
-- says nothing more of than its role does, and for every item copied before this one: an
-- old item draws as a plain one, of its role.
--
-- And whether the item is one only the history shows (`drawn_only`): a model's thinking, a
-- call still running, a compaction its harness wrote no summary item of. No reader of the
-- conversation's words reads one (`cf history`, a task's transcript, the handoff); the
-- history reads them in their place among the others.
--
-- The viewer reads the short fields from the item's head, where each is before any long
-- value, and the change an edit made (64,000 characters at most) from a row of its own,
-- charged as a part is: never from the item's row, which holds them past its words.
-- `diff_bytes` says how long the change is, 0 where there is none. Added in place, a
-- column a line, as 0026 added its own: nothing of either table moves, and no item copied
-- before is written again.
  ALTER TABLE transcript ADD COLUMN detail TEXT;
  ALTER TABLE transcript ADD COLUMN drawn_only INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE transcript_head ADD COLUMN kind TEXT;
  ALTER TABLE transcript_head ADD COLUMN name TEXT;
  ALTER TABLE transcript_head ADD COLUMN header TEXT;
  ALTER TABLE transcript_head ADD COLUMN status TEXT;
  ALTER TABLE transcript_head ADD COLUMN took INTEGER;
  ALTER TABLE transcript_head ADD COLUMN tokens INTEGER;
  ALTER TABLE transcript_head ADD COLUMN diff_bytes INTEGER NOT NULL DEFAULT 0;
  CREATE TABLE transcript_diff (
    item INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    CONSTRAINT transcript_diff_item_fk FOREIGN KEY (item)
      REFERENCES transcript_head (item) ON DELETE CASCADE
  ) STRICT;
