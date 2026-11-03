-- 030_race_definitions.sql — realm_state: Rassetabelle mit rassischer
-- Startwertmatrix (docs/Charaktererstellung_und_Charakterdarstellung.md,
-- Abschnitt „Speicherort der Rassen- und Startattributdaten"; Zahlen:
-- docs/Attribute_und_Regeneration.md §2, Basiswert 1, Mensch durchgehend 2).
--
-- 1) Zweck: eine Zeile je Rasse mit den sieben Startattributwerten. Die
--    Tabelle ist reine Daten-/Content-Schicht — kein Bezug zu class.rs,
--    regen.rs oder attributes.rs; die Matrix verändert weder Klassenwerte
--    noch Regenerationswerte noch bestehende Attributswirkungen.
-- 2) `race` trägt ausschließlich den kanonischen Wert: kleingeschrieben,
--    ohne Umlaute (mensch, elf, andorer, luzilla, mandalonier). PRIMARY KEY
--    macht die Zeile je Rasse eindeutig; der Realm-Loader prüft trotzdem
--    erneut (fail-closed, auch gegen einen bereits bestehenden Datenbestand).
-- 3) Attributspalten: IDENTISCH zu den Zeichen-/Schreibweisen der
--    characters-Attributspalten (Migration 001/011: strength, dexterity,
--    intelligence, constitution, wisdom, luck, endurance; `dexterity` ist der
--    name-seitige Wert der Geschicklichkeit wie in attributes.rs — die
--    characters-Tabelle führt dafür historisch den Spaltennamen `agility`).
--    Typ INT NOT NULL wie dort (i32-kompatibel). Bewusst KEINE DEFAULT-Werte:
--    ein fehlender Wert ist ein Datenfehler und muss sichtbar werden, nicht
--    still auf einen Default fallen.
-- 4) Seed: alle FÜNF Rassen vollständig, Werte wörtlich aus
--    Attribute_und_Regeneration.md §2 (nichts verändert, keine eigene
--    Balance). luzilla und Mandalonier sind contentseitig hinterlegt, aber
--    über die realmseitige Konfiguration für den Start deaktiviert
--    (Expansion 1 beziehungsweise 2).
-- 5) CREATE TABLE bewusst OHNE IF NOT EXISTS, Seed per normalem INSERT
--    (Muster Migration 028): ein Konflikt MUSS als Fehler sichtbar werden —
--    stillschweigendes Überspringen wäre ein verdeckter Datenbestand.
-- 6) Kollation: keine Spalte mit fester Kollation — die DB-Default-Kollation
--    gilt (Hintergrund wie 028: sqlx 0.8 liest VARCHAR mit utf8mb4_bin als
--    VARBINARY und scheitert am String-Decode).

CREATE TABLE race_definitions (
  race         VARCHAR(16) NOT NULL PRIMARY KEY,
  strength     INT NOT NULL,
  dexterity    INT NOT NULL,
  intelligence INT NOT NULL,
  constitution INT NOT NULL,
  wisdom       INT NOT NULL,
  luck         INT NOT NULL,
  endurance    INT NOT NULL
);

-- Spaltenreihenfolge: race, strength, dexterity, intelligence, constitution,
-- wisdom, luck, endurance. Werte je Rasse exakt §2-Matrix (Summen: 14/12/12/12/12).
INSERT INTO race_definitions
  (race, strength, dexterity, intelligence, constitution, wisdom, luck, endurance) VALUES
  ('mensch',      2, 2, 2, 2, 2, 2, 2),
  ('elf',         1, 4, 1, 1, 1, 3, 1),
  ('andorer',     1, 1, 4, 1, 3, 1, 1),
  ('luzilla',     1, 4, 1, 1, 1, 1, 3),
  ('mandalonier', 4, 1, 1, 3, 1, 1, 1);
