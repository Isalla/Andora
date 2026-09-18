-- 019_idia.sql — realm_state: Player Persistence Stufe B (docs/Player_Persistenz.md).
--
-- 1) Währung: Die bisherige Spielerwährung heißt verbindlich `idia`
--    (docs/Player_Persistenz.md §23). Die bestehende Spalte `gold`
--    wird datenbewahrend nach `idia` umbenannt — Loot-Drops vom
--    Content-Typ 'gold' (017_loot_v1.sql) erhöhen weiterhin diese
--    Spielerwährung; der Content-Typ bleibt 'gold'.
-- 2) Persistenz-Revision (docs §29): monoton steigende, pro Charakter
--    persistierte Revisionsnummer des letzten angewendeten Player-
--    Snapshots. Legt mit dem RAM-Zähler `persist_generation` die
--    Spool-/Recovery-Reihenfolge fest (idempotente Wiederanmeldung,
--    Superseded-/Quarantäne-Logik).

ALTER TABLE characters CHANGE COLUMN gold idia INT NOT NULL DEFAULT 0;

ALTER TABLE characters ADD COLUMN IF NOT EXISTS persist_revision BIGINT NOT NULL DEFAULT 0;