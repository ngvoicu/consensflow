//! Every query of the history, as the reads of it run them: each goes by a key
//! or an index and reads no row it need not (the test below holds each to its
//! plan).

/// The items of conversation `?1` from row `?2` the way a read goes
/// (`newest_first`, the row itself too where `with_it`), as their heads say
/// them, gone through by the index of what the read shows (every item, or none
/// of the tools' outputs). The items' own rows are not read: each holds its
/// role past its id, and the rest past its text.
pub(super) fn rows_from(tools: bool, newest_first: bool, with_it: bool) -> String {
    format!(
        "SELECT item AS row_id, role, bytes, parts, id_shown, complete, at, compaction, revision,
         kind, name, header, status, took, tokens, diff_bytes,
         octet_length(id_shown) + ifnull(octet_length(at), 0) + ifnull(octet_length(kind), 0)
           + ifnull(octet_length(name), 0) + ifnull(octet_length(header), 0)
           + ifnull(octet_length(status), 0) AS field_bytes
       FROM transcript_head INDEXED BY {}
       WHERE conversation_id = ?1{} AND item {}{} ?2 ORDER BY item {}",
        if tools {
            "transcript_head_conversation_index"
        } else {
            "transcript_head_said_index"
        },
        if tools { "" } else { " AND role != 'tool'" },
        if newest_first { "<" } else { ">" },
        if with_it { "=" } else { "" },
        if newest_first { "DESC" } else { "ASC" },
    )
}

/// The participant's (`?1`) conversation next to `?2` the way a read goes
/// (`newest_first`): its id, from its index alone.
pub(super) fn conversation_from(newest_first: bool) -> String {
    format!(
        "SELECT id FROM conversation INDEXED BY conversation_participant_index
       WHERE participant_id = ?1 AND id {} ?2 ORDER BY id {} LIMIT 1",
        if newest_first { "<" } else { ">" },
        if newest_first { "DESC" } else { "ASC" },
    )
}

/// The id of participant `?1`'s conversation now (the one that has not ended), from the
/// index of those. Its row is read up to `ended_at`, past its harness, its native session
/// (512 characters at most) and its times, each short whatever an older build wrote; its
/// role, which may be long, is kept after them.
pub(super) const CURRENT_CONVERSATION: &str =
    "SELECT id FROM conversation INDEXED BY conversation_current_unique
       WHERE participant_id = ?1 AND ended_at IS NULL";

/// The newest item of conversation `?1`, from its head's index alone.
pub(super) const NEWEST_ITEM: &str =
    "SELECT item FROM transcript_head INDEXED BY transcript_head_conversation_index
       WHERE conversation_id = ?1 ORDER BY item DESC LIMIT 1";

/// Whether there is project `?1`, by its row's id alone, none of its row read.
pub(super) const PROJECT_THERE: &str = "SELECT 1 FROM project WHERE id = ?";

/// Participant `?2` of project `?1`: its id and whether it left, from the index
/// that holds them, never from its row, which holds the roles it was given.
pub(super) const WHOSE: &str = "SELECT id, left_at IS NULL FROM participant
       INDEXED BY participant_history_index WHERE project_id = ?1 AND handle = ?2";

/// The head of the item at row `?3` of conversation `?2` of participant `?1`:
/// how many parts its text is kept in, and which copy of it its row holds;
/// none where it is not the participant's.
pub(super) const PART_HEAD: &str = "SELECT h.parts, h.revision FROM transcript_head h
         JOIN conversation c ON c.id = h.conversation_id
       WHERE c.participant_id = ?1 AND h.conversation_id = ?2 AND h.item = ?3";

/// The size in bytes of part `?2` of the item at row `?1`, from its row's
/// header alone.
pub(super) const PART_SIZE: &str =
    "SELECT octet_length(text) FROM transcript_part WHERE item = ?1 AND part = ?2";

/// Part `?2` of the item at row `?1`, its text: read past the part's end alone
/// ([`PART_TAIL`](super::super::transcripts::PART_TAIL) characters).
pub(super) const PART_TEXT: &str = "SELECT text FROM transcript_part WHERE item = ?1 AND part = ?2";

/// The change an edit made of the item at row `?1`, whose head says it made one.
pub(super) const DIFF_TEXT: &str = "SELECT text FROM transcript_diff WHERE item = ?1";

/// The end of part `?2` of the item at row `?1`, which its row holds before its text.
pub(super) const PART_END: &str = "SELECT tail FROM transcript_part WHERE item = ?1 AND part = ?2";

