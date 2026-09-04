-- 012_trusted_device_last_used.sql — auth: 30-Tage-Inaktivitätsverfall
-- Der Verfall lebt ausschließlich in trusted_devices (kein Ablauf-Feld
-- in accounts; der Account verfällt dadurch nicht).
-- last_used_at wird bei jedem erfolgreichen Login mit dem erkannten
-- Gerät aktualisiert. Bereits bestätigte Geräte bekommen den
-- Bestätigungszeitstempel (das Gerät war zuletzt bei der Bestätigung
-- aktiv).
-- Nach 30 Tagen Inaktivität wird das Gerät nicht mehr als bestätigtes
-- Gerät erkannt (2FA-Bypass entfällt) und befreit beim nächsten
-- Gerät-Flow seinen Platz im 3er-Limit.
ALTER TABLE trusted_devices
  ADD COLUMN last_used_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP;

UPDATE trusted_devices
  SET last_used_at = confirmed_at
  WHERE last_used_at <> confirmed_at;
