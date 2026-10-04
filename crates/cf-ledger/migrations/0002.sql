
  ALTER TABLE task ADD COLUMN taken_from_id INTEGER
    CONSTRAINT task_taken_from_fk REFERENCES participant (id) ON DELETE SET NULL;
  