-- 009_trusted_devices.sql — auth: bestätigte Geräte pro Account
-- token_hash = SHA-256 des kryptografisch zufälligen Device-Tokens
-- (32 Zufallsbytes, 64-hex); der Klartext-Tokentext existiert nur
-- für die Dauer der Login-Antwort und wird nie gespeichert.
-- Die Max-3-Regel ist KEINE DB-Constraint, sondern Service-Regel:
-- sie muss in der Logik geprüft und über 409 (max_devices_reached)
-- abgebildet werden, da Widerruf VOR Neubestätigung stehen muss.
CREATE TABLE IF NOT EXISTS trusted_devices (
  id INT AUTO_INCREMENT PRIMARY KEY,
  account_id INT NOT NULL,
  token_hash CHAR(64) NOT NULL UNIQUE,
  label VARCHAR(64) NOT NULL,
  confirmed_at TIMESTAMP NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_trusted_devices_account (account_id),
  CONSTRAINT fk_trusted_devices_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
