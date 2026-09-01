-- 004_quests.sql — character: persoenliche Questfortschritte (aus altschema.sql uebernommen)
USE character;

CREATE TABLE IF NOT EXISTS quests (
  char_id INT NOT NULL,
  quest_id VARCHAR(48) NOT NULL,
  state TINYINT NOT NULL DEFAULT 0,
  data JSON NULL,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (char_id, quest_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);
