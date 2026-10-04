-- 020_cooldown_persistence.sql — realm_state: Spieler-Ability-Cooldowns
-- (docs/Player_Persistenz.md §23 „Cooldowns im Player-Snapshot", P-18).
--
-- 1) Zweck: Laufende Spieler-Ability-Cooldowns überstehen Logout, Disconnect
--    und Realm-Neustart. Gespeichert wird je Fähigkeit der absolute
--    Ablaufzeitpunkt; das erlaubt, dass Offline-Zeit normal auf den Ablauf
--    angerechnet wird (kein Einfrieren beim Logout).
-- 2) Zeiteinheit: `ready_at_ms` = Millisekunden seit dem Unix-Epoch
--    (dieselbe Einheit wie `PersistSnapshot.captured_at_ms`). Die bestehende
--    Server-Uhr (Wall-Clock) bleibt die Zeitbasis; ein Uhrsprung kann die
--    verbleibende Dauer verändern.
-- 3) Vollständiges Ersetzen: Der Drain löscht die Sätze des Charakters und
--    schreibt die Map des Snapshots neu. Ein Snapshot mit bewusst leerer Map
--    entfernt damit zuvor gespeicherte Cooldowns. Snapshots im Altformat ohne
--    Cooldown-Feld lassen den Bestand unberührt (kein Ersetzen).
-- 4) Diese Tabelle gehört zum normalen Player-Snapshot (Komponente
--    `Progression`) und wird in derselben Transaktion wie der übrige
--    Snapshot geschrieben; `character_abilities` (010) ist das Vorbild für
--    eine je Charakter gehaltene, vollständig ersetzte Fähigkeitsliste.
--    NPC-Cooldowns bleiben bewusst RAM-Zustand und werden hier nicht
--    gespeichert.

CREATE TABLE IF NOT EXISTS character_cooldowns (
  char_id     INT         NOT NULL,
  ability_id  VARCHAR(64) NOT NULL,
  ready_at_ms BIGINT      NOT NULL,
  PRIMARY KEY (char_id, ability_id),
  FOREIGN KEY (char_id) REFERENCES characters(id)
);