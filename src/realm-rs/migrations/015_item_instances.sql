-- 015_item_instances.sql — realm_state: individuelle Item-Instanzen (Item System V1).
--
-- Eine Instanz ist ein konkretes individuelles Exemplar einer Definition
-- (item_id) mit eigener item_uuid. Sie speichert ausschließlich individuellen
-- Zustand, NICHTS als zweite Kopie der statischen Basiswerte. Effektiver Wert
-- = Basiswert (014) + Modifikation (015). Bei Erstellung berechnete Werte
-- werden persistent hier abgelegt und nicht bei jedem Laden rekonstruiert.
--
-- Spieler-/NPC-gecraftetes Equipment ist immer eine individuelle Instanz.

CREATE TABLE IF NOT EXISTS item_instances (
  item_uuid VARCHAR(64) NOT NULL PRIMARY KEY,
  item_id VARCHAR(48) NOT NULL,
  -- Stackgröße dieses Exemplars (1..=max_stack der Definition). Keine UUID
  -- pro einzelner Einheit eines normalen Stacks — ein Stack = ein Datensatz.
  count INT NOT NULL DEFAULT 1,
  -- Haltbarkeit (nur individuelles Equipment): current 0 = defekt, nicht
  -- zerstört, Stats deaktiviert (is_broken/stats_active). NULL = keine
  -- Haltbarkeit. max_durability wird später abgeleitet (noch keine Formel).
  durability_current INT NULL,
  durability_max INT NULL,
  -- Bindungszustand der Instanz: tradeable | bind_on_pickup | bind_on_equip
  -- | bound (bereits charaktergebunden, z. B. Quest-/Boss-Item).
  binding VARCHAR(32) NOT NULL DEFAULT 'tradeable',
  -- Hersteller-/Creator-Referenz (Charakter, der das Item erstellt hat).
  creator_id INT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_item_instances_item (item_id),
  CONSTRAINT fk_item_instances_def FOREIGN KEY (item_id) REFERENCES item_definitions(id),
  CONSTRAINT fk_item_instances_creator FOREIGN KEY (creator_id) REFERENCES characters(id) ON DELETE SET NULL
);

-- Dynamische Modifikatoren der Instanz (effektiv = Basiswert + Modifikator).
-- Genau eine Zeile je Instanz (kann weggelassen werden, wenn alle 0 sind).
CREATE TABLE IF NOT EXISTS item_instance_modifiers (
  item_uuid VARCHAR(64) NOT NULL PRIMARY KEY,
  damage_modifier DOUBLE NOT NULL DEFAULT 0,
  armor_modifier DOUBLE NOT NULL DEFAULT 0,
  -- berechnetes Gewicht nicht-stackbarer gecrafteter Items = 75 % der
  -- Gesamtmasse der verbrauchten Materialien (einmalig beim Crafting gesetzt).
  weight_modifier DOUBLE NOT NULL DEFAULT 0,
  -- Numerische Qualität (Crafting-Ergebnis 75/100/125 % Bereich bzw. delta) —
  -- Dezimalpräzision intern erhalten.
  quality_modifier DOUBLE NOT NULL DEFAULT 0,
  FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE CASCADE
);

-- Modifikatoren der 7 Grundattribute je Instanz.
CREATE TABLE IF NOT EXISTS item_instance_attribute_modifiers (
  item_uuid VARCHAR(64) NOT NULL,
  attribute VARCHAR(32) NOT NULL,
  modifier DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (item_uuid, attribute),
  FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE CASCADE
);

-- Modifikatoren der Resistenzen je Instanz (erweiterbar).
CREATE TABLE IF NOT EXISTS item_instance_resistance_modifiers (
  item_uuid VARCHAR(64) NOT NULL,
  resistance VARCHAR(32) NOT NULL,
  modifier DOUBLE NOT NULL DEFAULT 0,
  PRIMARY KEY (item_uuid, resistance),
  FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE CASCADE
);