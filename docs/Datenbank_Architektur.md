# Datenbank-Architektur

## 1. Ziel

Andora verwendet mehrere logisch getrennte Datenbanken.

Die Trennung dient dazu:

* Verantwortlichkeiten klar zu halten
* sensible Accountdaten von Realm-Servern fernzuhalten
* Realm-Zustände vollständig voneinander zu isolieren
* Charakter- und Spieldaten eines Realms gemeinsam zu verwalten
* die vollständigen statischen Definitionen und die dynamischen Zustände eines Realms gemeinsam bereitzustellen
* Realm-Versionen parallel und eigenständig aktualisierbar zu halten
* Transaktionen innerhalb eines Realms möglichst einfach und zuverlässig zu halten
* spätere Erweiterungen und Migrationen übersichtlich zu halten
* Datenbankrechte nach dem Prinzip der minimal notwendigen Berechtigung zu vergeben

Grundregel:

> Jede Datenbank hat eine klar definierte Aufgabe.
> Daten dürfen nicht beliebig zwischen Datenbanken verteilt werden.

Zusätzliche Grundregel:

> Charakterdaten gehören zu dem Realm, auf dem der Charakter existiert.

Eine separate globale `character`-Datenbank wird daher nicht verwendet.

---

## 2. Datenbank-Übersicht

```text
MariaDB
│
├── auth
│   └── Accounts, Login, Sessions, Realm-/Server-Registrierung
│
├── realm_state_de1
│   └── statische Weltdefinitionen + Charaktere + vollständiger persistenter Zustand von DE-1
│
├── realm_state_de2
│   └── statische Weltdefinitionen + Charaktere + vollständiger persistenter Zustand von DE-2
│
└── realm_state_en1
    └── statische Weltdefinitionen + Charaktere + vollständiger persistenter Zustand von EN-1
```

Beispiel:

```text
auth
realm_state_de1
realm_state_de2
realm_state_en1
```

Die früheren separaten Datenbanken:

```text
character
world_data
```

entfallen. Statische Weltdefinitionen und Charakterdaten sind in die jeweilige `realm_state_<realm>`-Datenbank integriert.

---

## 3. auth

Datei:

```text
src/api/db/auth/auth.sql
```

### Aufgabe

Die `auth`-Datenbank verwaltet Identität, Authentifizierung und die technische Realm-Verwaltung.

Sie beantwortet vor allem die Frage:

> Wer ist der Spieler und welche Server gehören offiziell zu Andora?

### Enthält

Zum Beispiel:

* Accounts
* Passwort-Hashes
* verschlüsselte E-Mail-Adressen
* E-Mail-Lookup-Hashes
* Accountstatus
* Sessions
* Login-Tokens
* registrierte Realms
* registrierte World-Server
* World-Server-Credentials
* Serverfreigaben
* Realm-Metadaten
* Fresh-Start-Konfiguration
* Transferregeln
* Heartbeat-/Online-Informationen

### Enthält ausdrücklich nicht

* Charakterinventar
* Charakterausrüstung
* Skills
* persönliche Questfortschritte
* Charakterattribute
* Item-Instanzen
* Crafting-Aufträge
* Gildenstädte
* Realm-Politik
* Monsterzustände
* Loot
* NPC-Weltzustände

### Zugriff

Nur der Auth/API-Service besitzt direkten Zugriff auf die `auth`-Datenbank.

Der Auth/API-Service verwendet dafür den technischen DB-Benutzer `andora_auth`.

Dieser Benutzer erhält ausschließlich die für `auth` notwendigen Rechte.

Die `AUTH_DB_*`-Zugangsdaten befinden sich ausschließlich in der Konfiguration des Auth/API-Service.

Webseite, Loginserver, Realmserver, Coordinator und Client erhalten keinen direkten Zugriff auf `auth`.

