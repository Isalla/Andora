-- 010_recovery_codes.sql — auth: 2FA-Recovery-Codes (10 pro Setup)
-- Einmalig: used_at NULL = noch verfügbar, Verbrauch löscht die
-- Nutzbarkeit. Der Klartext-Code wird NIE gespeichert; nur der
-- SHA-256 des normalisierten Codes (Großbuchstaben, Trenner
-- entfernt) lebt in der Datenbank.
CREATE TABLE IF NOT EXISTS recovery_codes (
  token_hash CHAR(64) NOT NULL PRIMARY KEY,
  account_id INT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  used_at TIMESTAMP NULL,
  CONSTRAINT fk_recovery_codes_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
