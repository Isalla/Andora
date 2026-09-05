-- 003_skills.sql — realm_state: Skills (aus dem character-Übergangsstand übernommen).

CREATE TABLE IF NOT EXISTS skills (
  char_id INT NOT NULL,
  skill_id VARCHAR(48) NOT NULL,
  lvl INT NOT NULL DEFAULT 1,
  PRIMARY KEY (char_id, skill_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);
