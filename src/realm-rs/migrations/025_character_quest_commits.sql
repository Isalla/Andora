-- 025_character_quest_commits.sql — realm_state: additive quest-completion receipt.
-- The full canonical completion order (not just a revision or hash) is written in
-- the SAME transaction as the full snapshot and the ACTIVE->COMPLETED row. Covers
-- commit-before-file-receipt crash. No FK/automatic expiry: removing a character
-- must not erase a completion identity. The UNIQUE (char_id, quest_id) key makes
-- the pair unique for this non-repeatable V1 scope; a divergent second completion
-- for the same pair fails here and the whole transaction is rolled back (conflict).
-- Created only; not executed by this coding job.
CREATE TABLE IF NOT EXISTS character_quest_commits (
  commit_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  char_id INT NOT NULL,
  quest_id VARCHAR(48) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  artifact_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  PRIMARY KEY (commit_id),
  UNIQUE (char_id, quest_id)
);
