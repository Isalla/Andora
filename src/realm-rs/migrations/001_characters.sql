-- 001_characters.sql — realm_state: Charakter-Grunddaten.
-- Zielarchitektur: Charaktertabellen liegen in der jeweiligen
-- realm_state_<realm>-Datenbank (aus dem character-Übergangsstand
-- übernommen; dort 001_characters.sql).
-- Hinweis: accounts liegt in der auth-DB (eigene Verbindung), daher
-- logisches account_id ohne FOREIGN KEY.

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