Webseite, Loginserver und Realmserver kommunizieren für Auth-Funktionen ausschließlich über definierte Endpunkte des Auth/API-Service.

Jeder Dienst erhält eigene Service-Credentials und nur die minimal notwendigen API-Berechtigungen.

Der Realmserver darf insbesondere keine:

* Passwort-Hashes
* E-Mail-Daten
* Verschlüsselungsschlüssel
* Login-Geheimnisse
* andere sensible Accountdaten

direkt lesen.

Die genaue Sicherheits- und Service-Struktur ist in:

```text
docs/Auth_API_Architektur.md
```

definiert.

> Nur der Auth/API-Service besitzt direkten Zugriff auf die Auth-Datenbank.

---

## 4. Statische Weltdefinitionen (in realm_state)

Eine zentrale `world_data`-Datenbank existiert nicht mehr.

Die statischen bzw. versionierten Definitionen der Spielwelt liegen realmbezogen in der Realm-Datenbank `realm_state_<realm>` des jeweiligen Realms.

Grundsatz:

> Die statische Inhaltsversion eines Realms beschreibt, was in diesem Realm existieren kann.

### Enthält (je Realm)

Zum Beispiel:

* Itemdefinitionen
* Waffen- und Rüstungsdefinitionen
* Monsterdefinitionen
* NPC-Grunddefinitionen
* Loot-Tabellen
* Regionen
* Dungeons
* Ressourcen
* Spawnregeln
* Händler-Grunddaten
* Crafting-Grunddaten
* Weltobjektdefinitionen
* weitere Realm-/Inhaltsversion-definierende Daten

### Wichtig

Die statischen Definitionen eines Realms beschreiben, was in dieser Realm-Inhaltsversion existieren kann. Sie sind von den dynamischen Zuständen desselben Realms zu unterscheiden.

Beispiel:

```text
realm_state_de1 (statisch):
NPC Borin existiert als NPC-Definition in der Inhaltsversion von DE-1.

realm_state_de1 (dynamisch):
Borin befindet sich gerade in Dorf A.

realm_state_de2 (dynamisch):
Borin befindet sich gerade auf dem Weg nach Stadt B.
```

Ebenso:

```text
realm_state_de1 (statisch):
Definition eines seltenen Eisenschwertes der Inhaltsversion von DE-1.

realm_state_de1 (dynamisch):
Konkrete Item-Instanz dieses Schwertes gehört Charakter 4711.
```

### Zugriff und Änderung

Änderungen an den statischen Definitionen eines Realms erfolgen kontrolliert über Realm-Updates, die den Realm in den Wartungsmodus versetzen (siehe `Deployment_Betriebsarchitektur.md`), sowie über SQL-Migrationen und Content-Updates der jeweiligen Realm-Datenbank. Andere Realms werden davon nicht betroffen.

---

## 5. realm_state

Datei für das Grundschema:

```text
src/realm/db/realm_state/realm_state.sql
```

Jeder unabhängige öffentliche Realm besitzt eine eigene persistente Realm-Datenbank.

Beispiele:

```text
realm_state_de1
realm_state_de2
realm_state_en1
```

### Aufgabe

Die Realm-Datenbank speichert sämtliche spielrelevanten persistenten Daten eines konkreten Realms, einschließlich seiner statischen Inhaltsversion.

Sie beantwortet damit drei zentrale Fragen:

> Was kann in dieser Realm-Version existieren?

> Was gehört den Charakteren dieses Realms?

> Was ist in diesem Realm passiert?

Die früheren Trennungen zwischen persönlicher Character-Datenbank, zentraler `world_data`-Datenbank und Realm-State entfallen. Jeder Realm besitzt seinen vollständigen statischen und dynamischen Datenstand selbst.

### Enthält statische Weltdefinitionen

Zum Beispiel:

