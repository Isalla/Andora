-- 011_security_events.sql — auth: Sicherheits-Event-Protokoll
--
-- Event-Typen werden NICHT durch das Schema erzwungen: event_type ist
-- VARCHAR(32) ohne CHECK/ENUM, diese Liste ist Dokumentation, keine
-- abgeschlossene Menge. Aktuell verwendete Konstanten:
--   store.go:      device_confirmed, device_revoked, two_factor_enabled,
--                  two_factor_disabled, two_factor_reset, password_changed,
--                  password_reset, recovery_code_used
--   parental.go:   parental_setup, parental_settings, parental_removed,
--                  parental_pin_changed, parental_email_changed,
--                  parental_email_removed, parental_period,
--                  parental_exception
--
-- Hinweis: die Parental-Einträge werden best-effort geschrieben (P-35); ein
-- Verlust dieser Zeilen ist kein Fehler des fachlichen Vorgangs.
-- account_id NULL erlaubt Events ohne Account (z. B. abgewiesene
-- Login-Versuche).
CREATE TABLE IF NOT EXISTS security_events (
  id INT AUTO_INCREMENT PRIMARY KEY,
  event_type VARCHAR(32) NOT NULL,
  account_id INT NULL,
  created_at TIMESTAMP NOT NULL,
  KEY idx_security_events_account (account_id),
  CONSTRAINT fk_security_events_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
