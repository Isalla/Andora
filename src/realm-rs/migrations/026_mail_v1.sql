-- 026_mail_v1.sql — realm_state: Andora Mail-System V1 (additive Erweiterung).
--
-- docs/Datenbank_Architektur.md §19: Mail-Tabellen → realm_state_<realm>.
-- docs/Coordinator.md §23: Rückerstattung über das Mail-System (Realm-side).
-- docs/Datenbank_Architektur.md §21: Migrationen sind ausschließlich additiv;
-- kein DROP, kein -- destructive:, kein ALLOW_DESTRUCTIVE_MIGRATIONS.
--
-- Die alte 005_mail.sql `mail` (character-Übergangsstand, INT id, to_id/from_id)
-- bleibt unangetastet und wird von keinem Code mehr verwendet. Der Name `mail`
-- ist dadurch für neue Tabellen blockiert; deshalb kollisionsfreie Namen.
--
-- Collation: die VARCHAR-Spaltecollation angleichedem DB-Default (utf8mb4_german2_ci),
-- dieselben item_instances und characters use. sqlx 0.8 MySQL maps a BINARY-collation
-- VARCHAR to VARBINARY, which the Realm's String-decoding (mail.rs/health.rs/db.rs)
-- cannot read; a non-binary collation keeps them VARCHAR. FK-compatibility requires
-- the referencing and referenced columns to share this collation (see fk below).

-- ── character_mail ────────────────────────────────────────────────────────
-- Eine systemerzeugte Rückerstattungs-Mail an einen Charakter. Items werden
-- nicht als Blob gespeichert, sondern über character_mail_items auf die
-- bestehenden item_instances-UUIDs referiert (Escrow: Item bleibt im Besitz
-- des Systems bis zum Claim).
CREATE TABLE IF NOT EXISTS character_mail (
  mail_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  character_id INT NOT NULL,
  kind VARCHAR(16) NOT NULL DEFAULT 'system_refund',
  subject VARCHAR(120) NOT NULL DEFAULT '',
  body MEDIUMTEXT NOT NULL DEFAULT '',
  created_at_ms BIGINT NOT NULL,
  read_at_ms BIGINT NULL,
  state VARCHAR(16) NOT NULL DEFAULT 'unread',
  source_job_id VARCHAR(64) NULL,
  refund_key VARCHAR(128) NULL,
  PRIMARY KEY (mail_id),
  UNIQUE (refund_key),
  KEY idx_mail_char (character_id),
  KEY idx_mail_state (state),
  CONSTRAINT fk_mail_char FOREIGN KEY (character_id) REFERENCES characters(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_german2_ci;

-- ── character_mail_items ──────────────────────────────────────────────────
-- Escrow-Referenz: ein Item-Instanz (item_instances.item_uuid) liegt in einer
-- Mail. Die Instanz selbst bleibt in item_instances und wird durch den
-- Lifecycle nur gelöscht, wenn keine Referenz mehr besteht (daher muss die
-- Prüfung in db.rs::item_uuid_is_referenced diese Tabelle mitführen).
CREATE TABLE IF NOT EXISTS character_mail_items (
  mail_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  item_uuid VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  position INT NOT NULL DEFAULT 0,
  PRIMARY KEY (mail_id, item_uuid),
  UNIQUE (item_uuid),
  KEY idx_mail_items_mail (mail_id),
  CONSTRAINT fk_mail_items_mail FOREIGN KEY (mail_id) REFERENCES character_mail(mail_id) ON DELETE CASCADE,
  CONSTRAINT fk_mail_items_item FOREIGN KEY (item_uuid) REFERENCES item_instances(item_uuid) ON DELETE RESTRICT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_german2_ci;

-- ── character_mail_claims ─────────────────────────────────────────────────
-- Receipt für einen durchgeführten Mail-Claim (analog character_quest_commits).
-- Die Vollständigkeit des Claims (Items + Idia + Zielrevision) wird im selben
-- Commit wie der Snapshot und die Escrow-Löschung festgehalten. Eine
-- Wiederholung mit identischem Inhalt ergibt AlreadyApplied, abweichender
-- Inhalt oder unzulässige Revision Konflikt.
-- NO FK to character_mail: a Claim-Receipt must survive the Escrow-Lösching of
-- its Mail (fk_claims_mail would CASCADE-delete the receipt and lose the payout
-- proof). UNIQUE(character_id, mail_id) keeps the pair unique; fk_claims_char
-- still binds the receipt to its owning character.
CREATE TABLE IF NOT EXISTS character_mail_claims (
  commit_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  character_id INT NOT NULL,
  mail_id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  artifact_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_german2_ci NOT NULL,
  created_at_ms BIGINT NOT NULL,
  PRIMARY KEY (commit_id),
  UNIQUE (character_id, mail_id),
  KEY idx_claims_char (character_id),
  CONSTRAINT fk_claims_char FOREIGN KEY (character_id) REFERENCES characters(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_german2_ci;