* Itemdefinitionen
* Waffen- und Rüstungsdefinitionen
* Monsterdefinitionen
* NPC-Grunddefinitionen
* Loot-Tabellen
* Regionen
* Dungeons
* Ressourcen
* Spawnregeln
* Händler-Grunddaten
* Crafting-Grunddaten
* Weltobjektdefinitionen
* weitere statische Definitionsdaten dieser Realm-Inhaltsversion

### Enthält Charakterdaten

Zum Beispiel:

* Charakter-Grunddaten
* Account-Referenz
* Name
* Rasse
* Geschlecht
* Appearance-Werte
* Klasse
* Level
* Erfahrung
* Attribute
* Inventar
* Ausrüstung
* Item-Instanzen
* Skills
* persönliche Questfortschritte
* persönliche Rufwerte
* persönliche NPC-Beziehungen
* persönliche Freischaltungen
* persönliche Reise-/Portal-Freischaltungen
* persönliche Crafting-Fortschritte
* Gold
* weitere charaktergebundene Daten

### Enthält Realm-Daten

Zum Beispiel:

* Gilden
* Gildenmitgliedschaften
* Gildenstädte
* politische Herrschaft
* Realm-Wirtschaft
* Auktionen
* Weltfortschritt
* Expansion-/Content-Fortschritt
* persistente NPC-Zustände
* persistente NPC-Positionen
* Weltveränderungen
* regionale Zustände
* Eventzustände
* Besitzverhältnisse
* persistente Gebäude
* weitere realmgebundene Daten

### Enthält außerdem realmbezogene Systeme

Dazu können unter anderem gehören:

* Crafting-Jobs
* Crafting-Job-Items
* Mail-System
* Mail-Anhänge
* Item-Recovery
* Rückerstattungen
* offene Spieleraufträge
* Realm-Supportinformationen

Diese Daten gehören bewusst in dieselbe Realm-Datenbank, weil sie eng mit Charakteren, Items und dem Zustand dieses Realms verbunden sind.

---

## 6. Charaktere gehören zu ihrem Realm

Ein Charakter existiert ausschließlich innerhalb der Datenbank seines Realms.

Beispiel:

```text
realm_state_de1.characters
→ Charaktere von DE-1

realm_state_de2.characters
→ Charaktere von DE-2
```

Ein Charakter von DE-1 wird nicht zusätzlich in einer globalen Character-Datenbank gespeichert.

Dadurch können charakterbezogene Vorgänge innerhalb derselben Datenbank verarbeitet werden.

Zum Beispiel:

```text
Charakter
↓
Inventar
↓
Item-Instanzen
↓
Crafting-Job
↓
Mail-Rückerstattung
```

Alle beteiligten Tabellen befinden sich innerhalb desselben Realm-Datenbankbereichs.

---

## 7. Vorteile für Transaktionen und Recovery

Die Integration der Charakterdaten in die Realm-Datenbank vereinfacht Vorgänge, die mehrere spielbezogene Datensätze gleichzeitig betreffen.

Besonders wichtig ist dies bei:

* Crafting
* Item-Übergaben
* Goldzahlungen
* Auktionen
* Handel
* Mail
* Loot
* Recovery
* Rückerstattungen
* Supportfällen

Beispiel Crafting:

```text
character_id
    ↓
craft_job_id
    ↓
übergebene item_ids
    ↓
bezahltes Gold
    ↓
Coordinator-KI-Auftrag
```

Scheitert der externe KI-Auftrag endgültig, besitzt der Realm weiterhin alle notwendigen Informationen.

Der Realm kann dann selbstständig:

* den Crafting-Job ermitteln
* den betroffenen Charakter ermitteln
* die übergebenen Item-IDs ermitteln
* das gezahlte Gold ermitteln
* die Items über das Mail-System zurückgeben
* Gold zurückerstatten
* den Job kontrolliert beenden

Der Coordinator benötigt dafür keinerlei Datenbankzugriff.

---

## 8. Crafting-Jobs und Item-Zuordnung

