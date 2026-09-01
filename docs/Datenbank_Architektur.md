# Datenbank-Architektur

## 1. Ziel

Andora verwendet mehrere logisch getrennte Datenbanken.

Die Trennung dient dazu:

-   Verantwortlichkeiten klar zu halten
-   sensible Accountdaten vom World-Server fernzuhalten
-   Realm-Zustände voneinander zu isolieren
-   statische Weltdaten zentral bereitzustellen
-   spätere Erweiterungen und Migrationen übersichtlich zu halten
-   Datenbankrechte nach dem Prinzip der minimal notwendigen
    Berechtigung zu vergeben

Grundregel:

> Jede Datenbank hat eine klar definierte Aufgabe.\
> Daten dürfen nicht beliebig zwischen Datenbanken verteilt werden.

------------------------------------------------------------------------

## 2. Datenbank-Übersicht

``` text
MariaDB
│
├── auth
│   └── Accounts, Login, Sessions, Realm-/Server-Registrierung
│
├── character
│   └── persönliche Charakterdaten und Fortschritt
│
├── world_data
│   └── statische globale Weltdaten
│
└── realm_state_<realm>
    └── persistenter Zustand eines konkreten Realms
```

Beispiel:

``` text
auth
character
world_data
realm_state_de1
realm_state_de2
realm_state_en1
```

------------------------------------------------------------------------

## 3. auth

Datei:

``` text
server/db/auth/auth.sql
```

### Aufgabe

Die `auth`-Datenbank verwaltet Identität, Authentifizierung und die
technische Realm-Verwaltung.

Sie beantwortet vor allem die Frage:

> Wer ist der Spieler und welche Server gehören offiziell zu Andora?

### Enthält

Zum Beispiel:

-   Accounts
-   Passwort-Hashes
-   verschlüsselte E-Mail-Adressen
-   E-Mail-Lookup-Hashes
-   Accountstatus
-   Sessions
-   Login-Tokens
-   registrierte Realms
-   registrierte World-Server
-   World-Server-Credentials
-   Serverfreigaben
-   Realm-Metadaten
-   Fresh-Start-Konfiguration
-   Transferregeln
-   Heartbeat-/Online-Informationen

### Enthält ausdrücklich nicht

-   Charakterinventar
-   Charakterausrüstung
-   Skills
-   Questfortschritt
-   Gildenstädte
-   Realm-Politik
-   Monsterzustände
-   Loot
-   NPC-Weltzustände

### Zugriff

Nur der Auth/API-Service besitzt direkten Zugriff auf die
`auth`-Datenbank.

Der Auth/API-Service verwendet dafür den technischen DB-Benutzer
`andora_auth`. Dieser Benutzer erhält ausschließlich die für `auth`
notwendigen Rechte.

Die `AUTH_DB_*`-Zugangsdaten befinden sich ausschließlich in der
Konfiguration des Auth/API-Service.

Webseite, Loginserver, Realm-/World-Server und Client erhalten keinen
direkten Zugriff auf `auth`. Webseite, Loginserver und
Realm-/World-Server kommunizieren für Auth-Funktionen ausschließlich
über definierte Endpunkte des Auth/API-Service. Jeder Dienst erhält
eigene Service-Credentials und nur die minimal notwendigen
API-Berechtigungen.

Der Realm-/World-Server darf insbesondere keine Passwort-Hashes,
E-Mail-Daten, Verschlüsselungsschlüssel oder andere sensible
Accountdaten direkt lesen.

Die genaue Sicherheits- und Service-Struktur ist in
`docs/Auth_API_Architektur.md` definiert.

> Nur der Auth/API-Service besitzt direkten Zugriff auf die
> Auth-Datenbank.

------------------------------------------------------------------------

## 4. character

Datei:

``` text
server/db/character/character.sql
```

### Aufgabe

Die `character`-Datenbank speichert Daten, die einem Charakter
persönlich gehören.

