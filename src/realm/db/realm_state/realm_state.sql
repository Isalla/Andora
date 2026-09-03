-- realm_state — persistenter Zustand eines konkreten Realms
-- Grundschema: bisherige realm-gebundene Tabellen des zentralen altschema.sql uebernommen
-- (guilds, guild_members, auctions). Ein Realm = eine eigene realm_state_<realm>-Datenbank
-- (z. B. realm_state_de1); diese Datei wird NICHT vom Server ausgefuehrt (kein CREATE DATABASE),
-- das Anlegen der Datenbank liegt beim Betreiber.
-- Migrationen in migrations/ werden mit den eingeschaerften realm-state-DB-Rechten angewendet.
-- Hinweis: Cross-DB-Bezuqe (characters, accounts) sind KEINE FOREIGN KEYS,
--         sondern Indexe/logische Referenzfelder (char_id, leader_id).
--         FKs gelten nur innerhalb der realm_state-DB selbst.
USE realm_state;

CREATE TABLE IF NOT EXISTS guilds (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(32) NOT NULL UNIQUE,
  leader_id INT NOT NULL,
  motto VARCHAR(120) NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  KEY idx_guilds_leader (leader_id)
);

CREATE TABLE IF NOT EXISTS guild_members (
  guild_id INT NOT NULL,
  char_id INT NOT NULL,
  role TINYINT NOT NULL DEFAULT 0,
  PRIMARY KEY (guild_id, char_id),
  FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
  KEY idx_guild_members_char (char_id)
);

CREATE TABLE IF NOT EXISTS auctions (
  id INT AUTO_INCREMENT PRIMARY KEY,
  seller_id INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  cnt INT NOT NULL DEFAULT 1,
  buyout INT NULL,
  bid INT NOT NULL DEFAULT 0,
  high_bidder INT NULL,
  state TINYINT NOT NULL DEFAULT 0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  expires_at TIMESTAMP NOT NULL,
  KEY idx_auctions_seller (seller_id)
);
CREATE INDEX idx_auction_state ON auctions(state, expires_at);
CREATE INDEX idx_auction_item ON auctions(item_id, state);
