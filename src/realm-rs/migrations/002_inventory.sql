-- 002_inventory.sql — realm_state: Inventar (aus dem character-Übergangsstand übernommen).

CREATE TABLE IF NOT EXISTS inventory (
  char_id INT NOT NULL,
  slot INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  cnt INT NOT NULL DEFAULT 1,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);
