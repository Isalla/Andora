# Login-, Account- und Realm-Architektur

## 1. Ziel

Dieses Dokument beschreibt die grundlegende Architektur für:

* Accounts und Authentifizierung
* Login
* Realm-Auswahl
* World-Server
* Character-Daten
* World-Daten
* persistente Realm-Zustände
* spätere Charaktertransfers
* Fresh-Start-Regeln
* zukünftige Zugriffe einer Webseite

Die Architektur soll von Beginn an mehrere Realms und mehrere technische World-Server ermöglichen, ohne spätere Erweiterungen unnötig zu erschweren.

---

# 2. Grundstruktur

Andora trennt folgende Bereiche voneinander:

```text
Account/Auth
    │
    ├── Account-Daten
    ├── Authentifizierung
    ├── Sessions
    └── Realm-Liste
    │
    ▼
Character-System
    │
    ├── Charaktere
    └── persistente Charakterdaten
    │
    ▼
Realm
    │
    ├── eigener Weltzustand
    └── ein oder mehrere World-Server
```

Dabei gilt:

> **Der Account gehört keinem Realm.**

> **Ein Realm ist eine eigenständige persistente Welt.**

> **Ein World-Server führt einen Realm technisch aus.**

---

# 3. Account/Auth-Service

Der Account/Auth-Service ist die zentrale Stelle für Accounts und Authentifizierung.

Zu seinen grundlegenden Aufgaben gehören:

* Account authentifizieren
* Accountstatus prüfen
* Sessions verwalten
* verfügbare Realms bereitstellen
* World-Server authentifizieren
* World-Server-Status verwalten
* später sichere Übergabe an einen Realm/World-Server

Der Spielclient greift nicht direkt auf die Account-Datenbank zu.

Auch eine spätere Webseite erhält keinen direkten Datenbankzugriff.

---

# 4. Account-Datenbank

Die Account-Datenbank enthält ausschließlich accountbezogene Informationen.

Beispiel:

```text
accounts
├── account_id
├── username
├── password_hash
├── email_encrypted
├── email_lookup_hash
├── status
├── created_at
└── weitere Account-Einstellungen
```

## Passwörter

Passwörter werden niemals im Klartext gespeichert und auch nicht reversibel verschlüsselt.

Sie werden mit einem dafür geeigneten Passwort-Hashverfahren gespeichert.

Vorgesehen ist beispielsweise:

```text
Argon2id
```

Damit kann das ursprüngliche Passwort nicht aus der Datenbank entschlüsselt werden.

## E-Mail-Adressen

E-Mail-Adressen können verschlüsselt gespeichert werden.

```text
email_encrypted
```

Der dafür verwendete Schlüssel wird getrennt von der Datenbank aufbewahrt.

Optional kann zusätzlich ein nicht reversibler Lookup-Hash gespeichert werden:

```text
email_lookup_hash
```

Dieser ermöglicht beispielsweise:

* Prüfung auf bereits verwendete E-Mail-Adressen
* Suche nach einer E-Mail-Adresse

ohne dafür sämtliche gespeicherten E-Mail-Adressen entschlüsseln zu müssen.

## Sicherheitsziel

> **Ein reiner Diebstahl der Account-Datenbank soll keine direkt lesbaren Passwörter oder E-Mail-Adressen offenlegen.**

---

# 5. API-Schicht

Externe Komponenten erhalten keinen direkten Zugriff auf die Account-Datenbank.

Stattdessen erfolgt der Zugriff über kontrollierte Schnittstellen.

```text
Spielclient ──────────┐
                      │
Webseite ─────────────┼──► Account/Auth-Service ──► Account-DB
                      │
World-Server ─────────┘
```

Dabei können unterschiedliche API-Bereiche und Berechtigungen verwendet werden.

Beispielsweise:

