-- 022_merchant_trading.sql — realm_state: NPC-Händler (docs/Handelssystem.md).
--
-- 1) Zweck: Händlerrolle je NPC-Spawn plus Verkaufssortiment mit expliziten
--    serverseitigen Kauf-/Verkaufspreisen (Idia, absolute Beträge).
--    `buy_price_idia` = Preis für den Spielerkauf; NULL = nicht im Angebot.
--    `sell_price_idia` = Ankaufpreis bei Spieler-Verkauf; NULL = der Händler
--    nimmt dieses Item nicht an. Unbegrenzter Angebotsbestand (keine Mengen-,
--    Quoten- oder Gebührenspalten).
-- 2) Buyback braucht keine Angebotszeile: Er ist spielergebunden und
--    händlerübergreifend (reiner Runtime-State, kein DB-Bestand).
-- 3) Keine Seed-Preise in dieser Migration (keine erfundenen
--    Produktionspreise); Sortimente pflegt der Content-Betrieb je RealmDB.
--    Zeilen mit unbekannter Item-ID oder ohne gültigen Preis werden vom
--    Loader übersprungen (fail-closed, `db::load_merchant_catalog`).
-- 4) Referenzen: Händlerrolle hängen an `monster_spawns(id)` (ON DELETE
--    CASCADE räumt Rolle und Angebote eines entfernten Spawns ab);
--    Angebots-Items hängen an `item_definitions(id)`.

CREATE TABLE IF NOT EXISTS npc_merchants (
  spawn_id INT NOT NULL PRIMARY KEY,
  CONSTRAINT fk_merchant_spawn FOREIGN KEY (spawn_id) REFERENCES monster_spawns(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS merchant_offers (
  spawn_id INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  buy_price_idia BIGINT NULL,
  sell_price_idia BIGINT NULL,
  PRIMARY KEY (spawn_id, item_id),
  KEY idx_offer_item (item_id),
  CONSTRAINT fk_offer_merchant FOREIGN KEY (spawn_id) REFERENCES npc_merchants(spawn_id) ON DELETE CASCADE,
  CONSTRAINT fk_offer_item FOREIGN KEY (item_id) REFERENCES item_definitions(id)
);
