-- schema.sql — MariaDB 10, Database: andora
CREATE DATABASE IF NOT EXISTS andora DEFAULT CHARSET utf8mb4;
USE andora;

CREATE TABLE accounts (
  id INT AUTO_INCREMENT PRIMARY KEY,
  username VARCHAR(32) NOT NULL UNIQUE,
  email VARCHAR(120) NOT NULL UNIQUE,
  pw_hash CHAR(60) NOT NULL,
  ban_until TIMESTAMP NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE characters (
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
  FOREIGN KEY (account_id) REFERENCES accounts(id)
);
CREATE UNIQUE INDEX idx_char_name ON characters(name);

CREATE TABLE inventory (
  char_id INT NOT NULL,
  slot INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  cnt INT NOT NULL DEFAULT 1,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE skills (
  char_id INT NOT NULL,
  skill_id VARCHAR(48) NOT NULL,
  lvl INT NOT NULL DEFAULT 1,
  PRIMARY KEY (char_id, skill_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE quests (
  char_id INT NOT NULL,
  quest_id VARCHAR(48) NOT NULL,
  state TINYINT NOT NULL DEFAULT 0,
  data JSON NULL,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (char_id, quest_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE guilds (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(32) NOT NULL UNIQUE,
  leader_id INT NOT NULL,
  motto VARCHAR(120) NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (leader_id) REFERENCES characters(id)
);

CREATE TABLE guild_members (
  guild_id INT NOT NULL,
  char_id INT NOT NULL,
  role TINYINT NOT NULL DEFAULT 0,
  PRIMARY KEY (guild_id, char_id),
  FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

CREATE TABLE auctions (
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
  FOREIGN KEY (seller_id) REFERENCES characters(id)
);
CREATE INDEX idx_auction_state ON auctions(state, expires_at);
CREATE INDEX idx_auction_item ON auctions(item_id, state);

CREATE TABLE mail (
  id INT AUTO_INCREMENT PRIMARY KEY,
  to_id INT NOT NULL,
  from_id INT NULL,
  subject VARCHAR(120) NOT NULL,
  body JSON NULL,
  read_at TIMESTAMP NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (to_id) REFERENCES characters(id) ON DELETE CASCADE
);