```text
Account/Auth-Service

├── Client/Login API
│   ├── Login
│   ├── Session
│   ├── Realm-Liste
│   └── Realm-Auswahl
│
├── Internal World API
│   ├── World-Server-Authentifizierung
│   ├── Registrierung
│   ├── Heartbeat
│   └── Session-/Tokenprüfung
│
└── spätere Web API
    ├── Accountverwaltung
    └── ausdrücklich freigegebene Accountfunktionen
```

Die Webseite wird erst später entwickelt.

Die API-Grenze wird trotzdem bereits bei der Serverarchitektur berücksichtigt.

---

# 6. World-Server-Authentifizierung

Ein World-Server darf sich nicht selbstständig als offizieller Andora-Server eintragen.

Jeder erlaubte World-Server muss vorher im Account/Auth-System registriert worden sein.

Beispiel:

```text
world_servers

world_server_id
realm_id
name
credential
enabled
host
port
version
last_heartbeat
current_players
max_players
```

Der World-Server authentifiziert sich beim Start.

```text
World-Server startet
        │
        ▼
Authentifizierung beim Account/Auth-Service
        │
        ▼
Server-ID gültig?
        │
        ▼
Credential gültig?
        │
        ▼
Server aktiviert?
        │
        ├── Nein → ablehnen
        │
        └── Ja
             │
             ▼
        Server registrieren
```

Ein fremder Server kann sich dadurch nicht einfach selbst in die offizielle Realm-/Serverstruktur eintragen.

Server-Credentials werden nicht unnötig im Klartext in der Datenbank gespeichert.

---

# 7. Heartbeat

Ein registrierter World-Server meldet regelmäßig seinen Zustand beim Account/Auth-Service.

Beispielsweise:

```text
world_server_id
status
current_players
max_players
version
last_heartbeat
```

Bleibt der Heartbeat über einen definierten Zeitraum aus, gilt der World-Server als nicht verfügbar.

Neue Spieler werden dann nicht mehr dorthin vermittelt.

---

# 8. Realm und World-Server

Realm und World-Server sind unterschiedliche Dinge.

## Realm

Ein Realm ist eine eigenständige persistente Andora-Welt.

Er besitzt unter anderem:

* eigenen Weltfortschritt
* eigene Wirtschaft
* eigene Gilden
* eigene Gildenstädte
* eigene politische Verhältnisse
* eigene Herrschaft
* eigene persistente Weltzustände
* eigene Expansion-/Progressionszustände
* eigene Transferregeln

Beispiele:

```text
Andora DE-1
Andora DE-2
Andora EN-1
```

Diese können sich spielerisch unterschiedlich entwickeln.

Beispielsweise könnte `DE-1` bereits weit in EXP1 fortgeschritten sein, während ein später gestarteter Realm noch einen wesentlich niedrigeren Weltfortschritt besitzt.

## World-Server

Ein World-Server ist dagegen eine technische Instanz, welche einen Realm ausführt.

Ein Realm kann später bei Bedarf von mehreren technischen World-Prozessen getragen werden.

```text
Realm DE-1
    │
    ├── World-Prozess A
    ├── World-Prozess B
    └── gemeinsamer Realm-Zustand
```

Die technische Skalierung eines Realms muss für den Spieler nicht sichtbar sein.

---

# 9. Freie Realm-Auswahl

Nach erfolgreicher Anmeldung erhält der Spieler die verfügbaren Realms.

Sprache, Standort und Latenz dienen ausschließlich der Information.

Beispiel:

```text
Andora DE-1
Sprache: Deutsch
Region: Europa
Status: Online
Ping: 24 ms

Andora EN-1
Sprache: Englisch
Region: Europa
Status: Online
Ping: 31 ms
```

Es gibt kein Geoblocking anhand dieser Angaben.

Ein Spieler darf einen verfügbaren Realm unabhängig von:

* Land
* Sprache
* Standort
* Latenz

auswählen.

Wenn einem Spieler eine höhere Latenz egal ist, darf er trotzdem den entsprechenden Realm verwenden.

> **Sprache, Region und Latenz informieren den Spieler. Sie entscheiden nicht für ihn.**

