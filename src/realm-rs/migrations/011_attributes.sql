-- 011_attributes: Grundattribute (docs/Attribute_und_Regeneration.md §§1–3).
-- Spalten strength/agility/intelligence/constitution existieren bereits (001)
-- mit implizitem DEFAULT 10. Wisdom/Luck/Endurance werden konsistent damit
-- neutral auf 10 gesetzt; eine konkrete Start-/Rassenverteilung ist NICHT
-- beschlossen und wird später mit dem Rassen-/Charaktersystem (§2) gesetzt.
-- MariaDB: ADD COLUMN IF NOT EXISTS bleibt idempotent (wie 008/010).
ALTER TABLE characters ADD COLUMN IF NOT EXISTS wisdom INT NOT NULL DEFAULT 10;
ALTER TABLE characters ADD COLUMN IF NOT EXISTS luck INT NOT NULL DEFAULT 10;
ALTER TABLE characters ADD COLUMN IF NOT EXISTS endurance INT NOT NULL DEFAULT 10;
