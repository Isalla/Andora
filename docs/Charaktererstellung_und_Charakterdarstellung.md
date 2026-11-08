# Charaktererstellung und clientseitige Charakterdarstellung

## Grundprinzip

Die spielmechanische Identität eines Charakters und seine grafische Darstellung werden in Andora strikt voneinander getrennt.

> **Der Server bestimmt, was ein Charakter ist. Der Client bestimmt, wie dieser Charakter dargestellt wird.**

Der Server speichert ausschließlich abstrakte Charakter- und Appearance-Werte. Grafiken, Character-Sets, alternative Darstellungen und kosmetische Mods sind ausschließlich Aufgabe des Clients.

---

## Regelbereich Charaktererstellung

Dieser Regelbereich führt die Charakterstruktur an einer Stelle zusammen:
VERBINDLICH-Liste, Angebot des Erstellungsablaufs, Spielbare Rassen,
Vorname, Startklasse und Startwerte, Umgang mit Platzhaltercharakteren,
Speicherort der Rassen- und Startattributdaten sowie die Beschlüsse dieser
Runde (Feld-zu-Quelle-Tabelle, `idia`, Startdatenstruktur, Regel 8b).
Jeder Fakt steht genau einmal; andere Stellen verweisen. Die fachlichen
Entscheidungen a–j der Anlage stehen im Abschnitt „Festlegungen zur
Charakteranlage (Entscheidungen a–j)" — dieser Regelbereich bindet sie an,
ohne sie zu duplizieren.

### Verbindliche Regeln der Charakteranlage

VERBINDLICH:

* Die Charaktererstellung ist ein **separater, authentifizierter Ablauf** und ist **nicht** Teil des Realm-Einstiegs. HELLO ist ausschließlich Login und Lookup und legt niemals einen Charakter an; der Realm lehnt einen unbekannten `char_id` fail-closed ab, statt einen Datensatz anzulegen (`Login_Realm_Architektur.md`, Abschnitt 6 und Abschnitt 16).
* Die `character_id` wird bei der Erstellung **serverseitig** vergeben und ist eine **positive Datenbank-ID** des Characters im jeweiligen Realm. Der Client liefert keine ID und darf keine erzwingen.
* Die Erstellung ist an den **authentifizierten Account** gebunden; die `account_id` wird serverseitig aus der Anmeldung übernommen und nie aus Clientangaben gesetzt.

Der Erstellungsablauf validiert mindestens:

