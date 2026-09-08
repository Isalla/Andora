-- 009_combat_v2.sql — realm_state: Combat V2 (NPC/Monster-Ebene).
--
-- Gemeinsamer, realm-autoritativer Kampfkern für Spieler UND NPC/Monster
-- (docs/Kampfsystem.md §§18–21, docs/Boss-System.md §§2–6).
--
-- Tabellen:
--   monster_definitions  — statische Content-Schicht (§§18,21: Werte aus
--                           Content/Lua oder Spawn-/DB-Daten).
--   monster_spawns       — Spawn-Platzierung + Home-Zone (§20).
--   monster_instances    — persistenter Runtime-Zustand inkl. Respawn-
--                           Timern über Realm-Neustarts hinweg (§21,
--                           Boss-System.md §6; Datenbank_Architektur.md §5).
--
-- Alle Werte sind vorläufig und werden anhand realer Praxistests angepasst
-- (docs/Kampfsystem.md §§4–7). Keine Architekturwerte.

-- ──────────────────────────────────────────────────────────────────────
-- 1) Definitions (Content-Schicht)
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS monster_definitions (
  id              VARCHAR(64)  NOT NULL PRIMARY KEY,
  name            VARCHAR(120) NOT NULL,
  -- Grundregel: attackable und aggressive sind getrennte Eigenschaften (§18).
  -- kind steuert Kategorie + Default-Respawn (§21).
  kind            VARCHAR(16)  NOT NULL DEFAULT 'normal',  -- normal|named|boss
  attackable      TINYINT(1)   NOT NULL DEFAULT 1,
  aggressive      TINYINT(1)   NOT NULL DEFAULT 1,
  aggro_range     DOUBLE       NOT NULL DEFAULT 8.0,
  attack_range    DOUBLE       NOT NULL DEFAULT 1.5,
  attack_duration_ms INT       NOT NULL DEFAULT 1500,
  weapon_damage   INT          NOT NULL DEFAULT 10,
  weapon_skill    INT          NOT NULL DEFAULT 1,
  armor           INT          NOT NULL DEFAULT 0,
  max_hp          INT          NOT NULL DEFAULT 100,
  move_speed      DOUBLE       NOT NULL DEFAULT 4.0,
  -- Respawn: Content-Wert (§21). NULL → Default pro kind.
  -- Defaults: normal=300000 (5min), named=600000 (10min), boss=3600000 (60min).
  respawn_ms      INT          NULL,
  -- Aggro-Formen (§21): faction = soziale Aggro-Gruppe; pack_id wird auf
  -- Spawn-Ebene gesetzt (feste Gruppe/Rudel).
  faction         VARCHAR(64)  NULL
);

-- ──────────────────────────────────────────────────────────────────────
-- 2) Spawn-Platzierung (Content-Schicht: Home-Zone, Leash, §20)
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS monster_spawns (
  id              INT          NOT NULL PRIMARY KEY AUTO_INCREMENT,
  monster_id      VARCHAR(64)  NOT NULL,
  zone_id         INT          NOT NULL DEFAULT 0,
  home_x          DOUBLE       NOT NULL DEFAULT 0.0,
  home_y          DOUBLE       NOT NULL DEFAULT 0.0,
  home_radius     DOUBLE       NOT NULL DEFAULT 5.0,
  leash_radius    DOUBLE       NOT NULL DEFAULT 15.0,
  -- Per-Spawn-Overrides (§18/§21): NULL = Definition-Wert.
  attackable      TINYINT(1)   NULL,
  aggressive      TINYINT(1)   NULL,
  respawn_ms      INT          NULL,
  -- Pack-ID: Instanzen desselben pack_id agieren als feste Einheit (§21).
  pack_id         VARCHAR(64)  NULL,
  FOREIGN KEY (monster_id) REFERENCES monster_definitions(id)
);

-- ──────────────────────────────────────────────────────────────────────
-- 3) Persistenter Runtime-Zustand (§5/§21, Boss-System.md §6)
-- ──────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS monster_instances (
  spawn_id        INT          NOT NULL PRIMARY KEY,
  -- Laufzustand
  status          VARCHAR(16)  NOT NULL DEFAULT 'alive',
    -- alive | dead | returning
  hp              INT          NOT NULL DEFAULT 0,
  x               DOUBLE       NOT NULL DEFAULT 0.0,
  y               DOUBLE       NOT NULL DEFAULT 0.0,
  -- Evade/Return (§20): Zeitstempel für Rücklaufzeit-Berechnung wird im
  -- laufenden Realm-Betrieb gehalten (Instant, nicht persistiert).
  return_started_at_ms BIGINT  NULL,
  -- Respawn (§21): absolute Epoch-Zeit in Millis; über Realm-Neustarts.
  respawn_after_ms BIGINT      NULL,
  -- Boss-Claim (Boss-System.md §§2–3): VARCHAR für spätere Erweiterung auf
  -- Gruppen-ID (Gruppensystem existiert noch nicht).
  claimed_by      VARCHAR(64)  NULL,
  claim_at_ms     BIGINT       NULL,
  -- Zeitstempel
  last_persist_at TIMESTAMP    NULL DEFAULT CURRENT_TIMESTAMP
);

-- ──────────────────────────────────────────────────────────────────────
-- SEED: Probe-Inhalte (zone 0). Werte sind PROVISORISCH und dienen
-- ausschließlich als vertikaler Schnitt zum Testen der Core-Logik.
-- Kein End-Balancing, keine Exp1-Encounter, keine endgültigen Werte
-- (docs/Kampfsystem.md §§4–7, Exp1-Ausschlussliste).
-- ──────────────────────────────────────────────────────────────────────
INSERT INTO monster_definitions (id, name, kind, attackable, aggressive, aggro_range, attack_range, attack_duration_ms, weapon_damage, weapon_skill, armor, max_hp, move_speed, respawn_ms, faction) VALUES
('wolf',             'Wolf',             'normal', 1, 1,  8.0, 1.5, 1500, 15, 1, 0, 120, 4.0,  NULL,  'wolfpack'),
('boar',             'Keiler',           'normal', 1, 0, 10.0, 1.5, 1500, 12, 1, 0, 100, 3.5,  NULL,  NULL),
('wolfpack_named',   'Alphawolf',        'named',  1, 1, 10.0, 1.5, 1500, 25, 2, 5, 400, 4.5,  NULL,  'wolfpack'),
('wurmlohr_boss',    'Wurmlord',         'boss',   1, 1, 12.0, 2.0, 2000, 30, 2, 10, 1500, 3.0, 600000, 'wurm');

-- Zone 0 Spawns (Home-Zone + Leash)
INSERT INTO monster_spawns (monster_id, zone_id, home_x, home_y, home_radius, leash_radius, pack_id) VALUES
('wolf',           0, 10.0, 10.0, 5.0, 15.0, 'pack1'),
('wolf',           0, 12.0, 12.0, 5.0, 15.0, 'pack1'),
('boar',           0, 30.0, 30.0, 5.0, 12.0, NULL),
('wolfpack_named', 0, 50.0, 20.0, 5.0, 15.0, NULL),
('wurmlohr_boss',  0, 80.0, 80.0, 5.0, 18.0, NULL);
