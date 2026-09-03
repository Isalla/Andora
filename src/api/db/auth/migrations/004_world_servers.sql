-- 004_world_servers.sql — auth: registrierte World-Server pro Realm
-- credential: geteiltes Secret des World-Servers; ausschließlich der
-- Auth/Dienst liest diese Tabelle. Die API vergleicht es
-- constant-time gegen das vom Server vorgelegte Secret.
CREATE TABLE IF NOT EXISTS world_servers (
  id INT AUTO_INCREMENT PRIMARY KEY,
  realm_id INT NOT NULL,
  name VARCHAR(64) NOT NULL UNIQUE,
  host VARCHAR(128) NOT NULL,
  port INT NOT NULL,
  version VARCHAR(32) NOT NULL DEFAULT '',
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  max_players INT NOT NULL DEFAULT 100,
  credential VARCHAR(128) NOT NULL,
  status VARCHAR(16) NOT NULL DEFAULT 'offline',
  current_players INT NOT NULL DEFAULT 0,
  last_heartbeat TIMESTAMP NULL,
  CONSTRAINT fk_world_servers_realm FOREIGN KEY (realm_id) REFERENCES realms (id) ON DELETE CASCADE
);