1. **Account-Bindung** – die `account_id` stammt aus der authentifizierten Sitzung; ein Auftrag ohne authentifizierte Sitzung wird abgelehnt.
2. **Name** – Länge und zulässiger Zeichensatz werden serverseitig geprüft; der Name darf kein Ersatz für die `character_id` sein.
3. **Eindeutigkeit** – ein bereits vergebener Name wird abgelehnt, statt einen zweiten Datensatz anzulegen.
4. **Rasse und Startklasse** – die übermittelte Rasse wird gegen die gültigen aktivierten Werte geprüft und in kanonischer Form gespeichert; die Startklasse folgt den Regeln des Klassensystems. Appearance-Felder gehören nicht zum V1-Umfang der Erstellung; ihre Prüfung und Speicherung ist für einen späteren Auftrag offen.
5. **Initiale Datensätze** — die Anlage schreibt genau **einen** Charakterdatensatz: die `characters`-Zeile und ihre sieben Grundattribute (Regel 8b, Abschnitt „Beschlüsse dieser Runde"). Die zugehörigen Tabellen `inventory`, `character_abilities`, `quests`, `skills` und `character_cooldowns` erhalten bei der Anlage **keine** Startzeilen; das Fehlen der Zeile ist ihr leerer Anfangszustand (Abschnitt „Startdatenstruktur"). Ein unvollständiger Charakter entsteht dadurch nicht; Starterausrüstung wird nicht erfunden („Festlegungen", (d) und (j)).
6. **Transaktionsgrenzen** – die Anlage der Datensätze erfolgt so, dass ein Fehler keinen halb angelegten Charakter hinterlässt.

### Angebot des Erstellungsablaufs

Die Charaktererstellung wird vom **Realm-Server** über seinen
bestehenden HTTP-Port angeboten, analog zur bereits eingebauten
Charakterliste `GET /characters`:

```text
POST /characters?session_id=<Login-Session>
```

- Die Prüfung der `session_id` läuft über die vorhandene signierte
  Auth-API; Sitzung, `account_id` und `character_id` folgen der
  VERBINDLICH-Liste.
- Eine clientgelieferte `account_id` oder `character_id` ist ohne
  Wirkung und wird abgelehnt.
- Die vergebene Datenbank-ID (VERBINDLICH-Liste) stammt aus der
  Realm-Datenbank dieses Realms.
- Transaktionsgrenzen: Eintrag 6 der VERBINDLICH-Liste; die
  Regelstelle der Anlage mit der Prüfung vor der Transaktion und
  dem Index als letzter Durchsetzung ist Abschnitt (a) der
  „Festlegungen zur Charakteranlage".
- Der Realm-Einstieg bleibt unberührt: Eintrag 1 der
  VERBINDLICH-Liste (`Login_Realm_Architektur.md` Abschnitt 6).

### Spielbare Rassen bei der Erstellung

Kanonische Rassenwerte sind durchgehend **kleingeschrieben** und führen **keine Umlaute**:

| Rasse | kanonischer Wert | beim Start |
|---|---|---|
| Menschen | `mensch` | aktiv |
| Elfen | `elf` | aktiv |
| Andorer | `andorer` | aktiv |
| Luzilla | `luzilla` | deaktiviert (Expansion 1) |
| Mandalonier | `mandalonier` | deaktiviert (Expansion 2) |

Die Realm-Validierung akzeptiert nur aktivierte Rassen. Eine
deaktivierte Rasse ist als Wert vorbereitet, aber bei der
Charaktererstellung nicht wählbar.

### Vorname

Bei der Charaktererstellung wird ausschließlich der **Vorname** vergeben.

- Länge 3 bis 10 Zeichen.
- Erlaubt sind Buchstaben einschließlich deutscher Umlaute sowie
  der Bindestrich; keine Ziffern, keine sonstigen Sonderzeichen.
- Führende und abschließende Leerzeichen werden entfernt; ein
  leerer oder nur aus Leerzeichen bestehender Name wird abgelehnt.
- Die Eindeutigkeit gilt realmweit und Groß-/Kleinschreibung
  gleichgültig.
- Funktionsbezeichnungen wie `gm`, `admin` oder `support` sind
  reserviert und werden abgelehnt.
- Nachnamen sind ausdrücklich **nicht** Bestandteil dieses
  Abschnitts. Sie sind als **accountgebundener** Zusatz geplant,
  dadurch teilen alle Charaktere eines Accounts denselben
  Nachnamen, und bei der Erstellung wird weiterhin nur der
  Vorname gewählt. Direkte Ansprache im Spiel verwendet den
  Namen ohne Nachnamen. Für die Nachnamen existiert derzeit
  weder Feld noch Regel.

### Startklasse und Startwerte bei der Erstellung

- Jeder neu erstellte Charakter beginnt als **Abenteurer**; `char_class`
  erhält diesen Wert bei der Anlage als Konstante (Feld-zu-Quelle-
  Tabelle, Abschnitt „Beschlüsse dieser Runde"). Die Grundklassenwahl
  erfolgt wie im Klassensystem dokumentiert später
  (`Klassensystem.md`, `Rassen-Fraktionen.md` Abschnitt 10) und bleibt
  bei Level 9 („Festlegungen", (c)).
- Die sieben Grundattribute werden aus der rassischen Startwertmatrix
  gesetzt. Zahlquelle ist `Attribute_und_Regeneration.md` Abschnitt 2;
  RAM-Kopie und Schreibpfad sind in „Festlegungen", (b) geregelt;
  Datenbasis und Spaltennamen im Abschnitt „Speicherort der Rassen-
  und Startattributdaten".
- Startdatensätze: Eintrag 5 der VERBINDLICH-Liste und Abschnitt
  „Startdatenstruktur". Es wird keine Starterausrüstung erfunden
  („Festlegungen", (d) und (j)).

### Umgang mit vorhandenen Test- und Platzhaltercharakteren

Bestehende Charakterdatensätze werden **nicht** per Migration auf
die kanonischen Rassenwerte umgestellt und nicht umgeschrieben; sie
werden mit der Zeit durch neu angelegte Charaktere abgelöst. Die
Erstellung schreibt ausschließlich kanonische Werte („Spielbare
Rassen bei der Erstellung"). Der Realm liest das Feld `race` tolerant:
ein unbekannter oder abweichend geschriebener Wert wird protokolliert,
verhindert aber **nicht** den Einstieg.

### Speicherort der Rassen- und Startattributdaten

Die rassische Startwertmatrix liegt **serverseitig als Daten** vor und wird
vom Realm beim Start geladen. Verbindliche Zahlenquelle ist
`Attribute_und_Regeneration.md` Abschnitt 2 (Basiswert 1, `mensch` durchgehend
auf 2); dieses Dokument wiederholt keine Zahlenwerte. Die Matrix gehört nicht
in die Kampf-, Klassen- oder Regenerationslogik: die Verteilung bleibt
daten- respektive contentgetrieben (`Attribute_und_Regeneration.md`
Abschnitt 11), damit eine Balancing-Anpassung keinen Architekturumbau
erfordert.

Fest steht:

- pro Rasse genau **eine** Zeile mit den sieben Startattributwerten;
- welche Rassen bei der Charaktererstellung wählbar sind, entscheidet die
  **realmseitige Konfiguration**; nicht gesetzt oder leer bedeutet, dass
  ausschließlich `mensch`, `elf` und `andorer` wählbar sind, während
  `luzilla` und `mandalonier` nicht wählbar sind;
- im Charakterdatensatz bleibt ausschließlich der kanonische Rassenwert
  (`race`); die Startattribute werden bei der Anlage aus der Tabelle
  abgeleitet und als Werte des Charakters gespeichert.

Der Speicherort ist mit der Umsetzung festgelegt: die Tabelle
`race_definitions` wird in der Migration `030_race_definitions.sql`
angelegt und beim Realm-Start durch den Realm geladen; die realmseitige
Konfiguration verwendet den Schlüssel `SELECTABLE_RACES`. Die
Attributspalten führen die sieben Grundattribute unter denselben Namen
wie `attributes.rs`.

Die Tabelle führt Geschicklichkeit unter `dexterity`; die `characters`-Tabelle
führt dieselbe Größe unter `agility` (seit Migration 001, durch Migration 011
unverändert). Bei den übrigen sechs Grundattributen stimmen die Spaltennamen
beider Tabellen überein. Die Namenszuordnung zwischen `agility` und `dexterity`
besitzt der Realm bereits in seinem Attribut-Lese- und -Schreibpfad; welcher
Schreibpfad sie bei der Charakteranlage trägt, regelt „Festlegungen", (b).

Die realmseitige Konfiguration enthält nur kanonische Rassenwerte. Ein
unbekannter oder nicht kanonischer Wert führt zu einem Abbruch des
Realm-Starts mit klarer Fehlermeldung; er wird nicht ignoriert oder durch
einen Standardwert ersetzt. Ein leerer Eintrag gilt als nicht kanonisch.
Derselbe kanonische Wert darf nicht mehrfach aufgeführt werden; auch ein
Duplikat führt zu einem Abbruch des Realm-Starts mit klarer Fehlermeldung.
Eine nach dieser Konfiguration wählbare Rasse
muss zusätzlich in der Rassetabelle vorhanden sein; fehlt sie, greift die
bereits beschriebene Prüfung beim Start des Loaders.

Der Loader der Rassetabelle arbeitet **fail-closed**. Der Realm-Start bricht
mit klarer Meldung ab, wenn:

- die Tabelle fehlt,
- ein Attributwert fehlt, nicht ganzzahlig, kleiner als 1 oder sonst ungültig ist,
- eine Zeile einen Rassenwert enthält, der nicht zu den fünf kanonischen
  Werten gehört,
- dieselbe Rasse in mehr als einer Zeile steht,
- eine Rasse fehlt, die nach der realmseitigen Konfiguration wählbar sein soll.

Ein Überspringen fehlerhafter Zeilen — das Verfahren der Item-Definitionen —
ist für Rassedaten ausdrücklich **nicht** vorgesehen, weil die Rasse Teil der
Charakteridentität ist und ohne vollständige Matrix kein Charakter korrekt
angelegt werden kann.

Eine Rasse, die nach der Konfiguration **nicht** wählbar ist, darf in der
Tabelle fehlen, ohne den Start zu blockieren. Die Migrationsinhalte
hinterlegen alle fünf Rassen vollständig; die Toleranz betrifft nur den
Loader, damit eine vorbereitete Expansion nicht den Start eines Realms
erzwingt, das sie noch nicht anbietet.

Bestehende Rust-Klassen-/Regenerationskonstanten bleiben davon unberührt: die
Klassen- und Regenerationsgrundwerte in `class.rs` und `regen.rs` sowie die
Attributswirkungen in `attributes.rs` (`Attribute_und_Regeneration.md`
Abschnitte 6 und 11) sind von der Rassetabelle getrennt. Die Tabelle
verändert weder Klassenwerte noch Regenerationswerte noch bestehende
Attributswirkungen.

Der Implementierungsstatus dieses Abschnitts steht ausschließlich in
`Projekt-Status.md` (Eintrag 22) und wird hier nicht wiederholt.

### Beschlüsse dieser Runde (1–10): Feldquellen, `idia`, Startdatenstruktur, Zuständigkeiten

Die Beschlüsse dieser Runde sind in diesem Abschnitt festgehalten.
Eintrag 5 der VERBINDLICH-Liste ist deckungsgleich mit dieser Struktur
gefasst; die Abschnitte (a) und (b) der „Festlegungen zur
Charakteranlage" verweisen auf die Tabellen dieses Abschnitts.

#### Feld-zu-Quelle-Tabelle aller `characters`-Spalten

Abgeleitet aus den Migrationen 001 bis 030: die Tabelle `characters`
führt genau die 29 Spalten der folgenden Tabelle — Migration 001 legt
19 Spalten an (davon `gold`, seit Migration 019 als `idia` geführt),
die Migrationen 008, 010, 011, 012, 018 und 019 ergänzen die übrigen;
keine weitere Migration in diesem Bereich ändert die Spaltenmenge.

Die Quelle nennt, wer den Wert bei der Anlage bestimmt:
**Sitzung** (aus der Sessionprüfung), **geprüfte Anfrage**
(clientgeliefert, serverseitig geprüft), **Konstante** (vom
Anlegepfad vorgegeben), **Rassenmatrix** (RAM-Kopie beim Start),
**Datenbank** (AUTO_INCREMENT) oder **DB-Default** (von der Anlage
nicht geschrieben; der in der Migration definierte Standardwert gilt).

| Spalte | Quelle | Wert/Bemerkung bei der Anlage |
|---|---|---|
| `id` | Datenbank | AUTO_INCREMENT; die vergebene positive Datenbank-ID (VERBINDLICH-Liste) |
| `account_id` | Sitzung | ausschließlich aus der Sessionprüfung |
| `name` | geprüfte Anfrage | Vorname nach Abschnitt „Vorname"; `idx_char_name` als letzte Durchsetzung ((a)) |
| `race` | geprüfte Anfrage | kanonischer, wählbarer Wert („Spielbare Rassen") |
| `char_class` | Konstante | `Adventurer` (kanonische Schreibweise); Leserseite: (e) |
| `strength` | Rassenmatrix | via (b) und `db::write_attributes` |
| `agility` | Rassenmatrix | Wert aus `race_definitions.dexterity` (Zuordnung: Abschnitt „Speicherort") |
| `intelligence` | Rassenmatrix | via (b) und `db::write_attributes` |
| `constitution` | Rassenmatrix | via (b) und `db::write_attributes` |
| `wisdom` | Rassenmatrix | via (b) und `db::write_attributes` |
| `luck` | Rassenmatrix | via (b) und `db::write_attributes` |
| `endurance` | Rassenmatrix | via (b) und `db::write_attributes` |
| `level` | DB-Default | 1 |
| `exp` | DB-Default | 0 |
| `idia` | DB-Default | 0 (Beschluss: Start-`idia` ist 0, kein Startbetrag) |
| `hp` | DB-Default | 100 |
| `mana` | DB-Default | 50 |
| `mana_max` | DB-Default | 50 (Migration 010) |
| `zone_id` | DB-Default | 0 |
| `pos_x` | DB-Default | 0 |
| `pos_y` | DB-Default | 0 |
| `guild_id` | DB-Default | NULL |
| `created_at` | DB-Default | CURRENT_TIMESTAMP |
| `combat_armor` | DB-Default | 0 (Migration 008) |
| `faction_transition` | DB-Default | 0 (Migration 012) |
| `free_attr_points` | DB-Default | 0 (Migration 018) |
| `rested_pool` | DB-Default | 0 (Migration 018) |
| `logout_at` | DB-Default | NULL (Migration 018) |
| `persist_revision` | DB-Default | 0 (Migration 019) |

#### Startdatenstruktur

Die Anlage schreibt genau **eine** Zeile — in `characters` (mit dem
Attribut-Update derselben Zeile, Regel 8b). Die zugehörigen Tabellen
erhalten bei der Anlage keine Startzeilen; für sie ist das Fehlen der
Zeile das definierte Startbild:

| Tabelle (Migration) | Zeile bei der Anlage | Bedeutung des Fehlens |
|---|---|---|
| `inventory` (002) | keine | leeres Inventar; der Persistenzpfad schreibt Vollbestände später selbst |
| `character_abilities` (010) | keine | **3c — OFFEN**, siehe unten |
| `quests` (004) | keine | keine Quests; der Quest-Lader liefert die leere Liste |
| `skills` (003) | keine | kein Eintrag = Startstufe 1 (`db::load_weapon_skill`) |
| `character_cooldowns` (020) | keine | keine Cooldowns; der Lader liefert die leere Liste |

**3c — Startfähigkeiten: OFFEN.** Der Realm lädt beim Start alle Zeilen
der Tabelle `ability_definitions` (`db::load_ability_definitions`,
angelegt in Migration 010) in die Ability-Registry. Der Seed der
Migration führt sechs Fähigkeitsdefinitionen (`fire_bolt`,
`healing_light`, `soul_rend`, `frost_nova`, `choke`, `battle_shout`)
und ist in der Migration selbst ausdrücklich als **provisorisch**
markiert („keine endgültigen Fähigkeiten"). Eine vollständige
maßgebliche Fähigkeitsliste existiert damit **nicht**. Bis sie
vorliegt, gilt: die Anlage schreibt keine Zeile in
`character_abilities`, und ein Startfähigkeiten-Umfang wird **nicht**
festgelegt und **nicht** erfunden.

#### Regel 8b — Spaltenzuständigkeit je Statement

Die Anlege-Transaktion enthält genau zwei Statements, die `characters`
schreiben, und sonst nichts außer dem Abschluss:

| Statement | Zuständigkeit (einzige) |
|---|---|
| `INSERT INTO characters (account_id, name, race, char_class)` | diese vier Spalten; `id` entsteht durch AUTO_INCREMENT |
| `UPDATE characters SET strength, agility, intelligence, constitution, wisdom, luck, endurance` (`db::write_attributes`) | die sieben Grundattribute |
| `COMMIT` (bei Erfolg) / Widerruf ohne Commit (jeder Fehlerpfad) | schreibt keine Spalte |

Jede Spalte hat genau eine Zuständigkeit; keine andere Stelle schreibt
bei der Anlage an `characters`. Die 17 nicht geschriebenen Spalten
behalten ihren DB-Default, `id` trägt die vergebene positive
Datenbank-ID. Der Konfliktfall erreicht das
Attribut-Update nicht: die Indexentscheidung (1062, (a)) liegt vor
ihm.

---

## Festlegungen zur Charakteranlage (Entscheidungen a–j)

Die folgenden Punkte sind als Regeln der Charakteranlage beschlossen.
Dieser Abschnitt ist die maßgebliche Regelstelle; andere Dokumente
(zum Beispiel `Projekt-Status.md` Eintrag 22) verweisen nur und
wiederholen die Regeln nicht. Die Struktur des Charaktersatzes —
Feldquellen, Startdatenstruktur, Statement-Zuständigkeiten — steht im
Regelbereich im Abschnitt „Beschlüsse dieser Runde (1–10)"; dieser
Abschnitt verweist darauf und wiederholt jene Tabellen nicht.

### (a) Anlage über `POST /characters`; Ablehnungen je Fall mit stabilem Grund

- Die Anlage läuft über Realm-HTTP `POST /characters` auf dem
  bestehenden HTTP-Port — demselben Port, über den auch
  `GET /characters` angeboten wird.
- Voraussetzung sind Sessionprüfung mit ausschließlich dort hergenommener
  `account_id`, positive Datenbank-ID und Transaktion — sie stehen in der
  VERBINDLICH-Liste des Regelbereichs und werden hier nicht wiederholt.
- Jede Ablehnung trägt einen stabilen Grund im Feld `error`: die
  Antwort ist ein JSON-Objekt mit genau diesem Feld, der Grundtext
  ist kleingeschrieben. Das ist wörtlich die Antwortform der
  bestehenden `GET /characters`-Route (`characters_for`,
  `src/realm-rs/src/health.rs`).
- Die folgende Tabelle führt alle Ablehnungsfälle der Anlage mit ihrer
  Bedeutung, damit der Code später erklärbar ist. Die ersten drei
  Zeilen sind wörtlich an der bestehenden `GET /characters`-Route
  abgelesen und gelten unverändert; die übrigen Zeilen sind
  beschlossen.

| Ablehnungsfall | HTTP-Status | Grund im Feld `error` | Bedeutung |
|---|---|---|---|
| fehlende, leere, mehrfach gelieferte oder ungültige Session; Session ohne gültige positive `account_id` | 401 | `invalid session` | nicht angemeldet oder Sitzung abgelaufen |
| falscher Realm (`realm_id` fehlt, mehrfach, ungültig oder passt nicht zum Realm) | 400 | `wrong realm` | der Request gehört nicht zu diesem Realm |
| Auth-Ausfall (Auth nicht verfügbar oder Prüfaufruf gescheitert) | 503 | `auth unavailable` | die Sitzung konnte nicht geprüft werden |
| Realm-Datenbank nicht erreichbar | 503 | `database unavailable` | Anlage unmöglich, nichts geschrieben |
| Name zu kurz oder zu lang, unzulässige Zeichen, leer nach Trim, reservierte Bezeichnung | 400 | `invalid name` | der Name entspricht nicht den Regeln des Abschnitts „Vorname" |
| Rasse nicht wählbar laut `SELECTABLE_RACES` | 400 | `race not selectable` | die Rasse existiert, ist aber nicht freigeschaltet |
| Name bereits vergeben (Vorprüfung oder Indexkonflikt) | 409 | `name taken` | der Name ist in diesem Realm schon vorhanden |

- Die GET-Route kennt keinen Fall mit 409; dieser Code ist neu und
  an dieser Stelle allein gültig.
- Die Prüfung auf einen bereits vergebenen Namen erfolgt **vor** der
  Transaktion gegen dieselbe Realm-Datenbank. Der UNIQUE-Index
  `idx_char_name` (`src/realm-rs/migrations/001_characters.sql:30`)
  bleibt die letzte Durchsetzung: bringt ein zweiter gleichzeitiger
  Wunsch denselben Namen zuvor, wird der Indexkonflikt fachlich
  identisch beantwortet — derselbe Grund (`name taken`), derselbe
  Statuscode (409), kein 500-Fehler, kein halb angelegter Charakter.

### Voraussetzung der Kollation (Namenseindeutigkeit)

- Die Gleichgültigkeit der Groß-/Kleinschreibung im Namen wird nicht
  vom Index erzeugt, sondern von der Kollation der Spalte `name`.
  `001_characters.sql` deklariert keine Kollation; die Spalte erbt
  sie vom Datenbank-Standard.
- Voraussetzung ist eine Groß-/Kleinschreibung gleichgültige
  Standardkollation der Realm-Datenbank; beide Entwicklungs-
  Datenbanken führen `utf8mb4_german2_ci`. Es wird deshalb keine
  neue Migration angelegt.
- Restrisiko, ausdrücklich festgehalten und nicht beschönigt: auf
  einer Datenbank mit `utf8mb4_bin` oder einer `_cs`-Kollation
  würden „Alda" und „alda" sowohl die Vorprüfung als auch den Index
  passieren — ohne Fehler und ohne Protokollierung. Eine dokumentierte
  Voraussetzung prüft nichts.

### (b) Startattribute aus der RAM-Kopie; Schreibpfad

- Die sieben Startwerte stammen aus der beim Realm-Start geladenen
  RAM-Kopie `race_start_attributes` (`World.race_start_attributes`,
  geladen durch `db::load_race_start_attributes`, fail-closed beim
  Start, danach unveränderlich). Innerhalb der Anlage erfolgt keine
  DB-Lesung der Matrix.
- Der Schreibpfad nutzt die bestehende Attributschreibfunktion
  `db::write_attributes`. Sie schreibt `Attributes.dexterity` in die
  Spalte `characters.agility`; der aus `race_definitions.dexterity`
  abgeleitete Startwert wird damit auf diesem Weg in
  `characters.agility` geschrieben. Die übrigen sechs Werte führen in
  beiden Tabellen denselben Namen.
- Welche Spalten die Anlage insgesamt schreibt, wem sie ihre Werte
  verdanken und welche Spalten ihre DB-Defaults behalten, steht in der
  Feld-zu-Quelle-Tabelle und in Regel 8b (Regelbereich, Abschnitt
  „Beschlüsse dieser Runde") — hier nicht wiederholt.

### (c) Rasse unveränderlich; Klassenwahl bleibt bei Level 9

- Die Rasse ist nach der Anlage unveränderlich. Es gibt keinen
  Nachzug einer anderen Rasse und keinen Respec in V1.
- Die Grundklassenwahl bleibt bei Level 9 (`class.rs`,
  `CLASS_CHOICE_LEVEL`); auch dort gilt weiterhin: kein Respec.

### (d) Keine Starter-Ausrüstung; kanonische Schreibweise der Rasse

- V1 kennt keine Starter-Ausrüstung: ein neu angelegter Charakter
  beginnt ohne Gegenstände.
- Beim Anlegen wird die Rasse in der kleingeschriebenen kanonischen
  Schreibweise gespeichert (`mensch`, `elf`, `andorer`; Abschnitt
  „Spielbare Rassen bei der Erstellung").

### (e) Umfassende Regelstelle für unbekannte Klassenwerte

- Die Zuordnung eines unbekannten Wertes des Feldes `char_class` auf
  `Adventurer` bleibt Bestand und wird **nicht** geändert
  (`ClassStatus::from_db_name`, `src/realm-rs/src/class.rs`).
- Ab dem Umsetzungsschritt wird diese Zuordnung protokolliert —
  ohne Geheimnisse und ohne Namen Dritter. Umsetzungsstelle ist der
  Aufrufpunkt der Zuordnung (Aufrufer von `ClassStatus::from_db_name`,
  `src/realm-rs/src/db.rs`); diese Regelstelle steht hier und ein
  zweites Mal nicht in der Doku von `class.rs`.
- Die Rücknormalisierung im Persistpfad bleibt unverändert: der
  Persistpfad liest den Wert tolerant über `from_db_name` und
  schreibt die kanonische Bezeichnung über
  `ClassStatus::canonical_db_name()` mit `db::write_character_class`
  zurück (`src/realm-rs/src/persist.rs`).

### (j) Startausrüstung später

- Ein fertig angelegter Charakter erhält später Startausrüstung.
  Ihr Inhalt wird **nicht** festgelegt, solange keine ausrüstbaren
  Item-Definitionen mit gesetzten Achsen `equip_subtype` und
  `handling` vorliegen.
- Bis dahin gilt Abschnitt (d): die Anlage schreibt keine Gegenstände.
- Präzisierung des Grundes: Das heißt **nicht** „keine Items in der
  Datenbank". Item-Definitionen sind vorhanden; es fehlt der
  Achsen-Content für die ausrüstbaren Definitionen.

---

## Appearance beim Erstellen

Nur während der Charaktererstellung übermittelt der Client Appearance-Daten an den Server. Dabei werden ausschließlich abstrakte Werte übertragen, beispielsweise:

```text
race = human
gender = male
body = 2
face = 1
hair = 2
hair_color = 4
skin = 3
```

Die Beispiele in diesem Abschnitt und den folgenden Abschnitten sind illustrativ. Für die Rasse gilt ausschließlich der kanonische Rassenwert aus Abschnitt „Spielbare Rassen bei der Erstellung". Dort lautet der Menschenwert `mensch`, nicht `human`.

Der Client übermittelt keine:

* Sprite-Dateien
* Texturen
* Character-Set-Namen
* Asset-Pfade
* Mod-Namen
* grafischen Dateien

Der Server prüft lediglich, ob die übermittelten Werte für die jeweilige Charaktererstellung gültig sind, und speichert anschließend die kanonischen Appearance-Werte beim Charakter.

---

## Serverseitige Charakterdaten

Der Realm-Server kennt beispielsweise:

```text
character_id
race
gender
body
face
hair
hair_color
skin
```

Diese Werte beschreiben den Charakter unabhängig davon, wie ein bestimmter Client ihn grafisch darstellt.

Ein Wert wie:

```text
hair = 2
```

bedeutet serverseitig ausschließlich:

> Der Charakter besitzt Haarvariante 2.

Wie Haarvariante 2 tatsächlich aussieht, ist für den Server bedeutungslos.

---

## Übertragung während des Spiels

Nach der Charaktererstellung funktioniert die Appearance-Übertragung für die Darstellung als One-Way-Verfahren.

```text
Realm-Server
      │
      │ kanonische Charakterdaten
      ▼
    Client
      │
      ├── Character-Set
      ├── alternative Darstellung
      └── kosmetische Mods
      │
      ▼
grafische Darstellung
```

Der Realm-Server liefert die benötigten Charakterdaten an den Client.

Der Client interpretiert diese Daten anschließend anhand seiner lokal vorhandenen Character-Sets.

Der Client sendet **keine Information darüber zurück**, welches Character-Set zur Darstellung verwendet wird.

---

## Character-Sets

Für ein Volk können mehrere grafische Character-Sets existieren.

Beispiel:

```text
Mensch
├── Standard
├── Alternative 1
└── Alternative 2
```

Diese Sets können unterschiedliche künstlerische oder regionale Erscheinungsstile darstellen.

Beispielsweise könnten für Menschen später neben der Standarddarstellung ein asiatisch inspiriertes und ein afrikanisch inspiriertes Character-Set angeboten werden.

Alle Sets bilden jedoch dieselben serverseitigen Appearance-Werte ab.

Beispiel:

```text
Server:
race = human
face = 1
hair = 2

Client A:
Character-Set = Standard

Client B:
Character-Set = Alternative 1

Client C:
Character-Set = Alternative 2
```

Alle drei Spieler betrachten denselben Charakter, können ihn aber unterschiedlich dargestellt bekommen.

---

## Auswahl durch den Betrachter

Welches Character-Set verwendet wird, entscheidet jeder Spieler für seinen eigenen Client.

Die Einstellung kann beispielsweise pro Volk erfolgen:

```text
Menschen  → Alternative 1
Elfen     → Standard
Andorer   → Standard
Luzilla   → Alternative 2
```

Dadurch kann derselbe Charakter auf unterschiedlichen Clients unterschiedlich aussehen.

Die Auswahl eines Character-Sets verändert niemals den eigentlichen Charakter.

---

## Rein kosmetisches System

Character-Sets haben keinerlei Einfluss auf die Spielmechanik.

Sie verändern insbesondere keine:

* Attribute
* Fähigkeiten
* Klassen
* Rassen
* Bewegungsgeschwindigkeit
* Kampfwerte
* Trefferberechnung
* Reichweiten
* serverseitigen Positionen
* Inventardaten
* Questdaten

Auch spielmechanisch relevante Kollisions- oder Zielregeln dürfen nicht von einem lokal verwendeten Character-Set abhängig sein.

Das Character-Set ist ausschließlich eine grafische Interpretation bereits vorhandener Serverdaten.

---

## Mod-Unterstützung

Durch die vollständige Trennung von Charakterdaten und Darstellung können später auch Community-Character-Sets ermöglicht werden.

Beispielsweise:

```text
character_sets/
├── human/
│   ├── default/
│   ├── alternative_01/
│   └── mods/
├── elf/
├── andorer/
└── luzilla/
```

Ein Mod darf die grafische Interpretation eines Charakters verändern.

Er darf jedoch keine serverseitigen Charakterdaten verändern.

Der Realm-Server muss weder wissen noch überprüfen, welches lokale Character-Set installiert oder aktiviert wurde.

---

## Keine Übertragung von Character-Set-Informationen

Folgende Informationen werden grundsätzlich nicht an den Realm-Server übertragen:

```text
selected_character_set
installed_character_sets
mod_name
sprite_path
texture_path
asset_name
render_style
```

Der Server benötigt diese Informationen nicht.

Damit bleibt auch ein selbst erstelltes Character-Set eines Modders vollständig clientseitig.

---

## Sicherheits- und Architekturgrenze

Die Trennung verhindert, dass kosmetische Client-Modifikationen Teil der autoritativen Spiellogik werden.

Der Server bleibt für den tatsächlichen Charakterzustand verantwortlich.

Der Client ist ausschließlich für dessen Darstellung verantwortlich.

```text
SERVER
────────────────────────────
Charakteridentität
Volk
Appearance-Werte
Spielwerte
Position
Ausrüstung
Gameplay-Zustand
────────────────────────────
            │
            ▼
CLIENT
────────────────────────────
Character-Set auswählen
Sprites/Assets laden
Appearance-Werte interpretieren
Charakter darstellen
kosmetische Mods anwenden
────────────────────────────
```

## Architekturregel

> **Appearance-Werte gehören zum Charakter. Character-Sets gehören zum Client.**

> **Nur bei der Charaktererstellung übermittelt der Client die ausgewählten kanonischen Appearance-Werte an den Server. Danach liefert der Server diese Werte zur Darstellung an die Clients. Welches Character-Set ein Client daraus verwendet, ist ausschließlich dessen lokale Entscheidung.**

Dadurch können neue offizielle Character-Sets und Community-Mods später ergänzt werden, ohne das serverseitige Charaktermodell oder die Spielmechanik verändern zu müssen.
