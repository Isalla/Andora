-- 017_loot_v1.sql — realm_state: Loot V1 (docs/Lootsystem.md).
--
-- Verbindliches Datenmodell für Server-autoritativen Loot:
--   loot_tables       — benannte Loot-Tabellen (Content-Schicht).
--   loot_entries      — Einträge je Tabelle: item | gold | chest mit
--                       min/max-Menge und unabhängiger Dropchance (0..1).
--                       Chest-Einträge referenzieren eine Inhaltstabelle
--                       (content_table_id), deren Loot beim Chest-Spawn
--                       ausgewürfelt und serverseitig gespeichert wird.
--   monster_definitions.loot_table_id — nullable; ohne Wert kein Loot.
--
-- Unabhängige Würfe: Jeder Eintrag wird pro Kill separat gewürfelt (es gibt
-- kein "genau ein Eintrag"-System, kein Pity/Luck-Ausgleich).
--
-- Item-Referenzen: loot_entries.item_id → item_definitions(id).
-- Chest-Inhalte sind V1 nur item/gold (eine Chest in einer Inhaltstabelle
-- wird beim Rollen ignoriert; die Hülle bleibt flach erweiterbar).

-- ──────────────────────────────────────────────────────────────────────
-- 1) Loot-Tabellen
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS loot_tables (
  id INT NOT NULL PRIMARY KEY AUTO_INCREMENT,
  name VARCHAR(120) NOT NULL
);

-- ──────────────────────────────────────────────────────────────────────
-- 2) Loot-Einträge
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS loot_entries (
  id INT NOT NULL PRIMARY KEY AUTO_INCREMENT,
  loot_table_id INT NOT NULL,
  -- item | gold | chest
  kind VARCHAR(16) NOT NULL,
  -- item: die fallengelassene Definition (NULL bei gold/chest).
  item_id VARCHAR(48) NULL,
  -- Mengenbereich je Wurf (item = Stückzahl, gold = Geldmenge).
  min_quantity INT NOT NULL DEFAULT 1,
  max_quantity INT NOT NULL DEFAULT 1,
  -- Unabhängige Dropchance (0..1) je Kill.
  chance DOUBLE NOT NULL DEFAULT 1.0,
  -- chest: Inhaltstabelle, aus der beim Chest-Spawn einmalig gerollt wird.
  content_table_id INT NULL,
  FOREIGN KEY (loot_table_id) REFERENCES loot_tables(id) ON DELETE CASCADE,
  FOREIGN KEY (item_id) REFERENCES item_definitions(id) ON DELETE SET NULL,
  FOREIGN KEY (content_table_id) REFERENCES loot_tables(id) ON DELETE SET NULL
);

-- ──────────────────────────────────────────────────────────────────────
-- 3) Monster-Definition: optionale Loot-Tabelle (NULL = kein Loot)
-- ──────────────────────────────────────────────────────────────────────
ALTER TABLE monster_definitions ADD COLUMN IF NOT EXISTS loot_table_id INT NULL;

-- ──────────────────────────────────────────────────────────────────────
-- 4) SEED: Probe-Inhalte (vertikaler Schnitt, Zone 0).
-- Werte sind PROVISORISCH und dienen ausschließlich dem Testen der
-- Core-Logik — kein End-Balancing (docs/Lootsystem.md "Noch nicht
-- festgelegte Werte"). Der Eber lässt bewusst keinen Loot fallen.
-- ──────────────────────────────────────────────────────────────────────

-- Klein-Content-Items (Rohstoffe/Nahrung, standard-stackbar).
INSERT INTO item_definitions (id, name, category, rarity, item_level, base_quality, max_stack, weight) VALUES
('wolf_hide',  'Wolfspelz',      'raw_material', 'common', 1, 100, 100, 0.1),
('wolf_meat',  'Wolfhackfleisch', 'food',        'common', 1, 100, 50,  0.2),
('boar_tusk',  'Eberzahn',       'raw_material', 'common', 1, 100, 100, 0.1),
('iron_ore',   'Eisenerz',       'raw_material', 'common', 1, 100, 100, 0.5);

-- Loot-Tabellen.
INSERT INTO loot_tables (id, name) VALUES
(1, 'wolf_loot'),
(2, 'alpha_loot'),
(3, 'wurmlord_loot'),
(4, 'simple_chest'),
(5, 'boss_chest');

-- Wolf: Gold + Wolfspelz + Fleisch.
INSERT INTO loot_entries (loot_table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) VALUES
(1, 'item', 'wolf_hide', 1, 3, 0.80, NULL),
(1, 'item', 'wolf_meat', 2, 5, 0.70, NULL),
(1, 'gold', NULL,       5, 20, 0.90, NULL);

-- Alphawolf (named): bessere Menge + einfache Truhe (20 %).
INSERT INTO loot_entries (loot_table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) VALUES
(2, 'item', 'wolf_hide', 3, 5, 1.00, NULL),
(2, 'gold', NULL,       20, 60, 1.00, NULL),
(2, 'chest', NULL,       0, 0, 0.20, 4);

-- Wurmlord (boss): viel Gold + Boss-Truhe (guaranteed), dazu Erze.
INSERT INTO loot_entries (loot_table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) VALUES
(3, 'item', 'iron_ore',  5, 10, 1.00, NULL),
(3, 'gold', NULL,      100, 400, 1.00, NULL),
(3, 'chest', NULL,       0, 0, 1.00, 5);

-- Einfache Truhe (Inhaltstabelle): geringe Menge, Gold.
INSERT INTO loot_entries (loot_table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) VALUES
(4, 'item', 'wolf_hide', 1, 2, 0.60, NULL),
(4, 'gold', NULL,        10, 50, 0.80, NULL);

-- Boss-Truhe (Inhaltstabelle): mehr Inhalt, mehr Gold.
INSERT INTO loot_entries (loot_table_id, kind, item_id, min_quantity, max_quantity, chance, content_table_id) VALUES
(5, 'item', 'iron_ore',  3, 8, 0.90, NULL),
(5, 'item', 'wolf_meat', 4, 8, 0.60, NULL),
(5, 'gold', NULL,        30, 120, 0.90, NULL);

-- Monster-UPDATE: Wolf/Alphawolf/Wurmlord erhalten Loot; der Eber (boar)
-- demonstriert "Monster ohne Loot" und bleibt unverändert (NULL).
UPDATE monster_definitions SET loot_table_id = 1 WHERE id = 'wolf';
UPDATE monster_definitions SET loot_table_id = 2 WHERE id = 'wolfpack_named';
UPDATE monster_definitions SET loot_table_id = 3 WHERE id = 'wurmlohr_boss';