Sie beantwortet vor allem die Frage:

> Was gehört zu diesem Charakter und welchen persönlichen Fortschritt
> hat er?

### Enthält

Zum Beispiel:

-   Charakter-Grunddaten
-   Name
-   Rasse
-   Geschlecht
-   Appearance-Werte
-   Klasse
-   Level
-   Erfahrung
-   Attribute
-   Inventar
-   Ausrüstung
-   Skills
-   persönliche Questfortschritte
-   persönliche Rufwerte
-   persönliche NPC-Beziehungen
-   persönliche Freischaltungen
-   persönliche Reise-/Portal-Freischaltungen
-   persönliche Crafting-Fortschritte
-   weitere charaktergebundene Daten

### Realm-Zuordnung

Ein Charakter besitzt eine Zuordnung zu einem Realm.

Dadurch kann geprüft werden:

-   auf welchem Realm der Charakter aktuell beheimatet ist
-   ob ein Transfer erlaubt ist
-   ob Fresh-Start-Regeln gelten
-   wie viele Charaktere ein Account auf einem Realm besitzt

Realm-spezifische Weltzustände selbst gehören jedoch nicht in
`character`.

------------------------------------------------------------------------

## 5. world_data

Datei:

``` text
server/db/world_data/world_data.sql
```

### Aufgabe

`world_data` enthält statische bzw. versionierte Definitionen der
Spielwelt.

Grundsatz:

> world_data beschreibt, was in Andora existieren kann.

### Enthält

Zum Beispiel:

-   Itemdefinitionen
-   Waffen- und Rüstungsdefinitionen
-   Monsterdefinitionen
-   NPC-Grunddefinitionen
-   Loot-Tabellen
-   Regionen
-   Dungeons
-   Ressourcen
-   Spawnregeln
-   Händler-Grunddaten
-   Crafting-Grunddaten
-   Weltobjektdefinitionen
-   weitere globale Definitionsdaten

### Wichtig

`world_data` ist nicht der aktuelle Zustand eines Realms.

Beispiel:

``` text
world_data:
NPC Borin existiert als NPC-Definition.

realm_state_de1:
Borin befindet sich gerade in Dorf A.

realm_state_de2:
Borin befindet sich gerade auf dem Weg nach Stadt B.
```

### Zugriff

Realm-Server erhalten auf `world_data` grundsätzlich nur die Rechte, die
sie zum Lesen benötigen.

Im normalen Serverbetrieb sollte ein Realm-Server globale
Definitionsdaten nicht verändern.

Änderungen an `world_data` erfolgen kontrolliert über:

-   SQL-Migrationen
-   Deployment
-   Content-Updates

------------------------------------------------------------------------

## 6. realm_state

Datei für das Grundschema:

``` text
server/db/realm_state/realm_state.sql
```

Jeder unabhängige öffentliche Realm besitzt einen eigenen persistenten
Realm-State.

Beispiele:

``` text
realm_state_de1
realm_state_de2
realm_state_en1
```

### Aufgabe

Der Realm-State speichert alles, was in einer konkreten Welt tatsächlich
passiert ist.

Grundsatz:

> realm_state beschreibt, was in diesem Realm passiert ist.

### Enthält

Zum Beispiel:

-   Gilden
-   Gildenmitgliedschaften mit Realm-Bezug
-   Gildenstädte
-   politische Herrschaft
-   Realm-Wirtschaft
-   Auktionen mit Realm-Bezug
-   Weltfortschritt
-   Expansion-/Content-Fortschritt
-   persistente NPC-Zustände
-   persistente NPC-Positionen
-   Weltveränderungen
-   regionale Zustände
-   Eventzustände
-   Besitzverhältnisse
-   persistente Gebäude
-   weitere realmgebundene Daten

### Realm-Isolation

Öffentlich getrennte Realms besitzen getrennte persistente Weltzustände.

Beispiel:

