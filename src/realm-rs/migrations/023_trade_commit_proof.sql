-- 023_trade_commit_proof.sql — realm_state: atomic two-character commit proof.
-- The full canonical artifact (not just a revision or hash) is written in the
-- SAME transaction as both characters. Covers commit-before-file-receipt crash.
-- No FK/automatic expiry: removing a character must not erase a commit identity.
-- Created only; not executed by this coding job.
CREATE TABLE IF NOT EXISTS character_trade_commits (
  commit_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  artifact_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  PRIMARY KEY (commit_id)
);
