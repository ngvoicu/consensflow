-- The summary a compaction writes in the user's place (Claude Code's
-- `isCompactSummary`) is the harness's own words, never the human's: the copy
-- marks it, so that no reader takes it for something the human typed (`cf
-- history`, the wake-up's check of what was typed, the handoff's last words)
-- and the history viewer names it. An item copied before this column is 0, as
-- it was read then. One column added in place, as 0017 added its own: nothing of
-- the transcript's rows moves. The column is one line: the test players that
-- hold a database this ledger left to what Node's did read the line to know what
-- Node's schema never had.
  ALTER TABLE transcript ADD COLUMN compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1));

-- Every field the history viewer reads but an item's text has a ceiling, which
-- each write keeps (`conversations::FIELD_MAX`, `HEADING_EVENT_MAX`): what an
-- older build wrote past one is brought within it here, once, never failing on
-- what it finds; each is measured in bytes, never by `length()`, which stops at
-- a NUL. An event a heading reads whose data is longer than 4,096 bytes is
-- written as what a heading reads of it, and the size it was: the
-- conversation's id where it is a whole number, and each name where JSON writes
-- it, its quotes too, in 258 bytes or fewer (so that a switch's four names and
-- its keys are 1,200 bytes at most); none where its data is no JSON. It is done
-- before the indexes of the log are built on it. The times, harnesses and
-- agents of the conversations and participants are brought within theirs past
-- this SQL (`schema::after`), where a cut can keep to a character.
  UPDATE event SET data = json_object(
      'conversation', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.conversation') = 'integer' THEN json_extract(data, '$.conversation') END END,
      'participant', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.participant') = 'text' THEN CASE WHEN octet_length(json_quote(json_extract(data, '$.participant'))) <= 258 THEN json_extract(data, '$.participant') END END END,
      'cutFromBytes', octet_length(data))
    WHERE kind IN ('conversation.started', 'conversation.followed', 'conversation.ended') AND octet_length(data) > 4096;
  UPDATE event SET data = json_object(
      'from', json_object(
        'harness', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.from.harness') = 'text' THEN CASE WHEN octet_length(json_quote(json_extract(data, '$.from.harness'))) <= 258 THEN json_extract(data, '$.from.harness') END END END,
        'agent', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.from.agent') = 'text' THEN CASE WHEN octet_length(json_quote(json_extract(data, '$.from.agent'))) <= 258 THEN json_extract(data, '$.from.agent') END END END),
      'to', json_object(
        'harness', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.to.harness') = 'text' THEN CASE WHEN octet_length(json_quote(json_extract(data, '$.to.harness'))) <= 258 THEN json_extract(data, '$.to.harness') END END END,
        'agent', CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.to.agent') = 'text' THEN CASE WHEN octet_length(json_quote(json_extract(data, '$.to.agent'))) <= 258 THEN json_extract(data, '$.to.agent') END END END),
      'cut', CASE WHEN json_valid(data) THEN CASE json_type(data, '$.cut') WHEN 'true' THEN json('true') WHEN 'false' THEN json('false') END END,
      'cutFromBytes', octet_length(data))
    WHERE kind = 'chief.switched' AND octet_length(data) > 4096;

-- The history viewer reads a participant's history a page at a time, and what
-- one page costs is bounded whatever the history's size: it reaches what it
-- reads through these indexes, and scans nothing; and it reads nothing of a row
-- past a value it does not read, which can be long (a row holds its values one
-- after another, and reaching one goes through those before it). The
-- participant it reads, by its handle, all it reads of it held in the index
-- (whether it left, its role and its agent), never in its row, which holds the
-- roles it was given before its agent; and its conversations:
  CREATE INDEX participant_history_index ON participant (project_id, handle, left_at IS NULL, role, agent);
  CREATE INDEX conversation_participant_index ON conversation (participant_id);
-- What the log says of a conversation, as each heading says it: where it began
-- or a window went to it, and where it ended; where each of a participant's
-- began or was gone to; and the chief's switches. An event an older build
-- wrote whose data is no JSON says nothing of a conversation or a participant
-- here: no index of the log may fail on what is in it.
  CREATE INDEX event_activation_index ON event (CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END) WHERE kind IN ('conversation.started', 'conversation.followed');
  CREATE INDEX event_ended_index ON event (CASE WHEN json_valid(data) THEN json_extract(data, '$.conversation') END) WHERE kind = 'conversation.ended';
  CREATE INDEX event_participant_index ON event (project_id, CASE WHEN json_valid(data) THEN json_extract(data, '$.participant') END) WHERE kind IN ('conversation.started', 'conversation.followed');
  CREATE INDEX event_switch_index ON event (project_id) WHERE kind = 'chief.switched';

-- What the viewer reads of each item but its text, by the item's row: its
-- role, the size of its text in bytes, how many parts its text is kept in (none
-- where it is read from its row), its id as a page shows it (its first 256
-- characters, and "…" where it is longer; the id itself is the item's identity,
-- kept whole in its row), whether it is complete, its time, whether it is a
-- compaction's summary, and which copy of it the row holds (the copy that
-- writes the item again changed makes it the next, so that a reader of a long
-- item's parts knows they are of one copy). A row holds its role past its id
-- and the rest past its text, either of which can run to megabytes, and SQLite
-- indexes no row by its id: so they are held here, where each is short. The
-- copy writes them with the row, and is their only writer but this migration,
-- which writes them for the items copied before it (`schema::after`). A row's
-- id is its own for as long as the row is: nothing rebuilds the table, and no
-- build runs VACUUM.
  CREATE TABLE transcript_head (
    item INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL,
    role TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    parts INTEGER NOT NULL,
    id_shown TEXT NOT NULL,
    complete INTEGER NOT NULL,
    at TEXT,
    compaction INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    CONSTRAINT transcript_head_conversation_fk FOREIGN KEY (conversation_id)
      REFERENCES conversation (id) ON DELETE CASCADE
  ) STRICT;
-- A conversation's items in the order they were first copied, which is their
-- row's (a copy made again keeps the row): all of them, which is also how a
-- conversation's heads are found when it is deleted; those that are no tool's
-- output, so that a page that leaves the tools out never goes through them; and
-- the tools' outputs alone, counted without being read.
  CREATE INDEX transcript_head_conversation_index ON transcript_head (conversation_id);
  CREATE INDEX transcript_head_said_index ON transcript_head (conversation_id) WHERE role != 'tool';
  CREATE INDEX transcript_head_tool_index ON transcript_head (conversation_id) WHERE role = 'tool';

-- An item longer than a part (64,000 characters; the human's words, an
-- agent's, are copied whole and can run to megabytes), or whose id is too long
-- to read past (`conversations::ID_IN_ROW`), is kept in parts as well, each part
-- a row of its item's, so that the viewer reads a part and no more, and never
-- reads the item's row past its id. Each holds the end of its text before its
-- text (its last 2,000 characters), what a search that goes on in the next part
-- reads of it. The copy writes them; the items copied before this table are
-- split once, when this migration runs (`schema::after`).
  CREATE TABLE transcript_part (
    item INTEGER NOT NULL,
    part INTEGER NOT NULL,
    tail TEXT NOT NULL,
    text TEXT NOT NULL,
    CONSTRAINT transcript_part_pk PRIMARY KEY (item, part),
    CONSTRAINT transcript_part_item_fk FOREIGN KEY (item)
      REFERENCES transcript_head (item) ON DELETE CASCADE
  ) STRICT;
