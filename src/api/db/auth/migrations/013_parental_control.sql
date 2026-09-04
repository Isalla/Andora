-- 013_parental_control.sql — auth: Elternkontrolle (Migration 013)
-- Elternkontrolle ist accountgebunden und wird vollständig serverseitig
-- erzwungen (Auth/API). Der Account erhält nur das Flag
-- parental_control_enabled (TINYINT, Default 0); alle Regelwerke,
-- Ausnahmen, Ferienzeiträume, Tages-Zähler und Benachrichtigungen
-- leben in eigenen Tabellen. KEIN Auth-DB-Zugriff aus dem Realm.
--
-- PIN: Nur der Argon2id-Hash wird persistiert (parent_pin_hash).
-- Eltern-E-Mail: Nur verschlüsselt (AES-256-GCM, Schlüssel aus config.env),
-- KEIN rückführbarer Lookup-Hash. Benachrichtigungen werden als DB-Rezepte
-- gespeichert; kein Mail-Engine in diesem Service.

-- Account-Flag: Elternkontrolle aktiv?
ALTER TABLE accounts
  ADD COLUMN parental_control_enabled TINYINT(1) NOT NULL DEFAULT 0;

-- Elternkontrolle eines Accounts: PIN-Hash, (optional) verschlüsselte
-- Eltern-E-Mail, wochenweise Tageslimits in Minuten, Warnschwelle.
CREATE TABLE parental_controls (
  account_id          INT NOT NULL PRIMARY KEY,
  parent_pin_hash     VARCHAR(255) NOT NULL,
  parent_email_enc    VARBINARY(512) NULL,
  monday_minutes      SMALLINT NOT NULL DEFAULT 0,
  tuesday_minutes     SMALLINT NOT NULL DEFAULT 0,
  wednesday_minutes   SMALLINT NOT NULL DEFAULT 0,
  thursday_minutes    SMALLINT NOT NULL DEFAULT 0,
  friday_minutes      SMALLINT NOT NULL DEFAULT 0,
  saturday_minutes    SMALLINT NOT NULL DEFAULT 0,
  sunday_minutes      SMALLINT NOT NULL DEFAULT 0,
  chat_enabled        TINYINT(1) NOT NULL DEFAULT 1,
  voice_enabled       TINYINT(1) NOT NULL DEFAULT 1,
  warning_minutes     SMALLINT NOT NULL DEFAULT 30,
  updated_at          TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE parental_controls
  ADD CONSTRAINT fk_parental_controls_account
  FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE;

-- Ferienzeiträume (Sonderzeiträume): eigene Wochenlimits für ein Datum
-- Fenster. Aktive Zeitfenster dürfen nicht überlappen (wird im Store
-- verifiziert, nicht durch DB-Constraint).
CREATE TABLE parental_control_periods (
  id                INT AUTO_INCREMENT PRIMARY KEY,
  account_id        INT NOT NULL,
  starts_at         DATE NOT NULL,
  ends_at           DATE NOT NULL,
  monday_minutes    SMALLINT NOT NULL DEFAULT 0,
  tuesday_minutes   SMALLINT NOT NULL DEFAULT 0,
  wednesday_minutes SMALLINT NOT NULL DEFAULT 0,
  thursday_minutes  SMALLINT NOT NULL DEFAULT 0,
  friday_minutes    SMALLINT NOT NULL DEFAULT 0,
  saturday_minutes  SMALLINT NOT NULL DEFAULT 0,
  sunday_minutes    SMALLINT NOT NULL DEFAULT 0,
  created_at        TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_pc_period_account (account_id, starts_at, ends_at),
  CONSTRAINT fk_pc_period_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);

-- Tagesausnahmen (+extra Minuten ODER Override des Tageslimits oder
-- Limit-Aufhebung). Highest priority: weekly < period < exception.
CREATE TABLE parental_control_exceptions (
  id              INT AUTO_INCREMENT PRIMARY KEY,
  account_id      INT NOT NULL,
  exception_date  DATE NOT NULL,
  extra_minutes   SMALLINT NULL DEFAULT NULL,
  override_minutes INT NULL DEFAULT NULL,
  created_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_pc_exc_account_date (account_id, exception_date),
  CONSTRAINT fk_pc_exc_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);

-- Tages-Zähler pro Account/Calendar-Day: server-side time.
-- used_seconds = bereits verbrauchte Spielzeit heute.
-- extended_at ist NICHT NULL nach einmaligem +1h (persists over
-- logout/reconnect).
-- buffer_started_at: Puffer-Einstieg (15/30 min), bleibt gesetzt bis
-- Puffer endet ODER Logout.
CREATE TABLE parental_daily_usage (
  account_id        INT NOT NULL,
  usage_date        DATE NOT NULL,
  used_seconds      INT NOT NULL DEFAULT 0,
  extended_at       TIMESTAMP NULL,
  buffer_started_at TIMESTAMP NULL,
  last_poll_at      TIMESTAMP NULL,
  day_minutes_limit INT NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, usage_date),
  CONSTRAINT fk_pc_usage_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);

-- Benachrichtigungs-Rezepte (kein Mail-Engine): eine DB-Zeile pro
-- Änderungs-Meldung. recipient_email_enc = verschlüsselte Eltern-E-Mail
-- (die betroffene Adresse, nicht zwingend die neu hinterlegte).
CREATE TABLE parental_notifications (
  id                     INT AUTO_INCREMENT PRIMARY KEY,
  account_id             INT NOT NULL,
  event_type             VARCHAR(64) NOT NULL,
  setting_name           VARCHAR(64) NOT NULL,
  old_value              VARCHAR(255) NOT NULL,
  new_value              VARCHAR(255) NOT NULL,
  created_at             TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  recipient_email_enc    VARBINARY(512) NULL,
  delivered_at           TIMESTAMP NULL,
  CONSTRAINT fk_pc_notif_account FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
);
CREATE INDEX idx_pc_notif_account ON parental_notifications (account_id, delivered_at);
