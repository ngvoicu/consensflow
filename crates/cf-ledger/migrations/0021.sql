-- The conversation a message on its way was pasted into: a window the human switched
-- to another conversation (`/new`, `/resume`, `/clear`) before its record showed the
-- paste leaves it in the one it left, and a start that settles what was on its way
-- reads that one's record, not the one the window shows now (the review of the Devin
-- arrival branch, 2026-10-22: a paste the conversation left had run was given back
-- and pasted again into the new one). Written as a delivery begins, and cleared when
-- the message is on its way no more, whatever moves it (the trigger), so a row says
-- it only while it is delivering. One column added in place, as 0020 added its own:
-- nothing of the messages' rows moves. A migration that rebuilds the message table
-- drops the trigger with it, and must make it again. 0021 here; renumbered to 0023 after
-- resume-tasks' 0021 and 0022 when this branch merges main after T-82.
  ALTER TABLE message ADD COLUMN pasted_into INTEGER CONSTRAINT message_pasted_into_fk REFERENCES conversation (id) ON DELETE SET NULL;
  CREATE TRIGGER message_pasted_into_settled AFTER UPDATE OF state ON message
    WHEN NEW.state <> 'delivering' AND NEW.pasted_into IS NOT NULL
    BEGIN
      UPDATE message SET pasted_into = NULL WHERE id = NEW.id;
    END;
