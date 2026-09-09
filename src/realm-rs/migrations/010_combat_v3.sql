-- 010_combat_v3.sql — realm_state: Combat V3 (Ability-System).
--
-- Fähigkeitsmodell für Spieler, NPCs, Named und Bosse
-- (docs/Ability-System.md, docs/Kampfsystem.md §§8, 17–21).
--
-- Tabellen:
--   ability_definitions  — Content-Schicht: Fähigkeiten als Daten
--                          (später Lua/Content-getrieben).
--   character_abilities  — gelernte Fähigkeiten je Charakter.
--
-- Änderungen:
--   characters.mana → wird beibehalten als aktuelles Mana;
--   characters.mana_max als maximales Mana (neu).
--   monster_definitions.abilities — Fähigkeiten NPCs als Komma-Liste.
--
-- Alle Werte sind vorläufig (docs/Kampfsystem.md §§4–7).

-- ──────────────────────────────────────────────────────────────────────
-- 1) Characters: Mana-Maximum hinzufügen
-- ──────────────────────────────────────────────────────────────────────
ALTER TABLE characters ADD COLUMN IF NOT EXISTS mana_max INT NOT NULL DEFAULT 50;

-- ──────────────────────────────────────────────────────────────────────
-- 2) Monster-Definitionen: Fähigkeiten-Spalte
-- ──────────────────────────────────────────────────────────────────────
ALTER TABLE monster_definitions ADD COLUMN IF NOT EXISTS abilities VARCHAR(255) NULL;

-- ──────────────────────────────────────────────────────────────────────
-- 3) Ability-Definitionen (Content-Schicht)
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ability_definitions (
  id                VARCHAR(64)  NOT NULL PRIMARY KEY,
  name              VARCHAR(120) NOT NULL,
  -- Ausführungsart (Ability-System.md §1)
  exec_type         VARCHAR(16)  NOT NULL DEFAULT 'instant',
    -- instant | cast | channel (channel = cast-Verhalten, vorläufig §1)
  -- Semantische Kategorie (Ability-System.md §14, realm-bestätigt)
  semantic_category VARCHAR(32) NOT NULL DEFAULT 'single_target_damage',
    -- single_target_damage | aoe_damage | heal | taunt | buff | debuff | control
  -- Mana & Cooldown (Ability-System.md §2)
  mana_cost         INT          NOT NULL DEFAULT 0,
  cooldown_ms       INT          NOT NULL DEFAULT 0,
  cooldown_persistent TINYINT(1) NOT NULL DEFAULT 0,
    -- 1 = bleibt über Tod/Logout erhalten
  -- Cast-Parameter (Ability-System.md §1, §3)
  cast_time_ms      INT          NOT NULL DEFAULT 0,
  range             DOUBLE       NOT NULL DEFAULT 10.0,
  -- AoE-Typ (Ability-System.md §4)
  aoe_type          VARCHAR(16)  NOT NULL DEFAULT 'single',
    -- single | target_radius | caster_radius | ground
  aoe_radius        DOUBLE       NOT NULL DEFAULT 0.0,
  -- Freundlichkeit (Ability-System.md §5): 1=feindlich, 0=freundlich
  host_effect       TINYINT(1)   NOT NULL DEFAULT 1,
  -- Effekt (Ability-System.md §6–7)
  effect_kind       VARCHAR(16)  NOT NULL DEFAULT 'damage',
    -- damage | heal | buff | debuff | stun | silence | root | slow
  effect_value      DOUBLE       NOT NULL DEFAULT 0.0,
  duration_ms       INT          NOT NULL DEFAULT 0,
    -- 0 = kein Zeit-/Tick-Effekt
  tick_ms           INT          NOT NULL DEFAULT 0,
    -- 0 = kein Tick (Sofortwirkung); >0 = DoT/HoT-Intervall
  effect_group      VARCHAR(64)  NULL
    -- NULL = kein stapelbarer Effekt (reiner Direktschaden)
);

-- ──────────────────────────────────────────────────────────────────────
-- 4) Gelernte Fähigkeiten je Charakter
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS character_abilities (
  char_id           INT          NOT NULL,
  ability_id        VARCHAR(64)  NOT NULL,
  learned_at        TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (char_id, ability_id),
  FOREIGN KEY (ability_id) REFERENCES ability_definitions(id)
);

-- ──────────────────────────────────────────────────────────────────────
-- SEED: Probe-Fähigkeiten (vertikaler Schnitt).
-- Werte sind PROVISORISCH und dienen ausschließlich als Testinhalte.
-- Kein End-Balancing, keine endgültigen Fähigkeiten
-- (docs/Kampfsystem.md §§4–7, Exp1-Ausschlussliste).
-- ──────────────────────────────────────────────────────────────────────
INSERT INTO ability_definitions
  (id, name, exec_type, semantic_category, mana_cost, cooldown_ms, cooldown_persistent, cast_time_ms, range, aoe_type, aoe_radius, host_effect, effect_kind, effect_value, duration_ms, tick_ms, effect_group)
VALUES
  -- Direkt-Schaden (Instant,_single_target)
  ('fire_bolt',       'Feuerblitz',        'instant', 'single_target_damage',
   8, 2000, 0, 0, 12.0, 'single', 0.0, 1, 'damage', 35.0, 0, 0, NULL),
  -- Heilung (Instant,single_target,freundlich)
  ('healing_light',   'Heilendes Licht',   'instant', 'heal',
   12, 3000, 0, 0, 15.0, 'single', 0.0, 0, 'heal', 40.0, 0, 0, NULL),
  -- DoT-Debuff (Instant,single_target,effekt-gruppe)
  ('soul_rend',       'Seelenriss',        'instant', 'debuff',
   10, 4000, 0, 0, 12.0, 'single', 0.0, 1, 'debuff', 8.0, 9000, 3000, 'doom'),
  -- AoE-Schaden (Cast,target_radius)
  ('frost_nova',      'Frost Nova',        'cast',    'aoe_damage',
   20, 8000, 0, 2000, 10.0, 'target_radius', 5.0, 1, 'damage', 50.0, 0, 0, NULL),
  -- Stun-Control (Instant,single_target, 3s)
  ('choke',           'Würgen',            'instant', 'control',
   15, 12000, 0, 0, 8.0, 'single', 0.0, 1, 'stun', 0.0, 3000, 0, 'crowd'),
  -- Buff (Instant,caster_radius,freundlich, 10m Radius)
  ('battle_shout',    'Kampfschrei',       'instant', 'buff',
   5, 6000, 0, 0, 0.0, 'caster_radius', 10.0, 0, 'buff', 5.0, 10000, 0, 'shout');
