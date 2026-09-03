-- 006_handoffs.sql — auth: Einmal-Spielerübergabe-Tokens
-- Binden einen Account an ein Realm; ein World-Server darf ein Token
-- genau einmal validieren (Validate + Use atomar).
CREATE TABLE IF NOT EXISTS handoffs (
  token_hash CHAR(64) NOT NULL PRIMARY KEY,
  account_id INT NOT NULL,
  realm_id INT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  expires_at TIMESTAMP NOT NULL,
  used_at TIMESTAMP NULL,
  CONSTRAINT fk_handoffs_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
  CONSTRAINT fk_handoffs_realm FOREIGN KEY (realm_id) REFERENCES realms (id) ON DELETE CASCADE
);
