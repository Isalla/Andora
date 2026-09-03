-- 011_security_events.sql — auth: Sicherheits-Event-Protokoll
-- Geschlossene Menge von Event-Typen (Konstanten in twofactor.go):
-- device_confirmed, device_revoked, two_factor_enabled,
-- two_factor_disabled, two_factor_reset, password_changed,
-- recovery_code_used. account_id NULL erlaubt Events ohne Account
-- (z. B. abgewiesene Login-Versuche).
CREATE TABLE IF NOT EXISTS security_events (
  id INT AUTO_INCREMENT PRIMARY KEY,
  event_type VARCHAR(32) NOT NULL,
  account_id INT NULL,
  created_at TIMESTAMP NOT NULL,
  KEY idx_security_events_account (account_id),
  CONSTRAINT fk_security_events_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