Bei einem Crafting-Auftrag erstellt der Realm zunächst einen eigenen eindeutigen Jobdatensatz.

Beispiel:

```text
craft_job_id = 4711
character_id = 812
```

Wenn der Spieler Gegenstände oder Materialien an einen NPC übergibt, werden die konkreten Item-Instanz-IDs diesem Job zugeordnet.

Beispiel:

```text
craft_job_items

job_id 4711 → item_id 18441
job_id 4711 → item_id 18442
job_id 4711 → item_id 19107
```

Diese Zuordnung ist die maßgebliche Recovery-Information.

Die Recovery darf nicht ausschließlich davon abhängen, dass ein Item korrekt mit einem Status wie:

```text
RESERVED
```

markiert wurde.

Dadurch bleibt die Zuordnung auch dann nachvollziehbar, wenn beispielsweise nach einem Update ein Fehler in der Reservierungslogik auftritt.

Zusätzlich werden rückerstattbare Kosten wie Gold eindeutig mit dem Job verknüpft.

---

## 9. Rückerstattung über das Mail-System

Scheitert ein Crafting-Auftrag endgültig, werden abgegebene Gegenstände nicht direkt in das Charakterinventar zurückgelegt.

Stattdessen verwendet der Realm das Mail-System.

Dadurch funktioniert die Rückgabe auch dann, wenn:

* der Spieler offline ist
* das Inventar voll ist
* der Charakter sich gerade an einem anderen Ort befindet

Grundprinzip:

```text
Crafting-Job fehlgeschlagen
        ↓
Character-ID ermitteln
        ↓
zugehörige Item-IDs ermitteln
        ↓
Gold/Kosten ermitteln
        ↓
Mail mit Items erzeugen
        ↓
Gold zurückerstatten
        ↓
Job endgültig abschließen
```

Eine Rückerstattung darf nur einmal durchgeführt werden.

Entsprechende Zustände müssen verhindern, dass bei Recovery oder wiederholter Verarbeitung Items oder Gold dupliziert werden.

---

## 10. Realm-Isolation

Öffentlich getrennte Realms besitzen vollständig getrennte persistente Spiel- und Charakterzustände.

Beispiel:

```text
DE-1
→ realm_state_de1

DE-2
→ realm_state_de2
```

Eine Gildenstadt auf DE-1 existiert dadurch nicht automatisch auf DE-2.

Ebenso existiert ein Charakter aus:

```text
realm_state_de1
```

nicht automatisch in:

```text
realm_state_de2
```

Die Trennung umfasst damit sowohl:

* Weltzustand
* Charakterzustand
* Items
* Inventare
* Gilden
* Wirtschaft
* Crafting-Jobs
* Mail
* weitere persistente Realm-Daten

---

## 11. Realms als eigenständige Inhaltsversionen

Jeder Realm besitzt nicht nur seinen eigenen dynamischen Zustand, sondern auch seine eigene statische Inhaltsversion in `realm_state_<realm>`.

Dadurch sind verschiedene Realm-Versionen parallel möglich, beispielsweise:

```text
Live
Classic
Test
Event
```

Realm-Versionen können unabhängig voneinander aktualisiert und migriert werden. Ein Update einer Realm-Version läuft automatisiert über Wartungsmodus → Shutdown → Backup → Update → Migration → Healthcheck → Freigabe und erzwingt keinen Neustart anderer Realms (siehe `Deployment_Betriebsarchitektur.md`).

Ein Charakter eines Realms existiert auch hier ausschließlich in der Realm-Datenbank seines Realms.

---

## 12. Charaktertransfers

Da Charaktere direkt in der Realm-Datenbank gespeichert werden, bedeutet ein Realmtransfer technisch eine kontrollierte Migration von Charakterdaten zwischen zwei Realm-Datenbanken.

Ein solcher Transfer darf nicht durch direkte Cross-DB-Abhängigkeiten im normalen Spielcode entstehen.

