-- A backup is another top-level participant, linked to the member it covers.  The
-- two can have the same model: their ids, windows and quota marks are their own.
-- A Vacation is independent of a quota hold.  The hold's kind is kept beside its
-- end so a timed busy hold cannot become a quota hold after a restart.
--
-- Existing timed holds are deliberately left with an unknown kind: their old
-- timestamp alone cannot distinguish a quota from a busy model.  The two special
-- ends used by the old engine do identify the account and moved-model holds.
-- All columns are nullable, so old projects, rows and history stay in place.
ALTER TABLE participant ADD COLUMN backup_of INTEGER REFERENCES participant(id);
ALTER TABLE participant ADD COLUMN vacation_since TEXT;
ALTER TABLE participant ADD COLUMN out_kind TEXT
  CHECK (out_kind IS NULL OR out_kind IN ('quota', 'account', 'busy', 'moved'));

UPDATE participant SET out_kind = CASE out_until
  WHEN '9999-12-31T23:59:59.998Z' THEN 'account'
  WHEN '9999-12-31T23:59:59.999Z' THEN 'moved'
  ELSE NULL END
WHERE out_until IS NOT NULL;

CREATE UNIQUE INDEX participant_one_live_backup ON participant(backup_of)
  WHERE backup_of IS NOT NULL AND left_at IS NULL;

-- Protect the relationship wherever a row is written, including a rejoin.
-- Departed rows keep their old links for history; only a live link is checked.
CREATE TRIGGER participant_backup_insert BEFORE INSERT ON participant
WHEN NEW.backup_of IS NOT NULL AND NEW.left_at IS NULL
BEGIN
  SELECT CASE WHEN NEW.member_id IS NOT NULL OR NEW.role IN ('human', 'chief')
    OR NEW.id = NEW.backup_of
    OR NOT EXISTS (
      SELECT 1 FROM participant owner WHERE owner.id = NEW.backup_of
        AND owner.project_id = NEW.project_id AND owner.member_id IS NULL
        AND owner.role NOT IN ('human', 'chief') AND owner.left_at IS NULL
        AND owner.backup_of IS NULL
    )
    OR EXISTS (SELECT 1 FROM participant child
      WHERE child.backup_of = NEW.id AND child.left_at IS NULL)
  THEN RAISE(ABORT, 'invalid backup relationship') END;
END;

CREATE TRIGGER participant_backup_update
BEFORE UPDATE OF backup_of, left_at, member_id, role, project_id ON participant
WHEN NEW.backup_of IS NOT NULL AND NEW.left_at IS NULL
BEGIN
  SELECT CASE WHEN NEW.member_id IS NOT NULL OR NEW.role IN ('human', 'chief')
    OR NEW.id = NEW.backup_of
    OR NOT EXISTS (
      SELECT 1 FROM participant owner WHERE owner.id = NEW.backup_of
        AND owner.project_id = NEW.project_id AND owner.member_id IS NULL
        AND owner.role NOT IN ('human', 'chief') AND owner.left_at IS NULL
        AND owner.backup_of IS NULL
    )
    OR EXISTS (SELECT 1 FROM participant child
      WHERE child.backup_of = NEW.id AND child.left_at IS NULL)
  THEN RAISE(ABORT, 'invalid backup relationship') END;
END;

-- A live primary cannot leave, become a backup or cease to be a member while
-- a live backup still names it.  The ledger takes the backup away first.
CREATE TRIGGER participant_backup_owner_update
BEFORE UPDATE OF backup_of, left_at, member_id, role, project_id ON participant
WHEN EXISTS (SELECT 1 FROM participant child WHERE child.backup_of = OLD.id AND child.left_at IS NULL)
  AND (NEW.left_at IS NOT NULL OR NEW.backup_of IS NOT NULL OR NEW.member_id IS NOT NULL
       OR NEW.role IN ('human', 'chief') OR NEW.project_id != OLD.project_id)
BEGIN
  SELECT RAISE(ABORT, 'member still has a backup');
END;
