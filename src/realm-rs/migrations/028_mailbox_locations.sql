-- 028_mailbox_locations.sql — realm_state: Briefkasten-Positionen (M2 Mailbox
-- Position Delivery).
--
-- 1) Zweck: Briefkästen sind zum jetzigen Stand reine Positionsangaben. Kein
--    Eintrag in monster_definitions/monster_spawns, keine NPC-/Welt-Entität,
--    kein Bezug zu einer anderen Tabelle und kein Fremdschlüssel. Die
--    Positionen werden einmalig beim Charaktereinstieg über das bestehende
--    WELCOME (additives Feld `mailboxes`) an den Client übertragen; Reichweite,
--    Bestand und Berechtigung prüft niemand (Mail-Inhalte laufen weiterhin
--    ausschließlich über die Mail-API, Eintrag 18g).
-- 2) Seed: genau die Positionen aus DECO_MAILBOXES im Testclient
--    (testclient/tc_world.gd) — Koordinaten wörtlich übernommen, nichts
--    ergänzt, weggelassen oder gerundet. Der Seed ist identisch zur lokalen
--    Deko, damit der Umstieg auf Serverdaten bildgenau bleibt.
-- 3) CREATE TABLE bewusst OHNE IF NOT EXISTS, Seed per normalem INSERT:
--    ein Konflikt (Tabelle oder Seedzeilen vorhanden) MUSS als Fehler
--    sichtbar werden — stillschweigendes Überspringen wäre ein verdeckter
--    Datenbestand.
-- 4) Kollation: keine Spalte mit fester Kollation — die DB-Default-Kollation
--    gilt. Hintergrund (Eintrag 18g): sqlx 0.8 liest VARCHAR mit utf8mb4_bin
--    als VARBINARY und scheitert dann am String-Decode; Default-Kollationen
--    (utf8mb4_german2_ci) werden korrekt als String gelesen.

CREATE TABLE mailbox_locations (
  id    VARCHAR(64) NOT NULL PRIMARY KEY,
  pos_x DOUBLE      NOT NULL,
  pos_y DOUBLE      NOT NULL
);

INSERT INTO mailbox_locations (id, pos_x, pos_y) VALUES
  ('mailbox-platz', 1.6, -3.4),
  ('mailbox-west-1', -6.2, -2.6),
  ('mailbox-west-2', -6.2, 7.6),
  ('mailbox-ost', 20.2, 6.0);
