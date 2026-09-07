-- 008_combat_v1.sql — realm_state: Combat V1.
-- Rüstungswert am Charakter (relevante physische Rüstung für die
-- Schadensreduktion laut docs/Kampfsystem.md §7). Die Werte werden
-- später durch das Ausrüstungssystem gespeist; der Mechanismus
-- (Reduktion + Klassen-Caps) ist über CombatCfg testbar und
-- konfigurierbar. MariaDB: ADD COLUMN IF NOT EXISTS bleibt idempotent.
ALTER TABLE characters ADD COLUMN IF NOT EXISTS combat_armor INT NOT NULL DEFAULT 0;