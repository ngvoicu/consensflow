-- The summary a compaction writes in the user's place (Claude Code's
-- `isCompactSummary`) is the harness's own words, never the human's: the copy
-- marks it, so that no reader takes it for something the human typed (`cf
-- history`, the wake-up's check of what was typed, the handoff's last words)
-- and the history viewer names it. An item copied before this column is 0, as
-- it was read then. One column added in place, as 0017 added its own: nothing of
-- the transcript's rows moves. The column is one line: the test players that
-- hold a database this ledger left to what Node's did read the line to know what
-- Node's schema never had. So is the next.
  ALTER TABLE transcript ADD COLUMN compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1));
-- Which copy of its item a row holds: the copy that writes the item again
-- changed makes it the next, so that a reader of a long item's parts knows
-- they are of one copy. An item copied before this column is at its first.
  ALTER TABLE transcript ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;

-- The history viewer reads a participant's history a page at a time, and what
-- one page costs is bounded whatever the history's size: it reaches what it
-- reads through these indexes, and scans nothing. A participant's conversations:
  CREATE INDEX conversation_participant_index ON conversation (participant_id);
-- A conversation's items in the order they were first copied, which is their
-- row's (a copy made again keeps the row): all of them; those that are no tool's
-- output, so that a page that leaves the tools out never goes through them; and
-- the tools' outputs alone, counted without being read.
  CREATE INDEX transcript_copied_index ON transcript (conversation_id);
  CREATE INDEX transcript_said_index ON transcript (conversation_id) WHERE role != 'tool';
  CREATE INDEX transcript_tool_index ON transcript (conversation_id) WHERE role = 'tool';
-- An item's fields that its row holds after its text, which a long item's row
-- holds only past the whole of its text.
  CREATE INDEX transcript_item_index ON transcript (conversation_id, item_id, complete, at, compaction, revision);
-- What the log says of a conversation, as each heading says it: where it began
-- or a window went to it, and where it ended; where each of a participant's
-- began or was gone to; and the chief's switches. An event an older build
-- wrote whose data is no JSON says nothing of a conversation or a participant
-- here: no index of the log may fail on what is in it.
  CREATE INDEX event_activation_index ON event (CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END) WHERE kind IN ('conversation.started', 'conversation.followed');
  CREATE INDEX event_ended_index ON event (CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END) WHERE kind = 'conversation.ended';
  CREATE INDEX event_participant_index ON event (project_id, CASE WHEN json_valid(data) THEN json_extract(data, '$.participant') END) WHERE kind IN ('conversation.started', 'conversation.followed');
  CREATE INDEX event_switch_index ON event (project_id) WHERE kind = 'chief.switched';

-- An item longer than a part (64,000 characters; the human's words, an
-- agent's, are copied whole and can run to megabytes) is kept in parts as well,
-- each part a row, so that the viewer reads a part and no more. The copy writes
-- them; the long items copied before this table are split once, when this
-- migration runs (`schema::after`).
  CREATE TABLE transcript_part (
    conversation_id INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    part INTEGER NOT NULL,
    text TEXT NOT NULL,
    CONSTRAINT transcript_part_pk PRIMARY KEY (conversation_id, item_id, part),
    CONSTRAINT transcript_part_item_fk FOREIGN KEY (conversation_id, item_id)
      REFERENCES transcript (conversation_id, item_id) ON DELETE CASCADE
  ) STRICT;
