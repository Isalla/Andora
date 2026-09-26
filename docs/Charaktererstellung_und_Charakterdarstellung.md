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
4. **Rasse, Klasse und Appearance** – die übermittelten Werte werden gegen die gültigen Werte geprüft und in kanonischer Form gespeichert; die Grundklasse folgt den Regeln des Klassensystems.
5. **Initiale Datensätze** – neben dem Charakter-Datensatz werden die zugehörigen Startdatensätze angelegt (zum Beispiel Ausrüstung/Inventar, Fähigkeiten, Questzustand), damit kein unvollständiger Charakter entsteht.
6. **Transaktionsgrenzen** – die Anlage der Datensätze erfolgt so, dass ein Fehler keinen halb angelegten Charakter hinterlässt.

### Offene Architekturentscheidung

Aus den bestehenden Dokumenten geht **nicht** eindeutig hervor, welcher Dienst den Erstellungsablauf anbietet und welche Datenbanktransaktion er verwendet. Diese Festlegung ist eine **gesonderte Entscheidung** und wird hier nicht vorweggenommen. Solange sie nicht getroffen ist, gilt die Regel oben unabhängig vom anbietenden Dienst: kein Anlegen im Realm-Einstieg, serverseitig vergebene positive `character_id`, fail-closed bei fehlendem oder fremdem `char_id` beim Lookup.

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
