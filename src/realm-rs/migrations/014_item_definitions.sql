-- 014_item_definitions.sql — realm_state: statische Item-Definitionen (Item System V1).
--
-- Eine Definition beschreibt, was ein Item grundsätzlich ist, und trägt die
-- STATISCHEN Basiswerte. Dynamische Abweichungen einzelner Exemplare gehören
-- ausschließlich auf die Instanz (item_instances, Migration 015) als
-- Modifikation. Effektiver Wert = Basiswert + Instanzmodifikation.
--
-- Inhaltswerte sind bewusst VARCHAR/DOUBLE statt geschlossener Enums, damit
-- die Kategorien-/Rarity-/Bindungsmengen über Content erweiterbar bleiben.
-- Die Rust-Schicht validiert die gültigen Werte beim Laden (item.rs).

CREATE TABLE IF NOT EXISTS item_definitions (
  id VARCHAR(48) NOT NULL PRIMARY KEY,
  name VARCHAR(160) NOT NULL,
  description VARCHAR(512) NULL,
  category VARCHAR(32) NOT NULL,
  rarity VARCHAR(32) NOT NULL DEFAULT 'common',
  item_level INT NOT NULL DEFAULT 1,
  -- Numerische Material-/Basisqualität (Percent-Ausgangswert, meist 100).
  base_quality DOUBLE NOT NULL DEFAULT 100,
  -- 1 Item oder 1 Stack belegt genau 1 Slot; max_stack ist Eigenschaft der
  -- Definition. Default je Kategorie in item.rs, pro Definition überschreibbar.
  max_stack INT NOT NULL DEFAULT 1,
  -- Gewicht: bei stackbaren Items eines VOLLEN Stacks; bei max_stack = 1 das
  -- normale Itemgewicht.
  weight DOUBLE NOT NULL DEFAULT 0,
  -- Waffe (nur für Kategorie weapon): Grundschaden, Duration zwischen
  -- automatischen Grundangriffen (ms), Reichweite.
  base_damage DOUBLE NULL,
  duration_ms INT NULL,
  range DOUBLE NULL,
  weapon_type VARCHAR(32) NULL,
  -- Rüstung (nur für Kategorie armor): Basis-Rüstungswert.
  armor_value DOUBLE NULL,
  -- Equip-Voraussetzungen: minimum_level (NULL = keine) und erlaubte Klassen
  -- (Satellitentabelle item_definition_classes; leer = alle Klassen). Keine
  -- Attributanforderungen (docs/Kampfsystem.md, item_properties.md).
  min_level INT NULL,
  -- Bindungsregel der Definition: tradeable | bind_on_pickup | bind_on_equip.
  binding_rule VARCHAR(32) NOT NULL DEFAULT 'tradeable'
);

-- Erlaubte Klassen je Definition (NULL/leer = keine Klassenbeschränkung).
-- Werte = kanonische ClassStatus-DB-Namen (Klasse wird WIRKLICH wiederverwendet,
-- keine zweite parallele Klassenarchitektur). Wertebereich in class.rs.
CREATE TABLE IF NOT EXISTS item_definition_classes (
  item_id VARCHAR(48) NOT NULL,
  class VARCHAR(16) NOT NULL,
  PRIMARY KEY (item_id, class),
  FOREIGN KEY (item_id) REFERENCES item_definitions(id) ON DELETE CASCADE
);

-- Basis-Attributboni der Definition (7 Grundattribute, erweiterbar).
CREATE TABLE IF NOT EXISTS item_definition_attributes (
  item_id VARCHAR(48) NOT NULL,
  attribute VARCHAR(32) NOT NULL,
  bonus DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (item_id, attribute),
  FOREIGN KEY (item_id) REFERENCES item_definitions(id) ON DELETE CASCADE
);

-- Basis-Resistenzen der Definition (erweiterbar, z. B. fire/frost/poison).
CREATE TABLE IF NOT EXISTS item_definition_resistances (
  item_id VARCHAR(48) NOT NULL,
  resistance VARCHAR(32) NOT NULL,
  bonus DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (item_id, resistance),
  FOREIGN KEY (item_id) REFERENCES item_definitions(id) ON DELETE CASCADE
);