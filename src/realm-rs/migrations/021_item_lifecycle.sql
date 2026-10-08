-- 021_item_lifecycle.sql — realm_state: Item-Instanz-Lifecycle-Metadaten
-- (docs/inventory_system.md §16–§18, Single-Process-Realm).
--
-- 1) Zweck: Abgekoppelte Item-UUIDs (verkauft, bei Vollverschmelzung
--    aufgegeben via `retired_uuid`, verworfene Puffer-Items) werden NICHT
--    sofort aus `item_instances` gelöscht. Stattdessen hält diese Tabelle die
--    Zuordnung (welche UUID wurde in welcher Snapshot-Revision von welchem
--    Prozesslauf abgekoppelt), und der Drain finalisiert zulässige Instanzen
--    kontrolliert — in derselben Transaktion wie Inventar, Idia und
--    `persist_revision` (docs/Player_Persistenz.md §30).
-- 2) Vollständiges Ersetzen: Der Drain löscht die Sätze des Charakters und
--    schreibt die Sicht des Snapshots neu. UUIDs, die im Snapshot wieder
--    platziert sind (widersprüchliche Zuordnung), entfallen ersatzlos und
--    werden nie gelöscht. Snapshots im Altformat ohne Lifecycle-Feld lassen
--    den Bestand unberührt (kein Ersetzen, keine Löschung).
-- 3) Referenzschutz: Referenzierte UUIDs (Platzierungs-/Pufferzeilen) werden
--    nie gelöscht; ihre Metadaten bleiben zur erneuten Prüfung erhalten.
--    Die Startup-Finalisierung verarbeitet ausschließlich diese markierten
--    Zeilen — kein Voll-Scan über `item_instances` (docs §16).
-- 4) Bewusst KEIN Fremdschlüssel auf `item_instances(item_uuid)`:
--    Unbestätigte Entfernungen werden in neueren Snapshots mitgeführt, auch
--    wenn die Instanzzeile bereits finalisiert wurde; ein FK würde dieses
--    Mitführen verhindern. Der Charakterbezug bleibt per FK mit
--    ON DELETE CASCADE erhalten (Charakterlöschung räumt Metadaten ab).
-- 5) Die Sell-/Buyback-History ist KEIN Teil dieser Tabelle: Sie bleibt
--    ausschließlich nicht-persistenter Runtime-State
--    (docs/Handelssystem.md §2/§10).

CREATE TABLE IF NOT EXISTS item_instance_finalizations (
  char_id              INT          NOT NULL,
  item_uuid            VARCHAR(64)  NOT NULL,
  -- Snapshot-Revision, die diese Abkopplung trägt (Zuordnung).
  detached_at_revision BIGINT       NOT NULL,
  -- Abkopplungsgrund: sold | merged | discarded (Zuordnungshilfe).
  reason               VARCHAR(16)  NOT NULL,
  -- Eigene Runtime-Kennung des abkoppelnden Prozesslaufs (Zuordnung).
  runtime_id           VARCHAR(64)  NOT NULL,
  -- Erfassungszeitpunkt (Millisekunden seit dem Unix-Epoch).
  recorded_at_ms       BIGINT       NOT NULL,
  PRIMARY KEY (char_id, item_uuid),
  KEY idx_finalizations_uuid (item_uuid),
  CONSTRAINT fk_finalizations_char FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);
