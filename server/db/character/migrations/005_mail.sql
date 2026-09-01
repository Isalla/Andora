-- 005_mail.sql — character: persoenliche Post (aus altschema.sql uebernommen)
USE character;

CREATE TABLE IF NOT EXISTS mail (
  id INT AUTO_INCREMENT PRIMARY KEY,
  to_id INT NOT NULL,
  from_id INT NULL,
  subject VARCHAR(120) NOT NULL,
  body JSON NULL,
  read_at TIMESTAMP NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (to_id) REFERENCES characters(id) ON DELETE CASCADE,
  FOREIGN KEY (from_id) REFERENCES characters(id) ON DELETE SET NULL
);
