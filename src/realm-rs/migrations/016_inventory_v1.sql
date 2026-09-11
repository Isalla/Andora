-- 016_inventory_v1.sql — realm_state: Inventory V1 (docs/inventory_system.md).
--
-- Verbindliches Datenmodell für:
-- - Grundinventar (8 Basis-Slots, Content-Wert)
-- - Ausrüstbare Rucksäcke / Taschen (eigene Slots, benennbar)
-- - Equipment- und Funktionsslots (21 Slots)
-- - Temporärer Sicherheits-Puffer (serverseitige Ausnahmefälle)
--
-- 1 Item oder 1 Stack = exakt 1 Slot. Keine Item-Größe, kein Multi-Slot.
-- Persistenz: Inventar + Bags + Bag-Namen + Equipment. Puffer temporär
-- (Inhalt beim Logout gelöscht). Alte 002_inventory.sql bleibt unverändert.
--
-- Item-Referenzen: item_instances(item_uuid) ON DELETE SET NULL.
-- Charakter-Referenzen: characters(id) ON DELETE CASCADE.

-- --- Grundinventar (Basis-Slots) ---
CREATE TABLE IF NOT EXISTS character_inventory (
  char_id INT NOT NULL,
  slot INT NOT NULL,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  KEY idx_ci_uuid (item_uuid),
  CONSTRAINT fk_ci_char FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE,
  CONSTRAINT fk_ci_item FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE SET NULL
);

-- --- Ausrüstbare Rucksäcke / Taschen (Container-Definition) ---
-- bag_id ist lokal pro Charakter (1, 2, 3 …), kein globaler Auto-Inkrement.
CREATE TABLE IF NOT EXISTS character_bags (
  char_id INT NOT NULL,
  bag_id INT NOT NULL,
  name VARCHAR(48) NOT NULL,
  slot_count INT NOT NULL,
  PRIMARY KEY (char_id, bag_id),
  KEY idx_cb_char (char_id),
  CONSTRAINT fk_cb_char FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);

-- --- Bag-Slots (Inhalt der einzelnen Rucksäcke) ---
CREATE TABLE IF NOT EXISTS bag_slots (
  char_id INT NOT NULL,
  bag_id INT NOT NULL,
  slot INT NOT NULL,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, bag_id, slot),
  KEY idx_bs_uuid (item_uuid),
  CONSTRAINT fk_bs_bag FOREIGN KEY (char_id, bag_id) REFERENCES character_bags(char_id, bag_id) ON DELETE CASCADE,
  CONSTRAINT fk_bs_item FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE SET NULL
);

-- --- Equipment- und Funktionsslots (21 Slots) ---
CREATE TABLE IF NOT EXISTS character_equipment (
  char_id INT NOT NULL,
  slot VARCHAR(32) NOT NULL,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  KEY idx_ce_uuid (item_uuid),
  CONSTRAINT fk_ce_char FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE,
  CONSTRAINT fk_ce_item FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE SET NULL
);

-- --- Temporärer Sicherheits-Puffer (Inhalt beim Logout gelöscht) ---
CREATE TABLE IF NOT EXISTS inventory_buffer (
  char_id INT NOT NULL,
  slot INT NOT NULL,
  item_uuid VARCHAR(64) NULL,
  PRIMARY KEY (char_id, slot),
  KEY idx_ib_uuid (item_uuid),
  CONSTRAINT fk_ib_char FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE,
  CONSTRAINT fk_ib_item FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE SET NULL
);