Transfers werden als eigener kontrollierter Vorgang behandelt.

Dabei müssen beispielsweise berücksichtigt werden:

* Charakterdaten
* Item-Instanzen
* Inventar
* Ausrüstung
* persönliche Fortschritte
* Skills
* Queststände
* Freischaltungen
* realmgebundene Besitzverhältnisse
* nicht übertragbare Daten
* Fresh-Start-Regeln

Die genaue Transferlogik wird separat definiert.

---

## 13. Datenbankverbindungen und DB-Benutzer

Jeder Datenbankbereich erhält eine eigene Verbindungskonfiguration und einen eigenen technischen MariaDB-Benutzer.

### AUTH

```text
AUTH_DB_HOST=
AUTH_DB_PORT=3306
AUTH_DB_USER=
AUTH_DB_PASSWORD=
AUTH_DB_NAME=auth
```

Diese Verbindung gehört ausschließlich zum Auth/API-Service.

Webseite, Loginserver, Realmserver und Coordinator erhalten keine `AUTH_DB_*`-Konfiguration.

### REALM STATE

```text
REALM_STATE_DB_HOST=
REALM_STATE_DB_PORT=3306
REALM_STATE_DB_USER=
REALM_STATE_DB_PASSWORD=
REALM_STATE_DB_NAME=realm_state_de1
```

Eine separate:

```text
CHARACTER_DB_*
```

Konfiguration existiert nicht mehr.

---

## 14. DB-Benutzer und Rechte

Beispiel:

```text
andora_auth
→ ausschließlich auth

andora_realm_de1
→ ausschließlich realm_state_de1

andora_realm_de2
→ ausschließlich realm_state_de2
```

Ein Realm-Benutzer erhält keinen Zugriff auf die Datenbank eines anderen Realms.

Beispiel:

```text
andora_realm_de1
```

darf nicht auf:

```text
realm_state_de2
```

zugreifen.

Die statischen Weltdefinitionen liegen in derselben Realm-Datenbank und unterliegen damit denselben Benutzerrechten wie die Realm-Daten.

Änderungen an statischen Weltdaten erfolgen kontrolliert über Realm-Updates und:

* Migrationen
* Deployment
* Content-Updates

---

## 15. Keine Sammelverbindung

Es gibt keine allgemeine DB-Verbindung, die automatisch auf mehrere fachlich getrennte Datenbanken zugreifen kann.

Insbesondere gibt es keine Legacy-Sammelkonfiguration wie:

```text
DB_*
WORLD_DB_*
```

die stillschweigend `auth` oder mehrere Realm-Datenbanken miteinander verbindet.

Fehlt eine notwendige Datenbankkonfiguration, soll der betreffende Dienst mit einer klaren Fehlermeldung abbrechen.

Er darf nicht automatisch auf eine andere Datenbank ausweichen.

---

## 16. Coordinator besitzt keine Datenbankrechte

Der Coordinator ist ausschließlich die zentrale Schnittstelle für KI-/Ollama-Anfragen.

Er besitzt keinerlei direkten Datenbankzugriff.

Insbesondere erhält er keine Credentials für:

* `auth`
* `realm_state_<realm>`

Der Coordinator verwaltet seine eigenen KI-Jobs ausschließlich über seine dafür vorgesehenen lokalen Queue- und Job-Dateien.

Er darf:

* KI-Jobs entgegennehmen
* Jobs priorisieren
* Ollama ansprechen
* Eingaben prüfen
* Antworten prüfen
* Korrekturversuche durchführen
* Fehlerstatus an Realmserver zurückgeben

Er darf nicht:

* Charakterdaten verändern
* Items erzeugen oder löschen
* Gold verändern
* Realm-Jobs abschließen
* Crafting-Datenbankeinträge verändern
* Mail erzeugen
* Recovery direkt durchführen

Grundsatz:

> Der Coordinator verarbeitet KI.
> Der Realm verwaltet das Spiel.

---

## 17. Mehrere technische Realm-Prozesse

Ein Realm kann später aus mehreren technischen Serverprozessen bestehen.

Diese Prozesse gehören weiterhin zum selben Realm und dürfen denselben Realm-State verwenden.

Beispiel:

```text
Realm DE-1

Realm-Prozess 1 ─┐
Realm-Prozess 2 ─┼── realm_state_de1
Realm-Prozess 3 ─┘
```

Das erzeugt keine neue Welt und keine getrennten Charakterdatenbanken.

Ein neuer öffentlicher Realm benötigt dagegen eine eigene persistente Realm-Datenbank.

---

## 18. SQL-Dateibaum

Die bisherige zentrale `schema.sql` wird nicht dauerhaft weiterverwendet.

Der Datenbankbaum lautet:

```text
src/api/
└── db/
    └── auth/
        ├── auth.sql
        └── migrations/

src/realm/
└── db/
    ├── realm_state/
    │   ├── realm_state.sql
    │   ├── migrations/
    │   └── seed/              (statische Realm-Definitionen / Inhaltsversion)
```

Der bisherige Bereich:

```text
src/realm/db/character/
```

entfällt.

Der bisherige Bereich:

```text
src/realm/db/world_data/
```

entfällt ebenfalls; dessen Inhalte gehören künftig als statische Definitionen zu:

```text
src/realm/db/realm_state/
```

Charaktertabellen und deren Migrationen gehören künftig nach:

```text
src/realm/db/realm_state/
```

Beispiele:

```text
src/api/db/auth/migrations/
├── 001_accounts.sql
├── 002_sessions.sql
└── 003_realms.sql

src/realm/db/realm_state/migrations/
├── 001_characters.sql
├── 002_inventory.sql
├── 003_skills.sql
├── 004_guilds.sql
├── 005_world_state.sql
├── 006_npc_state.sql
├── 007_item_definitions.sql
├── 008_npc_definitions.sql
├── 009_monster_definitions.sql
├── 010_crafting_jobs.sql
└── 011_mail.sql
```

Die tatsächliche Nummerierung richtet sich nach dem vorhandenen Migrationsstand.

Bereits angewendete Migrationen werden nicht nachträglich umnummeriert oder verändert.

---

## 19. Tabellen-Ownership

Jede Tabelle gehört genau zu einem fachlichen Datenbankbereich.

Die folgende Zuordnung ist keine vollständige Liste zukünftiger Tabellen.

```text
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
→ realm_state_<realm>

character_inventory
→ realm_state_<realm>

character_equipment
→ realm_state_<realm>

item_instances
→ realm_state_<realm>

character_skills
→ realm_state_<realm>

item_definitions
→ realm_state_<realm>

monster_definitions
→ realm_state_<realm>

npc_definitions
→ realm_state_<realm>

character_quests
→ realm_state_<realm>

craft_jobs
→ realm_state_<realm>

craft_job_items
→ realm_state_<realm>

mail
→ realm_state_<realm>

mail_attachments
→ realm_state_<realm>

guilds
→ realm_state_<realm>

guild_members
→ realm_state_<realm>

auctions
→ realm_state_<realm>

guild_cities
→ realm_state_<realm>
```

Eine Tabelle wird nicht aus Bequemlichkeit in eine andere Datenbank gelegt.

Wenn ein Dienst Informationen aus einem Bereich benötigt, auf den er keinen direkten Zugriff haben soll, erfolgt dies über eine definierte Server-/API-Schnittstelle.

> Datenbankzugriff folgt der Verantwortung des Dienstes und nicht der Bequemlichkeit des Codes.

---

## 20. Keine Cross-DB-Abhängigkeiten ohne Prüfung

Neue Systeme müssen vor dem Anlegen einer Tabelle festlegen:

