-- The human may have the chief do the work itself. Existing projects keep the usual role.
  ALTER TABLE project ADD COLUMN own_hands INTEGER NOT NULL DEFAULT 0;