``` text
DE-1
→ realm_state_de1

DE-2
→ realm_state_de2
```

Eine Gildenstadt auf DE-1 existiert dadurch nicht automatisch auf DE-2.

------------------------------------------------------------------------

## 7. Datenbankverbindungen und DB-Benutzer

Jede Andora-Datenbank erhält eine vollständig eigene
Verbindungskonfiguration, einen eigenen technischen MariaDB-Benutzer und
einen eigenen Connection-Pool.

Grundsatz:

> Eine Datenbank = eigene Verbindungskonfiguration + eigener technischer
> DB-Benutzer + eigener Connection-Pool.

Dies gilt auch dann, wenn während der Entwicklung zunächst alle
Datenbanken auf demselben MariaDB-Server liegen.

### AUTH

``` text
AUTH_DB_HOST=
AUTH_DB_PORT=3306
AUTH_DB_USER=
AUTH_DB_PASSWORD=
AUTH_DB_NAME=auth
```

Diese Verbindungskonfiguration gehört ausschließlich zum
Auth/API-Service. Webseite, Loginserver und Realm-/World-Server erhalten
keine `AUTH_DB_*`-Konfiguration.

### CHARACTER

``` text
CHARACTER_DB_HOST=
CHARACTER_DB_PORT=3306
CHARACTER_DB_USER=
CHARACTER_DB_PASSWORD=
CHARACTER_DB_NAME=character
```

### WORLD DATA

``` text
WORLD_DATA_DB_HOST=
WORLD_DATA_DB_PORT=3306
WORLD_DATA_DB_USER=
WORLD_DATA_DB_PASSWORD=
WORLD_DATA_DB_NAME=world_data
```

### REALM STATE

``` text
REALM_STATE_DB_HOST=
REALM_STATE_DB_PORT=3306
REALM_STATE_DB_USER=
REALM_STATE_DB_PASSWORD=
REALM_STATE_DB_NAME=realm_state_de1
```

### Warum diese Trennung?

Heute dürfen alle vier Datenbanken auf demselben MariaDB-Server liegen:

``` text
MariaDB Server A
├── auth
├── character
├── world_data
└── realm_state_de1
```

Später können sie ohne grundlegenden Umbau auf unterschiedliche
Datenbankserver verteilt werden:

``` text
DB-Server A → auth
DB-Server B → character
DB-Server C → world_data
DB-Server D → realm_state_de1
```

Damit können Last, Wartung und Sicherheitsgrenzen später unabhängig
behandelt werden.

### Rechte

Jeder technische DB-Benutzer erhält ausschließlich Rechte auf seinen
eigenen Datenbankbereich.

``` text
andora_auth       → nur auth
andora_character  → nur character
andora_world_data → nur world_data
andora_realm_de1  → nur realm_state_de1
```

Für `world_data` soll der laufende Realm-/World-Server grundsätzlich nur
die für den Betrieb erforderlichen Leserechte verwenden. Änderungen an
statischen Weltdaten erfolgen kontrolliert über Migrationen, Deployment
oder Content-Updates.

Ein Realm-State-Benutzer erhält keinen Zugriff auf andere
Realm-State-Datenbanken.

### Keine Sammelverbindung

Es gibt keine allgemeine `WORLD_DB_*`-Sammelkonfiguration für
`character`, `world_data` und `realm_state`.

Ebenso gibt es keinen Legacy-Fallback auf alte allgemeine
`DB_*`-Variablen.

Fehlt eine notwendige DB-Konfiguration, soll der betroffene Dienst mit
einer klaren Fehlermeldung abbrechen, statt stillschweigend eine andere
Datenbank zu verwenden.

------------------------------------------------------------------------

## 8. Mehrere technische World-Prozesse

Ein Realm kann später aus mehreren technischen World-Server-Prozessen
bestehen.

Diese Prozesse gehören weiterhin zum selben Realm und dürfen denselben
Realm-State verwenden.

