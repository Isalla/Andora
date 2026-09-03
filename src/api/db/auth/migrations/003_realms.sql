-- 003_realms.sql — auth: registrierte Realms (Betriebsdaten)
CREATE TABLE IF NOT EXISTS realms (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(64) NOT NULL UNIQUE,
  language VARCHAR(8) NOT NULL,
  region VARCHAR(16) NOT NULL,
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  fresh_start_until TIMESTAMP NULL,
  transfer_policy VARCHAR(32) NOT NULL DEFAULT 'standard'
);
