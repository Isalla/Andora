-- 029: Ausrüstungs-Achsen für Item-Definitionen (additiv, nicht-destruktiv).
-- Spezifikation: docs/item_properties.md „Ausrüstungs-Achsen" und
-- docs/inventory_system.md §7.1 (kanonische Subtyp-/Handling-Schreibweise).
-- NULL ist bei nicht ausrüstbaren Kategorien der Bestandzustand; die
-- Validierungspflicht für Weapon/Armor/Accessory liegt in ItemDefinition::
-- validate() (Auftrag A1_ITEM_AXEN). weapon_type bleibt unangetastet.
ALTER TABLE item_definitions
  ADD COLUMN equip_subtype VARCHAR(32) NULL,
  ADD COLUMN handling VARCHAR(32) NULL;
