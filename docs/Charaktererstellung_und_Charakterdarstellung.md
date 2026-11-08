# Charaktererstellung und clientseitige Charakterdarstellung

## Grundprinzip

Die spielmechanische Identität eines Charakters und seine grafische Darstellung werden in Andora strikt voneinander getrennt.

> **Der Server bestimmt, was ein Charakter ist. Der Client bestimmt, wie dieser Charakter dargestellt wird.**

Der Server speichert ausschließlich abstrakte Charakter- und Appearance-Werte. Grafiken, Character-Sets, alternative Darstellungen und kosmetische Mods sind ausschließlich Aufgabe des Clients.

---

## Charaktererstellung

Nur während der Charaktererstellung übermittelt der Client Appearance-Daten an den Server.

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
5. **Initiale Datensätze** – neben dem Charakter-Datensatz werden die zugehörigen Startdatensätze angelegt (zum Beispiel Ausrüstung/Inventar, Fähigkeiten, Questzustand), damit kein unvollständiger Charakter entsteht.
6. **Transaktionsgrenzen** – die Anlage der Datensätze erfolgt so, dass ein Fehler keinen halb angelegten Charakter hinterlässt.

### Angebot des Erstellungsablaufs

Die Charaktererstellung wird vom **Realm-Server** über seinen
bestehenden HTTP-Port angeboten, analog zur bereits eingebauten
Charakterliste `GET /characters`:

```text
POST /characters?session_id=<Login-Session>
```

- Die `session_id` wird über die vorhandene signierte Auth-API
  geprüft; die `account_id` stammt ausschließlich aus dieser Prüfung.
- Eine clientgelieferte `account_id` oder `character_id` ist ohne
  Wirkung und wird abgelehnt.
- Die `character_id` ist die vom Server vergebene positive
  Datenbank-ID in der Realm-Datenbank dieses Realms.
- Die Anlage erfolgt in einer Transaktion, damit kein halber
  Charakter zurückbleibt.
- Die Prüfung auf einen bereits vergebenen Namen erfolgt vor der
  Transaktion gegen dieselbe Realm-Datenbank.

Der Realm-Einstieg selbst bleibt davon unberührt und legt niemals
einen Charakter an (`Login_Realm_Architektur.md` Abschnitt 6).

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

- Jeder neu erstellte Charakter beginnt als **Abenteurer**; die
  Grundklassenwahl erfolgt wie im Klassensystem dokumentiert
  später (`Klassensystem.md`, `Rassen-Fraktionen.md` Abschnitt 10).
- Die sieben Grundattribute werden aus der rassischen
  Startwertmatrix gesetzt (`Attribute_und_Regeneration.md`
  Abschnitt 2).
- Die zugehörigen Startdatensätze werden vollständig und
  fehlerfrei angelegt. Der konkrete Umfang der Startdatensätze
  wird mit dem Umsetzungsauftrag festgelegt; es wird keine
  Starterausrüstung erfunden.

### Umgang mit vorhandenen Test- und Platzhaltercharakteren

Bestehende Charakterdatensätze werden **nicht** per Migration auf
die kanonischen Rassenwerte umgestellt. Die Erstellung schreibt
ausschließlich kanonische Werte. Der Realm liest das Feld
`race` tolerant: ein unbekannter oder abweichend geschriebener
Wert wird protokolliert, verhindert aber **nicht** den Einstieg.
Vorhandene Platzhaltercharaktere werden nicht umgeschrieben und nicht
per Migration angepasst; sie werden mit der Zeit durch neu angelegte
Charaktere abgelöst.

### Appearance beim Erstellen

Dabei werden ausschließlich abstrakte Werte übertragen, beispielsweise:

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

## Speicherort der Rassen- und Startattributdaten

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
besitzt der Realm bereits in seinem Attribut-Lese- und -Schreibpfad. Zu
ergänzen ist sie bei der Charakteranlage: der aus `race_definitions.dexterity`
abgeleitete Startwert muss beim Anlegen in die Spalte `characters.agility`
geschrieben werden. Diese Verwendung der Rassetabelle bei der Charakteranlage
ist derzeit nicht implementiert.

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

**Status: Datenbasis, Loader und realmseitige Konfiguration implementiert.
Die Anwendung der Matrix bei der Charakteranlage (`POST /characters`) ist
noch nicht implementiert.**

---

## Festlegungen zur Charakteranlage (Entscheidungen a–j)

Die folgenden Punkte sind als Regeln der Charakteranlage beschlossen.
Dieser Abschnitt ist die maßgebliche Regelstelle; andere Dokumente
(zum Beispiel `Projekt-Status.md` Eintrag 22) verweisen nur und
wiederholen die Regeln nicht.

### (a) Anlage über `POST /characters`; Ablehnungen je Fall mit stabilem Grund

- Die Anlage läuft über Realm-HTTP `POST /characters` auf dem
  bestehenden HTTP-Port — demselben Port, über den auch
  `GET /characters` angeboten wird.
- Die `account_id` stammt ausschließlich aus der Sessionprüfung; die
  vergebene `character_id` ist eine positive Datenbank-ID; die Anlage
  erfolgt in einer Transaktion.
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
