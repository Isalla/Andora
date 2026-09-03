-- 005_recoveries.sql — auth: Einmal-Passwort-Reset-Tokens
-- used_at NULL = noch nicht verbraucht; Verbrauch + neue Passwort-
-- Einstellung + Ban-Kläung geschehen in einer Transaktion.
CREATE TABLE IF NOT EXISTS recoveries (
  token_hash CHAR(64) NOT NULL PRIMARY KEY,
  account_id INT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  expires_at TIMESTAMP NOT NULL,
  used_at TIMESTAMP NULL,
  CONSTRAINT fk_recoveries_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
