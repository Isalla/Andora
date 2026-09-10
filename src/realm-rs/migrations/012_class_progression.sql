-- 012_class_progression.sql — realm_state: Klassenbasis (docs/Klassensystem.md).
--
-- Ergänzt um die minimale typsichere Klassenprogression:
--   faction_transition — Fraktions-Übergangs-Hook (L10-Regel):
--     Level 10 ist das Maximum im neutralen Startgebiet; darüber erst
--     nach Fraktionswahl und Übergang in ein Fraktionsgebiet. Das
--     Fraktions-/Zonensystem folgt technisch; diese Spalte ist der Hook.
--
-- char_class bleibt der stabile DB-Container: neuer Charakter startet als
-- 'Adventurer' (Kanon-Name, docs/Klassensystem.md); Bestandswerte
-- (deutsche Kernklassen, legacy Warrior/Mage, Unterklassen) werden beim
-- Laden tolerant zugeordnet (from_db_name). Die Tutorial-Phase L1–8 ist
-- levelabgeleitet für Adventurer und benötigt KEINE eigene Spalte.

ALTER TABLE characters ADD COLUMN IF NOT EXISTS faction_transition TINYINT(1) NOT NULL DEFAULT 0;
