-- 002_sessions.sql — auth: persistierte Login-Sessionen
-- token_hash: SHA-256 des Zufallstokens (der rohe Token wird NIE in der
-- DB gespeichert). Abgleich und Löschung erfolgen ausschließlich über
-- den Hash.
CREATE TABLE IF NOT EXISTS sessions (
  token_hash CHAR(64) NOT NULL PRIMARY KEY,
  account_id INT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  expires_at TIMESTAMP NOT NULL,
  KEY idx_sessions_account (account_id),
  KEY idx_sessions_expires (expires_at),
  CONSTRAINT fk_sessions_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
