-- 002_auctions.sql — realm_state: Auktionen mit Realm-Bezug (aus altschema.sql uebernommen)
USE realm_state;

CREATE TABLE IF NOT EXISTS auctions (
  id INT AUTO_INCREMENT PRIMARY KEY,
  seller_id INT NOT NULL,
  item_id VARCHAR(48) NOT NULL,
  cnt INT NOT NULL DEFAULT 1,
  buyout INT NULL,
  bid INT NOT NULL DEFAULT 0,
  high_bidder INT NULL,
  state TINYINT NOT NULL DEFAULT 0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  expires_at TIMESTAMP NOT NULL,
  KEY idx_auctions_seller (seller_id)
);
CREATE INDEX idx_auction_state ON auctions(state, expires_at);
CREATE INDEX idx_auction_item ON auctions(item_id, state);
