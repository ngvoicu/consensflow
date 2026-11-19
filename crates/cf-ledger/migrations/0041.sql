-- A held member's login revision is a fingerprint of metadata, never login contents.
ALTER TABLE participant ADD COLUMN hold_login_revision TEXT;
ALTER TABLE participant ADD COLUMN setup_generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE participant ADD COLUMN hold_generation INTEGER NOT NULL DEFAULT 0;
