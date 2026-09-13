-- 018_progression.sql — realm_state: Progressionssystem V1 (docs/
-- Erfahrung_und_Progressionssystem.md §§4–7, 12).
--
-- Ergänzt die Feldgruppe der Charakter-Progression in characters:
--   free_attr_points — freie Attributpunkte (§5): bei Levelaufstieg
--                      gutgeschrieben (L1=0, normal 5, zehntes Level 10).
--   rested_pool      — Rested-EXP-Pool (§12): max. 50 % der aktuellen
--                      Level-Anforderung, offline 10 % pro 24 h; wird nur
--                      durch Kill-EXP abgerufen.
--   logout_at        — Wallclock-Logout-Zeitpunkt (Epoch-Sekunden) des
--                      letzten Ausloggens (§12); NULL = nie ausgeloggt bzw.
--                      nach dem Login-Berechnen zurückgesetzt. Basis für
--                      die einmalige Rested-Berechnung beim Login.
--
-- Zusätzlich Content-Ebene: Gegnerlevel für die Leveldifferenz (§7):
--   monster_definitions.level — Level des Monsters; bestimmt gemeinsam mit
--                      den Charakterleveln den EXP-Multiplikator beim Kill.
--                      DEFAULT 1 ist nur ein technischer Fallbackwert; die
--                      tatsächlichen Monsterlevel werden separat als
--                      Gameplay-/Content-Balancing definiert.

ALTER TABLE characters ADD COLUMN IF NOT EXISTS free_attr_points INT NOT NULL DEFAULT 0;

ALTER TABLE characters ADD COLUMN IF NOT EXISTS rested_pool INT NOT NULL DEFAULT 0;

ALTER TABLE characters ADD COLUMN IF NOT EXISTS logout_at BIGINT NULL;

ALTER TABLE monster_definitions ADD COLUMN IF NOT EXISTS level INT NOT NULL DEFAULT 1;