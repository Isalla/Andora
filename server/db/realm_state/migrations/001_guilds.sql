-- 001_guilds.sql — realm_state: Gilden + Guild-Mitgliedschaften (aus altschema.sql uebernommen)
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
