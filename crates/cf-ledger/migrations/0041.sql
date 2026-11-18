-- A worker's own done suspends its unanswered questions until a same-window reopen.
ALTER TABLE message ADD COLUMN done_question_state TEXT;
ALTER TABLE message ADD COLUMN done_question_reason TEXT;
