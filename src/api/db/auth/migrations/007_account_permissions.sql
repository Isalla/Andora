-- 007_account_permissions.sql — auth: pro Account freigegebene Berechtigungen
-- Geschlossene Menge (siehe src/api/auth.go, Permission-Constanten).
CREATE TABLE IF NOT EXISTS account_permissions (
  account_id INT NOT NULL,
  permission VARCHAR(64) NOT NULL,
  granted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (account_id, permission),
  CONSTRAINT fk_account_permissions_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