Beispiel:

``` text
Realm DE-1

World-Prozess 1 ─┐
World-Prozess 2 ─┼── realm_state_de1
World-Prozess 3 ─┘
```

Das erzeugt keine neue Welt.

Ein neuer öffentlicher Realm benötigt dagegen einen eigenen persistenten
Realm-State.

------------------------------------------------------------------------

## 9. SQL-Dateibaum

Die bisherige zentrale `schema.sql` wird nicht dauerhaft
weiterverwendet.

Der Datenbankbaum lautet:

``` text
server/
└── db/
    ├── auth/
    │   ├── auth.sql
    │   └── migrations/
    │
    ├── character/
    │   ├── character.sql
    │   └── migrations/
    │
    ├── world_data/
    │   ├── world_data.sql
    │   ├── migrations/
    │   └── seed/
    │
    └── realm_state/
        ├── realm_state.sql
        └── migrations/
```

Beispiele für Migrationen:

``` text
server/db/auth/migrations/
├── 001_accounts.sql
├── 002_sessions.sql
└── 003_realms.sql

server/db/character/migrations/
├── 001_characters.sql
├── 002_inventory.sql
└── 003_skills.sql

server/db/realm_state/migrations/
├── 001_guilds.sql
├── 002_world_state.sql
└── 003_npc_state.sql
```

------------------------------------------------------------------------

## 10. Tabellen-Ownership

Jede Tabelle gehört genau zu einem fachlichen Datenbankbereich.

Die folgende Zuordnung zeigt vorhandene bzw. bereits konkret geplante
Tabellen. Sie ist ausdrücklich **keine vollständige Liste aller
zukünftigen Tabellen**. Weitere Tabellen werden mit der Implementierung
der jeweiligen Systeme ergänzt und gemäß ihrer fachlichen Verantwortung
zugeordnet.

``` text
accounts
→ auth

sessions
→ auth

realms
→ auth

world_servers
→ auth

world_server_credentials
→ auth

world_server_heartbeats
→ auth

characters
→ character

item_definitions
→ world_data

guilds
→ realm_state

guild_members
→ realm_state

auctions
→ realm_state

guild_cities
→ realm_state
```

Eine Tabelle wird nicht aus Bequemlichkeit in eine andere Datenbank
gelegt.

Wenn ein Dienst Informationen aus einem Bereich benötigt, auf den er
keinen direkten Zugriff haben soll, erfolgt der Zugriff über eine
definierte Server-/API-Schnittstelle.

> Datenbankzugriff folgt der Verantwortung des Dienstes und nicht der
> Bequemlichkeit des Codes.

------------------------------------------------------------------------

## 11. Keine Cross-DB-Abhängigkeiten ohne Prüfung

Neue Systeme müssen vor dem Anlegen einer Tabelle festlegen:

1.  Wem gehören die Daten?
2.  Sind sie accountgebunden?
3.  Sind sie charaktergebunden?
4.  Sind sie statische Weltdaten?
5.  Sind sie Zustand eines konkreten Realms?

Erst danach wird entschieden, in welche Datenbank die Tabelle gehört.

------------------------------------------------------------------------

## 12. Migrationen und db_version

Jede Andora-Datenbank besitzt eine eigene Migrationshistorie und eine
eigene `db_version`-Tabelle:

``` text
auth
character
world_data
realm_state_<realm>
```

Beispiel:

``` sql
CREATE TABLE IF NOT EXISTS db_version (
    version INT NOT NULL PRIMARY KEY,
    migration VARCHAR(255) NOT NULL,
    applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### Automatische Startprüfung

Beim Start prüft der für die jeweilige Datenbank zuständige Dienst den
installierten Schema-Stand.

``` text
Dienst startet
      ↓
Datenbankverbindung herstellen
      ↓
db_version prüfen/erzeugen
      ↓
höchste installierte Version ermitteln
      ↓