---

# 10. Character-Daten

Persistente Charakterdaten werden von den eigentlichen Realm-Zuständen getrennt.

Dafür kann eine zentrale Character-Datenbank bzw. ein eigener Character-Service verwendet werden.

Beispiel:

```text
Character-DB

characters
inventory
equipment
skills
quest_progress
currency
reputation
appearance
...
```

Ein Charakter besitzt eine eindeutige `character_id` und gehört einem Account.

Die endgültige technische Aufteilung einzelner Character-Systeme wird erst beim jeweiligen System festgelegt.

---

# 11. Charakter und Realm

Charakterdaten werden nicht unnötig direkt in die World-State-Datenbank eingebettet.

Dadurch bleibt ein späterer Realmtransfer technisch möglich.

Ein Realm besitzt jedoch seinen eigenen persistenten Weltzustand.

Ein Charakter kann deshalb nicht beliebig zwischen völlig unterschiedlich entwickelten Realms springen.

Beispiel:

```text
DE-1
Weltfortschritt: EXP1 / Ebene 42

DE-5
Weltfortschritt: Ebene 19
```

Ein hoch entwickelter Charakter aus `DE-1` darf nicht automatisch auf einen frisch gestarteten `DE-5` gelangen.

Der Wechsel zwischen eigenständigen Realms ist deshalb ein kontrollierter **Charaktertransfer**.

---

# 12. Character-Transfer

Ein Charaktertransfer bedeutet nicht zwingend, dass sämtliche Character-Daten physisch zwischen zwei komplett getrennten Datenbanken kopiert werden müssen.

Da persistente Character-Daten zentral verwaltet werden können, kann der Transfer vor allem die kontrollierte Realm-Zuordnung des Charakters verändern.

Dabei muss später definiert werden, welche Daten übertragbar sind.

Persönliche Daten können beispielsweise transferierbar sein:

* Level
* Klasse
* Skills
* Ausrüstung
* Inventar
* persönliche Questfortschritte

Realmgebundene Zustände benötigen eigene Regeln.

Dazu gehören beispielsweise:

* Gildenmitgliedschaft
* Gildenstadt
* politische Herrschaft
* laufende Auktionen
* realmbezogene Ranglisten
* realmbezogene Weltzustände

Die genauen Transferregeln werden erst definiert, wenn die betroffenen Systeme implementiert werden.

---

# 13. Fresh-Start-Sperre

Neue Realms erhalten eine Fresh-Start-Phase.

Standard:

```text
14 Tage
```

Während dieser Zeit dürfen keine bestehenden Charaktere aus älteren Realms auf den neuen Realm übertragen werden.

Wer dort spielen möchte, erstellt einen neuen Charakter.

Beispiel:

```text
DE-5 startet
      │
      ▼
Fresh Start: 14 Tage
      │
      ├── neuer Charakter → erlaubt
      │
      └── bestehender Charaktertransfer → gesperrt
```

Dadurch wird verhindert, dass hoch entwickelte Charaktere unmittelbar die neue Wirtschaft und Progression dominieren.

Die Sperre schützt ebenfalls die soziale und politische Entwicklung des neuen Realms.

Insbesondere sollen etablierte Gilden nicht unmittelbar mit bestehenden Charakteren auf einen neuen Realm wechseln und dort innerhalb kürzester Zeit die Herrschaft übernehmen können.

> **Ein neuer Realm soll tatsächlich eine neue Welt sein und neuen Spielern sowie neuen Gilden eine echte Startchance geben.**

Die Dauer der Fresh-Start-Phase soll konfigurierbar sein.

Beispielsweise:

```text
realm_id
created_at
fresh_start_until
transfer_policy
```

Dadurch können später auch Realms mit:

* 14 Tagen Transfersperre
* 30 Tagen Transfersperre
* dauerhaft deaktivierten Transfers

realisiert werden, ohne die Serverlogik umzuschreiben.

---