/// The text of the item at row `?1`, one kept in no parts: read past its id
/// alone, which is no longer than
/// [`ID_IN_ROW`](super::super::transcripts::ID_IN_ROW) bytes, as one whose id is
/// longer is kept in parts.
pub(super) const ROW_TEXT: &str = "SELECT text FROM transcript WHERE rowid = ?1";

/// Message `?1`'s parties: its project and recipient, which its row holds
/// before its body, and its sender's handle, which the participant's holds
/// before anything long. Nothing of the body, nor of the task.
pub(super) const MESSAGE_PARTIES: &str = "SELECT m.project_id, m.recipient_id, s.handle AS sender
       FROM message m LEFT JOIN participant s ON s.id = m.sender_id
       WHERE m.id = ?";

#[cfg(test)]
mod tests {
    use super::*;

    /// How SQLite goes about `sql`, a line a step.
    fn plan(db: &rusqlite::Connection, sql: &str) -> Vec<String> {
        let mut explained = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let count = explained.parameter_count();
        explained
            .query_map(rusqlite::params_from_iter(vec![1; count]), |row| row.get(3))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The log keeps no index the headings read (CHF-12): what began, ended or
    /// switched a conversation, and its tools' outputs counted apart.
    #[test]
    fn the_ledger_keeps_no_index_of_what_the_history_s_headings_read() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db, &crate::schema::MIGRATIONS).unwrap();
        let left: Vec<String> = db
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN ('event_activation_index',
                 'event_ended_index', 'event_participant_index', 'event_switch_index', 'transcript_head_tool_index')",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(left, Vec::<String>::new());
    }

    /// Every query of the history, and how SQLite goes about each: by a key or
    /// an index, never a scan; the participant and its conversations from an
    /// index alone (the participant's row holds its roles); an item's fields
    /// from its head, never its row (which holds its role past its id). Each
    /// other reads a row only up to a field that is short, and charged where it
    /// can be more than a few bytes.
    #[test]
    fn every_query_of_the_history_goes_by_a_key_or_an_index_and_reads_no_row_it_need_not() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db, &crate::schema::MIGRATIONS).unwrap();
        let head = |index: &str, to: &str| {
            format!("SEARCH transcript_head USING INDEX {index} (conversation_id=? AND rowid{to}?)")
        };
        let part = "SEARCH transcript_part USING INDEX sqlite_autoindex_transcript_part_1 (item=? AND part=?)";
        let by_row = |table: &str| format!("SEARCH {table} USING INTEGER PRIMARY KEY (rowid=?)");
        let queries: Vec<(String, Vec<String>)> = vec![
            (PROJECT_THERE.into(), vec![by_row("project")]),
            (
                WHOSE.into(),
                vec!["SEARCH participant USING COVERING INDEX participant_history_index (project_id=? AND handle=?)".into()],
            ),
            (
                conversation_from(true),
                vec!["SEARCH conversation USING COVERING INDEX conversation_participant_index (participant_id=? AND rowid<?)".into()],
            ),
            (
                conversation_from(false),
                vec!["SEARCH conversation USING COVERING INDEX conversation_participant_index (participant_id=? AND rowid>?)".into()],
            ),
            (rows_from(false, true, false), vec![head("transcript_head_said_index", "<")]),
            // SQLite says a bound the row itself is within as one it is not.
            (rows_from(false, false, true), vec![head("transcript_head_said_index", ">")]),
            (rows_from(true, true, true), vec![head("transcript_head_conversation_index", "<")]),
            (rows_from(true, false, false), vec![head("transcript_head_conversation_index", ">")]),
            (PART_SIZE.into(), vec![part.into()]),
            (PART_TEXT.into(), vec![part.into()]),
            (PART_END.into(), vec![part.into()]),
            (ROW_TEXT.into(), vec![by_row("transcript")]),
            (DIFF_TEXT.into(), vec![by_row("transcript_diff")]),
            (PART_HEAD.into(), vec![by_row("h"), by_row("c")]),
            (
                MESSAGE_PARTIES.into(),
                vec![by_row("m"), format!("{} LEFT-JOIN", by_row("s"))],
            ),
            (
                CURRENT_CONVERSATION.into(),
                vec!["SEARCH conversation USING INDEX conversation_current_unique (participant_id=?)".into()],
            ),
            (
                NEWEST_ITEM.into(),
                vec!["SEARCH transcript_head USING COVERING INDEX transcript_head_conversation_index (conversation_id=?)".into()],
            ),
        ];
        for (sql, expected) in queries {
            assert_eq!(plan(&db, &sql), expected, "{sql}");
        }
    }
}