vorhandene Migrationen prüfen
      ↓
fehlende Migrationen numerisch anwenden
      ↓
jede erfolgreiche Migration in db_version eintragen
      ↓
Dienst normal starten
```

Schlägt eine notwendige Migration fehl, wird der Start des betroffenen
Dienstes abgebrochen und der Fehler klar geloggt.

### Zuständigkeit für auth

Da ausschließlich der Auth/API-Service direkten Zugriff auf `auth`
besitzt, ist ausschließlich dieser Dienst für Prüfung und Anwendung der
`auth`-Migrationen verantwortlich.

Webseite, Loginserver und Realm-/World-Server führen keine Migrationen
auf `auth` aus.

Die Zuständigkeit für `character`, `world_data` und `realm_state` wird
anhand der jeweiligen Dienstverantwortung separat festgelegt. Diese
Datenbanken werden nicht automatisch dem Auth/API-Service zugeordnet.

### Unveränderliche Migrationen

Bereits erfolgreich angewendete Migrationen werden niemals nachträglich
verändert.

Muss beispielsweise ein bereits durch `001_accounts.sql` angelegtes
Schema später geändert werden, wird eine neue Migration erstellt, zum
Beispiel:

``` text
005_encrypt_account_email.sql
```

### Regeln für Qwen/OpenCode

Qwen/OpenCode darf SQL-Migrationen erstellen und während der Entwicklung
mit den dafür vorgesehenen eingeschränkten DB-Zugangsdaten anwenden.

Dabei gelten folgende Regeln:

-   keine MariaDB-Admin-Credentials verwenden
-   keine DB-Benutzer selbst anlegen
-   keine Rechte selbst verändern
-   keine fremden Datenbanken verändern
-   keine Tabellen ohne Zuordnung zu einem DB-Bereich erstellen
-   keine neue zentrale `schema.sql` aufbauen
-   bereits angewendete Migrationen nicht nachträglich verändern
-   Migrationen nur mit dem vorgesehenen technischen DB-Benutzer
    anwenden

Produktive Migrationen werden später gesondert geregelt.

------------------------------------------------------------------------

## 13. Sicherheitsgrenzen

### Auth

Nur der Auth/API-Service besitzt direkten Zugriff auf sensible
Accountdaten und auf die `auth`-Datenbank.

Webseite, Loginserver und Realm-/World-Server greifen für
Auth-Funktionen ausschließlich über die Auth-API zu und besitzen eigene
Service-Credentials mit minimal notwendigen Berechtigungen.

### Character

Enthält persönliche Spielfortschritte, aber keine Passwörter oder
E-Mail-Schlüssel.

### World Data

Ist überwiegend lesend und enthält keine Account-Geheimnisse.

### Realm State

Enthält nur den Zustand des jeweiligen Realms.

Dadurch führt die Kompromittierung eines Realm-Servers nicht automatisch
zum Zugriff auf:

-   Passwörter
-   E-Mail-Adressen
-   andere Realm-Zustände
-   administrative Datenbankzugänge

------------------------------------------------------------------------

## 14. Leitsätze

> Auth weiß, wer du bist.

> Character weiß, was deinem Charakter gehört.

> World Data weiß, was in Andora existieren kann.

> Realm State weiß, was in dieser Welt passiert ist.

> Jeder Realm-Server bekommt nur die Datenbankrechte, die er tatsächlich
> benötigt.

> Eine Datenbank ist kein Ablageort für beliebige Tabellen, sondern
> besitzt eine klar definierte Verantwortung.

> Nur der Auth/API-Service besitzt direkten Zugriff auf die
> Auth-Datenbank.

> Webseite, Loginserver und Realm-/World-Server verwenden für
> Auth-Funktionen ausschließlich die Auth-API.

> Jede Andora-Datenbank besitzt ihre eigene `db_version`-Historie.

> Bereits angewendete Migrationen werden niemals nachträglich verändert.