# 14. Statische World-Daten

Grundlegende Definitionen der Spielwelt werden von den individuellen Realm-Zuständen getrennt.

Eine zentrale World-Data-Schicht kann beispielsweise enthalten:

```text
world_data

├── Monsterdefinitionen
├── Itemdefinitionen
├── Loottabellen
├── NPC-Grunddefinitionen
├── Regionen
├── Dungeons
├── Ressourcen
├── Spawnregeln
└── weitere grundlegende Weltdaten
```

Diese Daten beschreiben die grundlegenden Regeln und Inhalte von Andora.

> **World-Data beschreibt, was in Andora existieren kann.**

---

# 15. Persistenter Realm-State

Jeder Realm besitzt dagegen seinen eigenen tatsächlichen Weltzustand.

Beispielsweise:

```text
realm_state

├── realm_id
├── Weltfortschritt
├── Gilden
├── Gildenstädte
├── Herrschaft
├── politische Zustände
├── Wirtschaftszustände
├── persistente NPC-Zustände
├── persistente Weltveränderungen
├── Expansion-Fortschritt
└── weitere dynamische Zustände
```

Damit können sich zwei Realms trotz identischer grundlegender World-Daten vollkommen unterschiedlich entwickeln.

Beispiel:

```text
world_data
      │
      ├───────────────┐
      ▼               ▼
Realm DE-1         Realm DE-5
EXP1 weit          frisch gestartet
alte Gilden        neue Gilden
Gildenstädte       noch keine Städte
entwickelte        junge
Wirtschaft         Wirtschaft
```

> **World-Data sagt, was existieren kann.**

> **Realm-State sagt, was in dieser konkreten Welt tatsächlich passiert ist.**

---

# 16. Charaktererstellung

Nach der Realm-Auswahl kann der Spieler:

* einen vorhandenen, für diesen Realm gültigen Charakter verwenden
* einen neuen Charakter erstellen

Bei einem Fresh-Start-Realm kann die Verwendung bzw. Übertragung älterer Charaktere entsprechend der Realmregeln gesperrt sein.

Die eigentliche Charaktererstellung und die clientseitigen Character-Sets sind separat dokumentiert.

---

# 17. Serverliste und Benutzerfreundlichkeit

Die Server-/Realmliste soll dem Spieler relevante Informationen übersichtlich darstellen.

Dazu können gehören:

```text
Name
Sprache
Region
Status
Spielerzahl
Maximalspieler
Latenz
Fresh-Start-Status
Transferstatus
```

Falls Charaktere realmgebunden geführt werden, kann zusätzlich angezeigt werden, wie viele Charaktere der Account dort besitzt.

Realms mit vorhandenen Charakteren werden vom Client priorisiert und am Anfang der Liste dargestellt.

Die Sortierung erfolgt clientseitig.

Der Server liefert lediglich die notwendigen sachlichen Informationen.

Die endgültige Realm-Auswahl trifft immer der Spieler.

---

# 18. Spätere Webseite

Eine öffentliche Webseite ist kein Bestandteil der ersten Entwicklungsphase.

Sie wird erst entwickelt, wenn Andora spielerisch und visuell weit genug definiert ist, dass das Spiel sinnvoll präsentiert werden kann.

Die Serverarchitektur wird jedoch bereits darauf vorbereitet.

Eine spätere Webseite:

```text
Webseite
   │
   ▼
Web API
   │
   ▼
Account/Auth-Service
```

Die Webseite erhält keine direkten Zugangsdaten zur Account-Datenbank.

Bei einer Kompromittierung des Webservers sollen dadurch keine unmittelbaren Datenbank-Zugangsdaten für die Account-Datenbank verfügbar sein.

---

# 19. Sicherheitsprinzip

Andora folgt dem Prinzip der minimal notwendigen Berechtigungen.

> **Jeder Dienst erhält nur Zugriff auf die Daten und Funktionen, die er tatsächlich benötigt.**

Daraus folgen unter anderem:

* keine Klartextpasswörter
* keine direkt lesbaren E-Mail-Adressen in der Datenbank
* Schlüssel getrennt von verschlüsselten Daten
* kein direkter Account-DB-Zugriff durch die Webseite
* kein direkter Account-DB-Zugriff durch den Spielclient
* authentifizierte World-Server
* keine selbstständige Aufnahme fremder World-Server
* getrennte Berechtigungen für unterschiedliche APIs
* kontrollierte Character-Transfers
* serverseitige Prüfung von Fresh-Start-Regeln

---

# 20. Grundlegender Login-Ablauf

Der spätere grundlegende Ablauf sieht damit folgendermaßen aus:

```text
Client startet
      │
      ▼
Account/Auth-Service
      │
      ▼
Login
      │
      ▼
Account authentifiziert
      │
      ▼
Realm-Liste abrufen
      │
      ▼
Client zeigt/sortiert Realms
      │
      ▼
Spieler wählt Realm
      │
      ▼
Charakter auswählen / erstellen
      │
      ▼
Realmregeln prüfen
      │
      ├── Fresh-Start
      ├── Transferberechtigung
      └── Character-Zuordnung
      │
      ▼
sichere Übergabe
      │
      ▼
World-Server des Realms
      │
      ▼
Charakter betritt Andora
```

---

# 21. Architektur-Leitsätze

> **Der Account gehört keinem Realm.**

> **Ein Realm ist eine eigenständige persistente Welt.**

> **World-Data beschreibt, was existieren kann. Realm-State beschreibt, was tatsächlich passiert ist.**

> **World-Server führen einen Realm technisch aus; sie definieren nicht dessen dauerhafte Identität.**

> **Neue Realms beginnen als echte neue Welten und können durch eine Fresh-Start-Sperre vor dem unmittelbaren Import alter Machtstrukturen geschützt werden.**

> **Charakterdaten und Realm-Zustand werden so getrennt, dass spätere kontrollierte Charaktertransfers möglich bleiben.**

> **Sprache, Region und Latenz informieren den Spieler – die Wahl des Realms trifft der Spieler selbst.**

> **Webseite, Client und World-Server greifen ausschließlich über dafür vorgesehene Schnittstellen auf Account-Funktionen zu.**

# Schutz sensibler Daten im Arbeitsspeicher

## Grundprinzip

Verschlüsselung schützt Daten im Ruhezustand, beispielsweise in der Datenbank oder in Backups.

Sobald ein laufender Dienst Daten tatsächlich verwenden muss, können diese kurzfristig entschlüsselt im Arbeitsspeicher vorhanden sein.

Daher gilt:

> Sensible Daten dürfen nur in den Prozessen und nur für die Dauer vorhanden sein, in der sie tatsächlich benötigt werden.

---

## Passwörter

Passwörter werden nicht dauerhaft im Klartext gespeichert.

Beim Login existiert das eingegebene Passwort nur kurzzeitig im Arbeitsspeicher, um es gegen den gespeicherten Passwort-Hash zu prüfen.

Danach wird es nicht weiter benötigt.

Der Account/Auth-Service gibt Passwörter niemals an andere Dienste weiter.

Insbesondere erhalten World-Server niemals:

- Klartextpasswörter
- Passwort-Hashes
- Passwort-Salts
- Passwort-Reset-Daten

---

## E-Mail-Adressen

E-Mail-Adressen werden verschlüsselt gespeichert.

Die entschlüsselte E-Mail-Adresse wird nur dann erzeugt, wenn eine Funktion sie tatsächlich benötigt, beispielsweise:

- Accountverwaltung
- Passwort-Reset
- E-Mail-Änderung
- Versand einer notwendigen Account-Nachricht

World-Server benötigen keine E-Mail-Adressen und bekommen diese daher niemals übertragen.

---

## Trennung der Dienste

Die einzelnen Dienste erhalten nur die Daten, die sie benötigen.

Beispiel:

Account/Auth-Service:
- Benutzername
- Passwort-Hash
- verschlüsselte E-Mail
- Accountstatus
- Sessions
- Authentifizierungsdaten

Character-Service:
- account_id
- character_id
- Charakterdaten

World-Server:
- account_id
- character_id
- gültige World-Session
- notwendige Spielberechtigungen

World-Server erhalten keine sensiblen Accountdaten.

---

## World-Handoff

Beim Wechsel vom Login-/Account-Service zum World-Server werden keine sensiblen Accountdaten übertragen.

Der World-Server erhält beispielsweise nur:

account_id
character_id
handoff_token
session_id
permissions

Der World-Server prüft, ob der Handoff gültig ist.

Danach wird der Handoff-Token ungültig bzw. läuft nach kurzer Zeit automatisch ab.

---

## Schutz bei kompromittierten World-Servern

Ein kompromittierter World-Server soll nicht automatisch einen vollständigen Account-Datenverlust ermöglichen.

Da dort keine Passwörter, Passwort-Hashes, E-Mail-Adressen oder Account-Verschlüsselungsschlüssel benötigt werden, sollen diese Daten dort auch niemals vorhanden sein.

Damit gilt:

> Ein World-Server kennt den Spieler, aber nicht seine sensiblen Accountdaten.

---

## Schutz bei kompromittiertem Webserver

Eine spätere Webseite enthält keine Zugangsdaten zur Account-Datenbank und keinen dauerhaften Schlüssel zur Entschlüsselung aller Accountdaten.

Sie kommuniziert ausschließlich über die dafür vorgesehene API.

Die Webseite bekommt nur die Daten zurück, die für die jeweilige Funktion notwendig sind.

---

## Schlüssel

Schlüssel für verschlüsselte Accountdaten werden getrennt von der eigentlichen Account-Datenbank gespeichert.

Sie dürfen nicht:

- gemeinsam mit einem Datenbank-Backup gespeichert werden
- in Git eingecheckt werden
- an World-Server verteilt werden
- an den Spielclient übertragen werden
- unnötig an den Webserver weitergegeben werden

Eine spätere Schlüsselrotation muss möglich sein.

---

## Sessions und Tokens

Authentifizierungs- und Übergabe-Tokens sollen:

- zufällig und nicht vorhersagbar sein
- nur begrenzte Lebensdauer besitzen
- widerrufbar sein
- möglichst nur für einen bestimmten Zweck gelten
- nach Verwendung ungültig werden können

Ein World-Handoff-Token ist beispielsweise nur für den Übergang zu einem bestimmten World-Server oder Realm gültig.

---

## RAM-Dump-Szenario

Andora geht davon aus, dass ein Angreifer bei vollständiger Kontrolle über einen laufenden Server möglicherweise auch dessen Arbeitsspeicher auslesen kann.

Eine vollständige Verhinderung sensibler Klartextdaten im RAM ist nicht möglich, wenn ein Dienst diese Daten tatsächlich verarbeiten muss.

Das Sicherheitsziel lautet deshalb:

> Ein kompromittierter Prozess soll nur die sensiblen Daten preisgeben können, die dieser Prozess tatsächlich benötigt.

Ein RAM-Dump eines World-Servers darf daher beispielsweise keine komplette Accountdatenbank, E-Mail-Liste oder Passwort-Hashes enthalten.

Ein RAM-Dump des Account/Auth-Servers kann dagegen aktuell verwendete sensible Daten enthalten und muss deshalb als besonders kritischer Sicherheitsvorfall behandelt werden.

---

## Architekturregel

> Datenminimierung gilt nicht nur für Datenbanken, sondern auch für Netzwerkverkehr und Arbeitsspeicher.

> Kein Dienst erhält sensible Informationen nur „für den Fall, dass er sie vielleicht irgendwann braucht“.

> Je weniger sensible Daten ein Prozess kennt, desto kleiner ist der mögliche Schaden bei einer Kompromittierung.