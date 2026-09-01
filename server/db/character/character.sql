-- character — persoenliche Charakterdaten und Fortschritt
-- Grundschema: bisherige character-Tabellen aus dem zentralen altschema.sql uebernommen.
-- Diese Datei wird NICHT vom Server ausgefuehrt (kein CREATE DATABASE);
-- das Anlegen der Datenbank liegt beim Betreiber.
-- Migrationen in migrations/ werden mit den eingeschaerften character-DB-Rechten angewendet.
-- Hinweis: accounts gehoert zur auth-DB (andere Verbindung); daher keine FK/JOIN zu auth.
--         account_id bleibt logisches Referenzfeld.
USE character;

CREATE TABLE IF NOT EXISTS characters (
  id INT AUTO_INCREMENT PRIMARY KEY,
  account_id INT NOT NULL,
  name VARCHAR(32) NOT NULL,
  race VARCHAR(16) NOT NULL,
  char_class VARCHAR(16) NOT NULL,
  level INT NOT NULL DEFAULT 1,
  exp INT NOT NULL DEFAULT 0,
  gold INT NOT NULL DEFAULT 0,
  hp INT NOT NULL DEFAULT 100,
  mana INT NOT NULL DEFAULT 50,
  strength INT NOT NULL DEFAULT 10,
  agility INT NOT NULL DEFAULT 10,
  intelligence INT NOT NULL DEFAULT 10,
  constitution INT NOT NULL DEFAULT 10,
  zone_id INT NOT NULL DEFAULT 0,
  pos_x FLOAT NOT NULL DEFAULT 0,
  pos_y FLOAT NOT NULL DEFAULT 0,
  guild_id INT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  KEY idx_char_account (account_id)
);
CREATE UNIQUE INDEX idx_char_name ON characters(name);

CREATE TABLE IF NOT EXISTS inventory (
  char_id INT NOT NULL,
  slot INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  cnt INT NOT NULL DEFAULT 1,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS skills (
  char_id INT NOT NULL,
  skill_id VARCHAR(48) NOT NULL,
  lvl INT NOT NULL DEFAULT 1,
  PRIMARY KEY (char_id, skill_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS quests (
  char_id INT NOT NULL,
  quest_id VARCHAR(48) NOT NULL,
  state TINYINT NOT NULL DEFAULT 0,
  data JSON NULL,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (char_id, quest_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS mail (
  id INT AUTO_INCREMENT PRIMARY KEY,
  to_id INT NOT NULL,
  from_id INT NULL,
  subject VARCHAR(120) NOT NULL,
  body JSON NULL,
  read_at TIMESTAMP NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (to_id) REFERENCES characters(id) ON DELETE CASCADE,
  FOREIGN KEY (from_id) REFERENCES characters(id) ON DELETE SET NULL
);