1. Gehören die Daten zur Account-/Sicherheitsidentität?
2. Sind es statische Definitionen der Inhaltsversion eines konkreten Realms?
3. Gehören sie zu einem konkreten Realm?
4. Gehören sie zu einem Charakter dieses Realms?
5. Müssen sie gemeinsam mit anderen Realm-Daten transaktional verarbeitet werden?

Danach wird entschieden:

```text
Account/Sicherheit
→ auth

statische Definition oder Charakter/Spielzustand eines Realms
→ realm_state_<realm>
```

Charaktergebundene Daten bilden keinen eigenen globalen Datenbankbereich mehr.

---

## 21. Migrationen und db_version

Jede Andora-Datenbank besitzt eine eigene Migrationshistorie und eine eigene `db_version`-Tabelle.

Damit existieren Migrationshistorien für:

```text
auth
realm_state_<realm>
```

Eine separate:

```text
character
```

Migrationshistorie existiert nicht mehr.

Beispiel:

```sql
CREATE TABLE IF NOT EXISTS db_version (
    version INT NOT NULL PRIMARY KEY,
    migration VARCHAR(255) NOT NULL,
    applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### Automatische Startprüfung

Beim Start prüft der für die jeweilige Datenbank zuständige Dienst den installierten Schema-Stand.

```text
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

Schlägt eine notwendige Migration fehl, wird der Start des betroffenen Dienstes abgebrochen und der Fehler klar geloggt.

### Zuständigkeit für auth

Da ausschließlich der Auth/API-Service direkten Zugriff auf `auth` besitzt, ist ausschließlich dieser Dienst für Prüfung und Anwendung der `auth`-Migrationen verantwortlich.

Webseite, Loginserver, Realmserver und Coordinator führen keine Migrationen auf `auth` aus.

### Zuständigkeit für realm_state

Der zuständige Realmserver verwaltet die Migrationen seiner Realm-Datenbank.

Dies umfasst künftig auch Charakter-, Inventar-, Item-, Crafting- und Mailtabellen.

### Zuständigkeit für statische Realm-Definitionen

Die Migrationen der statischen Definitionen eines Realms erfolgen im Rahmen des automatisierten Realm-Updates (Wartungsmodus → Shutdown → Backup → Update → Migration → Healthcheck → Freigabe, siehe `Deployment_Betriebsarchitektur.md`). Dabei wird nur die jeweils betroffene Realm-Datenbank migriert; andere Realms bleiben unberührt.

---

## 22. Unveränderliche Migrationen

Bereits erfolgreich angewendete Migrationen werden niemals nachträglich verändert.

Muss ein bereits bestehendes Schema später geändert werden, wird eine neue Migration erstellt.

Beispiel:

```text
005_encrypt_account_email.sql
```

Die Integration der früheren Character-Struktur in `realm_state` muss ebenfalls über neue Migrationen erfolgen, sofern entsprechende Tabellen bereits tatsächlich angelegt oder produktiv verwendet wurden.

Bestehende angewendete Migrationen werden nicht einfach umgeschrieben.

---

## 23. Regeln für Qwen/OpenCode

Qwen/OpenCode darf SQL-Migrationen erstellen und während der Entwicklung mit den dafür vorgesehenen eingeschränkten DB-Zugangsdaten anwenden.

Dabei gelten folgende Regeln:

* keine MariaDB-Admin-Credentials verwenden
* keine DB-Benutzer selbst anlegen
* keine Rechte selbst verändern
* keine fremden Datenbanken verändern
* keine Tabellen ohne Zuordnung zu einem DB-Bereich erstellen
* keine neue zentrale `schema.sql` aufbauen
* bereits angewendete Migrationen nicht nachträglich verändern
* Migrationen nur mit dem vorgesehenen technischen DB-Benutzer anwenden
* keine separate Character-Datenbank neu einführen
* Character-Tabellen gehören zum jeweiligen `realm_state_<realm>`
* Coordinator erhält keine Datenbankverbindung

