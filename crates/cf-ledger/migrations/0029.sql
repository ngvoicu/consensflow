-- The history viewer reads a participant's history as one timeline of items, with no
-- heading of any conversation (chief-harness, CHF-11 and CHF-12): nothing reads the log
-- for where a conversation began, was gone to or ended, or for the chief's switches, and
-- nothing counts a conversation's tools' outputs apart. The indexes 0020 made for those
-- reads go, as no read uses them and each write of what they hold kept them up. The log
-- itself and every other index of 0020 stay as they are.
  DROP INDEX event_activation_index;
  DROP INDEX event_ended_index;
  DROP INDEX event_participant_index;
  DROP INDEX event_switch_index;
  DROP INDEX transcript_head_tool_index;
