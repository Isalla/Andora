-- 008_accounts_twofactor.sql — auth: TOTP-2FA-Felder auf accounts
-- two_factor_secret enthält NUR den verschlüsselten TOTP-Secret
-- (AES-256-GCM, Schlüssel aus config.env); der Klartext-Basiswert
-- (20 Zufallsbytes, Base32) wird niemals persistiert.
-- last_totp_counter / last_totp_at bilden den Replay-Schutz: ein Code
-- wird nur akzeptiert, wenn sein Zeitstempel-STRIKT GRÖSSER ist als
-- der zuletzt verbrauchte Counter.
ALTER TABLE accounts
  ADD COLUMN two_factor_enabled TINYINT(1) NOT NULL DEFAULT 0,
  ADD COLUMN two_factor_secret VARBINARY(1024) NULL,
  ADD COLUMN last_totp_counter INT NULL,
  ADD COLUMN last_totp_at TIMESTAMP NULL;
