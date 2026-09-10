-- 013_group_system.sql — realm_state: Gruppensystem V1 (§7 Monster-EXP).
--
-- Ergänzt Monster-EXP auf Content-Ebene (docs/Gruppensystem.md §7):
--   exp_reward  — normale Monster-EXP in Punkten. Beim Kill wird das
--                 gesamt (100 %) zwischen den aktiven Gruppenmitgliedern
--                 bzw. dem Claim-Spieler aufgeteilt (siehe combat_tick /
--                 award_monster_exp). 0 = keine EXP.
--
-- Werte sind PROVISORISCH (vertikaler Schnitt, kein End-Balancing), wie
-- der Content-Seed in 009_combat_v2.sql.

ALTER TABLE monster_definitions ADD COLUMN IF NOT EXISTS exp_reward INT NOT NULL DEFAULT 0;

-- Provisorische Seed-Werte für die 009-er Probe-Inhalte.
UPDATE monster_definitions SET exp_reward = 25 WHERE id = 'wolf';
UPDATE monster_definitions SET exp_reward = 20 WHERE id = 'boar';
UPDATE monster_definitions SET exp_reward = 90 WHERE id = 'wolfpack_named';
UPDATE monster_definitions SET exp_reward = 400 WHERE id = 'wurmlohr_boss';