Produktive Migrationen werden später gesondert geregelt.

---

## 24. Sicherheitsgrenzen

### Auth

Nur der Auth/API-Service besitzt direkten Zugriff auf sensible Accountdaten und auf die `auth`-Datenbank.

Webseite, Loginserver und Realmserver greifen für Auth-Funktionen ausschließlich über die Auth-API zu und besitzen eigene Service-Credentials mit minimal notwendigen Berechtigungen.

Der Coordinator besitzt ebenfalls keinen Zugriff auf `auth`.

### Realm-Definitionen

Die statischen Weltdefinitionen liegen realmbezogen in `realm_state_<realm>` und enthalten keine Account-Geheimnisse.

### Realm State

`realm_state_<realm>` enthält:

* Charakterdaten
* Items
* Inventare
* persönliche Fortschritte
* Gilden
* Wirtschaft
* Crafting-Jobs
* Mail
* Weltzustand
* weitere Daten dieses konkreten Realms

Ein Realmserver erhält ausschließlich Zugriff auf die für ihn vorgesehenen Realm-Datenbanken und notwendigen globalen Lesedaten.

### Coordinator

Der Coordinator besitzt keinerlei Datenbankrechte.

Dadurch führt eine Kompromittierung des Coordinators nicht automatisch zu direktem Datenbankzugriff auf:

* Accounts
* Passwörter
* E-Mail-Adressen
* Charaktere
* Items
* Gold
* Realmzustände

Der Realm validiert weiterhin alle KI-Ergebnisse, bevor daraus spielmechanische Aktionen entstehen.

---

## 25. Backup- und Realm-Grenze

Da Charakter- und Weltzustände gemeinsam in `realm_state_<realm>` gespeichert werden, bildet die Realm-Datenbank eine natürliche Backup-Einheit.

Ein Backup von:

```text
realm_state_de1
```

enthält damit sowohl:

* den Zustand der Welt von DE-1
* die Charaktere von DE-1
* deren Items und Inventare
* deren Crafting-Jobs
* deren Mail
* weitere persistente Realm-Daten

Dadurch wird verhindert, dass Character-Daten und Realmzustand aus unterschiedlichen Backup-Zeitpunkten wiederhergestellt werden und dadurch Inkonsistenzen entstehen.

`auth` und `realm_state_<realm>` bleiben unabhängige Datenbankbereiche mit eigenen Backup- und Migrationsanforderungen.

---

## 26. Leitsätze

> Auth weiß, wer du bist.

> Jede Realm-Inhaltsversion weiß, was in diesem Realm existieren kann.

> Realm State weiß, wer und was in diesem Realm existiert und was dort passiert ist.

> Jeder Realm besitzt seinen vollständigen statischen und dynamischen Datenstand selbst.

> Ein Charakter gehört vollständig zu seinem Realm.

> Charakterdaten und Realmzustand werden nicht künstlich auf getrennte Datenbanken verteilt.

> Jeder Realmserver bekommt nur die Datenbankrechte, die er tatsächlich benötigt.

> Ein Realmserver erhält keinen Zugriff auf die Realm-Datenbank eines anderen Realms.

> Eine Datenbank ist kein Ablageort für beliebige Tabellen, sondern besitzt eine klar definierte Verantwortung.

> Nur der Auth/API-Service besitzt direkten Zugriff auf die Auth-Datenbank.

> Webseite, Loginserver und Realmserver verwenden für Auth-Funktionen ausschließlich die Auth-API.

> Der Coordinator besitzt keine Datenbankrechte.

> Der Coordinator verarbeitet KI. Der Realm verwaltet das Spiel.

> Jede Andora-Datenbank besitzt ihre eigene `db_version`-Historie.

> Bereits angewendete Migrationen werden niemals nachträglich verändert.
