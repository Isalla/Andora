# Login-, Account- und Realm-Architektur

## 1. Ziel

Dieses Dokument beschreibt die grundlegende Architektur für:

* Accounts und Authentifizierung
* Login
* Realm-Auswahl
* Realm-Server (kein separater Worldserver-Dienst; Zielkette Auth/API → Login → Realm)
* Character-Daten
* World-Daten
* persistente Realm-Zustände
* spätere Charaktertransfers
* Fresh-Start-Regeln
* Realm-Rulesets (konfigurierbare Regelvarianten je Realm)
* zukünftige Zugriffe einer Webseite

Die Architektur soll von Beginn an mehrere Realms ermöglichen, ohne spätere Erweiterungen unnötig zu erschweren. Ein Realm wird vom zuständigen Realm-Server ausgeführt (ggf. mehrere technische Realm-Prozesse mit gemeinsamem Realm-Zustand); einen separaten Worldserver-Dienst in der Kette gibt es nicht.

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
    └── Realm-Server (führt den Realm aus; ggf. mehrere
        technische Prozesse mit gemeinsamem Realm-Zustand)
```

Dabei gilt:

> **Der Account gehört keinem Realm.**

> **Ein Realm ist eine eigenständige persistente Welt.**

> **Der Realm-Server führt seinen Realm technisch aus; es gibt keinen separaten Worldserver-Dienst.**

---

# 3. Account/Auth-Service

Der Account/Auth-Service ist die zentrale Stelle für Accounts und Authentifizierung.

Zu seinen grundlegenden Aufgaben gehören:

* Account authentifizieren
* Accountstatus prüfen
* Sessions verwalten
* verfügbare Realms bereitstellen
* Handoff-Tokens für die sichere Übergabe an einen Realm verwalten
* (LEGACY: World-Server-Authentifizierung/-Status — kein separater Worldserver mehr)

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
Login-Service ────────┼──► Account/Auth-Service ──► Account-DB
                      │
Webseite ─────────────┼──► Account/Auth-Service ──► Account-DB
                      │
Realmserver ──────────┘
```

Zielkette für den Spieleinstieg: `Auth/API → Login → Realm` (der
Login-Service ist in `src/login` implementiert; es gibt keinen
separaten Worldserver-Dienst).

Dabei können unterschiedliche API-Bereiche und Berechtigungen verwendet werden.

Beispielsweise:

```text
Account/Auth-Service

├── Client/Login API (Login-Service)
│   ├── Login
│   ├── Session
│   ├── Realm-Liste
│   ├── Realm-Auswahl
│   └── Handoff-Ausstellung
│
├── Realm API (Realmserver)
│   ├── Handoff-Tokenprüfung (einmalig, realm-gebunden)
│   ├── Sessionprüfung
│   └── Elternkontroll-Abfragen
│
└── spätere Web API
    ├── Accountverwaltung
    └── ausdrücklich freigegebene Accountfunktionen
```

(LEGACY: die frühere „Internal World API" mit
World-Server-Authentifizierung/-Registrierung/-Heartbeat entfällt —
`/world/*`-Endpunkte bleiben nur kompatibel bestehen.)

Die Webseite wird erst später entwickelt.

Die API-Grenze wird trotzdem bereits bei der Serverarchitektur berücksichtigt.

---

# 6. Realm-Übergabe per Handoff (kein separater Worldserver)

Ein Realm-Server trägt sich nicht selbstständig als offizieller
Andora-Server ein: Der Spieler gelangt ausschließlich über eine
sichere Übergabe (Handoff) vom Login-Service auf seinen Realm.

```text
Login stellt Handoff aus (handoff.create: account_id + realm_id,
einmalig, kurze TTL) und stellt dazu die Session aus
        │
        ▼
Client verbindet zum Realm-Server (HELLO: session_id + handoff_token)
        │
        ▼
Realm-Server prüft+verbraucht den Handoff (handoff.validate, einmalig)
        │
        ▼
realm_id gebunden? (REALM_ID des Servers muss passen)
        │
        ├── Nein → ablehnen, Verbindung schließen
        │
        ▼
session_id vorhanden + gültig? (session.validate)
        │
        ├── nein → ablehnen, Verbindung schließen
        │
        ▼
session.account_id == handoff.account_id?
        │
        ├── nein → ablehnen, Verbindung schließen
        │
        └── ja → account_id übernehmen, Charakter betritt Andora
```

Ein fremder Server kann sich dadurch nicht einfach selbst in die offizielle Realm-/Serverstruktur eintragen; ein abgefangenes Token ist nur einmal und nur für den gebundenen Realm gültig.

Der Einstieg ist fail-closed: Fehlt der Handoff, ist er ungültig, verbraucht, ungültig abgelaufen oder an einen anderen Realm gebunden, fehlt die `session_id`, ist die Session ungültig/abgelaufen oder gehört die Session zu einem anderen Account als der Handoff, lehnt der Realm-Server den Einstieg ab und schließt die Verbindung. Nur wenn alle Prüfungen gleichzeitig erfolgreich sind, wird der Account in den Realm gelassen. Dabei prüft der Realm-Server ausschließlich über die signierte Auth-API; eine direkte Account-/Session-DB-Zugriff hat er nicht.

(LEGACY: Die frühere Registrierung separater World-Server mit eigenen
Server-Credentials (`world_servers`-Tabelle, `/world/authenticate`)
wird von keinem Dienst mehr verwendet und bleibt nur kompatibel
bestehen. Server-Credentials werden nicht unnötig im Klartext in der
Datenbank gespeichert.)

## Charakter-Lookup beim Einstieg (fail-closed)

VERBINDLICH für den Realm-Einstieg:

* **HELLO ist ausschließlich Login und Lookup. HELLO erzeugt niemals einen Charakter.** Der Einstieg darf keinen Charakter-Datensatz anlegen, auch nicht beim ersten HELLO eines unbekannten `char_id`.
* **`char_id` ist eine serverseitig vergebene positive Datenbank-ID** des Characters im Realm, zu dem der Account gehört. Der Client wählt sie nicht frei und darf sie nicht selbst vergeben; sie stammt aus dem vorgelagerten Auswahl- und Erstellungsschritt (siehe Abschnitt 16 und `Charaktererstellung_und_Charakterdarstellung.md`).
* **Der Realm-Lookup erfolgt mit `id` UND `account_id`.** Beide Bedingungen müssen gemeinsam erfüllt sein.
* **Ein fehlender oder fremder Datensatz wird fail-closed abgelehnt:** kein erfolgreicher Realm-Einstieg, keine Verbindung, kein Spieler im RAM-Zustand des Realms, keine Teilaktualisierung. Ein nicht gefundener `char_id` ist ein Ablehnungsfall, kein Anlass für einen Schreibvorgang.

Ein Charakter, der zu einem anderen Account oder zu keinem Account gehört, wird vom Realm nicht geladen und nicht erzeugt.

## Verbindungs-Einzigkeit und Takeover

Für die Zuordnung eines Charakters zu Realm-Verbindungen gilt verbindlich:

* Pro Charakter darf zu jedem Zeitpunkt höchstens eine aktive, zur Spiellogik berechtigte Realm-Verbindung existieren.
* Baut derselbe Charakter eine neue, vollständig authentifizierte Verbindung auf, übernimmt die neue Verbindung.
* Die alte Verbindung wird vor der Übergabe entmachtet und anschließend getrennt.
* Beide Verbindungen dürfen niemals gleichzeitig Spiellogik ausführen.
* Der aktuelle serverautoritative RAM-Zustand bleibt maßgeblich und darf nicht durch einen älteren Datenbankstand überschrieben werden.
* Das Cleanup einer verdrängten Verbindung darf weder die neue Verbindung noch den aktuellen Player-Zustand entfernen.
* Zuordnung und Cleanup müssen verbindungsspezifisch abgesichert werden, beispielsweise durch `conn_id` oder eine Verbindungsgeneration.
* Bei einem Fehler muss der Vorgang fail-closed enden, ohne zwei aktive Eigentümer derselben Charakterinstanz zu erzeugen.
* Ein einzelner Takeover ist ein normaler Reconnect-Fall und führt nicht zu Bann oder Bestrafung.

Die Regeln für das zugehörige Sicherheits-Logging und die Einordnung der Quell-IP stehen in `datenschutz_zugang.md` und `netzwerk_ip_schutz.md`.

---

# 7. Heartbeat

Der Realm-Server und der Spielclient halten ihre Verbindung über den
Spiel-Heartbeat aufrecht (`HEARTBEAT` → `SYNC`; siehe Protokoll in
`shared/protocol.gd` bzw. `src/realm-rs/src/protocol.rs`). Bleiben die Lebenszeichen eines Spielers aus,
gilt er als getrennt (Position speichern, DESPAWN, Registry putzen).

Der Realm-Server meldet zusätzlich seinen Betriebszustand über
`GET /health` und `GET /status` (Spielerzahl, Uptime, Tick-Statistiken).
Diese Heartbeat-/Statusmeldungen tragen die `realm_id` des Realms als verbindliches Zuordnungsfeld (`Coordinator.md`, Abschnitt 30), damit Realms bei gemeinsamen Diensten und der Administration eindeutig unterscheidbar bleiben.
Bleibt ein Realm unerreichbar, vermittelt der Login dorthin keine
neuen Spieler mehr (deaktivierte/nicht gelistete Realms lehnt bereits
`/handoff` ab).

(LEGACY: Der frühere World-Server-Heartbeat (`world_servers`-Tabelle,
`/world/heartbeat`) wird von keinem Dienst mehr verwendet und bleibt
nur kompatibel bestehen.)

---

# 8. Realm und Realm-Server

Realm (persistente Welt) und Realm-Server (ausführender Dienst) sind
unterschiedliche Dinge; einen separaten Worldserver-Dienst gibt es
nicht.

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

Realm-Versionen können parallel existieren, beispielsweise als `Live`, `Classic`, `Test` oder `Event`. Jede Realm-Version besitzt ihre eigene statische Inhaltsversion und ist eigenständig aktualisierbar (siehe `Deployment_Betriebsarchitektur.md`). Die Realm-Auswahl zeigt dem Spieler den jeweiligen Realm inklusive seiner Inhaltsversion.

**Hinweis:** Realm-Versionen (Live, Classic, Test, Event) sind Inhaltsversionen eines Realms, keine clientgespezifischen Realms. Es gibt keine PC-, Pi-, Browser- oder UE-Realms; alle offiziellen Clients verbinden sich mit denselben Realms (`Mehrere_Offizielle_Clients.md`).

## Realm-Server

Der Realm-Server ist dagegen der Dienst, welcher einen Realm ausführt
(Rust, `src/realm-rs`; der frühere Node.js/TypeScript-Code unter `src/realm/`
wurde aus dem Repository entfernt).

Ein Realm kann später bei Bedarf von mehreren technischen Realm-Prozessen getragen werden.

```text
Realm DE-1
    │
    ├── Realm-Prozess A
    ├── Realm-Prozess B
    └── gemeinsamer Realm-Zustand (realm_state_de1)
```

Die technische Skalierung eines Realms muss für den Spieler nicht sichtbar sein.

## Realm-Rulesets

Jeder Realm verwendet genau ein definiertes Ruleset. Rulesets sind Realm-Regelvarianten, keine Client-Varianten.

Grundsätze:

* `normal` ist das regulär verwendete Ruleset.
* Weitere Rulesets (`hardcore`, `roleplay`) sind reservierte Möglichkeiten und können später bei tatsächlichem Bedarf aktiviert werden; ihre konkreten Spielregeln sind noch nicht definiert.
* Die Realm-Software wird nicht pro Ruleset geforkt: Dasselbe Realm-Binary führt jedes Ruleset aus; das Ruleset ist Konfiguration (Realm-Metadaten), keine eigene Implementierung.
* Zentrale Spielmechaniken dürfen nicht unnötig fest auf ausschließlich ein Ruleset verdrahtet werden.
* Alle unterstützten offiziellen Clients eines Realms verwenden dasselbe Ruleset. Es gibt keine Godot-, Browser- oder UE-spezifischen Rulesets.

Reservierte Rulesets (Vorbereitung statt Aktivierung):

* `normal`: reguläres Andora-Regelwerk.
* `hardcore`: alternative Realm-Regeln für Spieler, die eine härtere Spielweise wünschen. Dauerhafter Charaktertod (Permadeath) ist als mögliche Regel vorgemerkt, aber noch nicht verbindlich definiert.

Für das Dungeon-/Encounter-Design gilt im Hinblick auf einen zukünftigen Hardcore-Realm folgendes Fairnessprinzip (keine vollständigen Hardcore-Regeln): Je schwerer die Konsequenz eines Fehlers, desto wichtiger ist es, dass die Gefahr durch Beobachtung, Erfahrung, Kommunikation und gutes Gruppenspiel beherrschbar bleibt. Spieler sollen nach einem schweren Fehler nachvollziehen können, was sie falsch gemacht haben („Wir hätten die Patrouille abwarten müssen.“ statt „Woher hätten wir das wissen sollen?“). Hardcore darf nicht dadurch künstlich schwer werden, dass bekannte Dungeonmechaniken ohne erkennbare Grundlage plötzlich andere Aggro-, Social-Aggro- oder Pullregeln verwenden. Die grundlegenden Encounter-Regeln bleiben lesbar und erlernbar; die wesentlich höhere Konsequenz eines Fehlers kann bereits einen erheblichen Teil der Hardcore-Schwierigkeit erzeugen. (Prinzipienquelle: faire, lesbare Gefahren mit Entscheidungsfenster; siehe `references/Andora-Design-Vorschlaege.md`, Kandidat 7.)
* `roleplay`: alternative Realm-Regeln für stärker rollenspielorientierte Spieler. Welche mechanischen RP-Regeln gelten, wird erst später festgelegt.

Hardcore- und RP-Realms müssen nicht zum Release angeboten werden. Ziel ist, später auf Spielerinteresse reagieren zu können und beispielsweise kurzfristig einen Realm mit `HC` oder `RP` im Namen starten zu können, ohne dafür zunächst die grundlegende Realm-Architektur umbauen zu müssen.

Beispiel (später möglich):

```text
DE-1       → normal
DE-2       → normal
DE-RP-1    → roleplay
DE-HC-1    → hardcore
```

Charaktertransfers zwischen Realms mit unterschiedlichen Rulesets benötigen eigene, später zu definierende Regeln (siehe Abschnitt 12). Fresh-Start-Regeln gelten unabhängig vom Ruleset.

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

Persistente Charakterdaten gehören zur Realm-Datenbank des jeweiligen Realms (`realm_state_<realm>`).

Eine separate zentrale Character-Datenbank bzw. ein eigener Character-Service wird nicht verwendet.

Beispiel:

```text
realm_state_de1

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

Ein Charakter existiert ausschließlich in der Datenbank des Realms, auf dem er spielt. Die endgültige technische Aufteilung einzelner Character-Systeme wird erst beim jeweiligen System festgelegt.

---

# 11. Charakter und Realm

Charakterdaten liegen direkt in der Realm-Datenbank ihres Realms (`realm_state_<realm>`).

Dadurch können realmbezogene Vorgänge innerhalb derselben Datenbank verarbeitet werden. Ein späterer Realmtransfer bleibt als kontrollierte Migration zwischen zwei Realm-Datenbanken technisch möglich (siehe Abschnitt 12).

Ein Realm besitzt seinen eigenen persistenten Weltzustand und seine eigene statische Inhaltsversion.

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

Da Charakterdaten in der Realm-Datenbank ihres Realms liegen (`realm_state_<realm>`), ist ein Realmtransfer eine kontrollierte Migration von Charakterdaten zwischen zwei Realm-Datenbanken.

Ein solcher Transfer darf nicht durch direkte Cross-DB-Abhängigkeiten im normalen Spielcode entstehen. Er wird als eigener, kontrollierter Vorgang behandelt.

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

# 14. Statische Weltdefinitionen

Grundlegende Definitionen der Spielwelt liegen nicht zentral, sondern realmbezogen in der Realm-Datenbank (`realm_state_<realm>`) als statische Inhaltsversion des jeweiligen Realms.

Eine Realm-Inhaltsversion kann beispielsweise enthalten:

```text
realm_state_de1 (statische Inhaltsversion)

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

Diese Daten beschreiben die grundlegenden Regeln und Inhalte dieser Realm-Version.

> **Die statische Inhaltsversion beschreibt, was in diesem Realm existieren kann.**

Da sich die Inhaltsdefinitionen in der Realm-Datenbank befinden, können verschiedene Realm-Versionen (z. B. `Live`, `Classic`, `Test` oder `Event`) parallel unterschiedliche Inhalte besitzen und eigenständig aktualisiert werden.

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

Damit können sich zwei Realms trotz identischer oder unterschiedlicher statischer Definitionen vollkommen unterschiedlich entwickeln.

Beispiel:

```text
realm_state_de1              realm_state_de5
(eigene statische            (eigene statische
 Definitionen +              Definitionen +
 eigener dynamischer         eigener dynamischer
 Zustand)                    Zustand)

EXP1 weit                    frisch gestartet
alte Gilden                  neue Gilden
Gildenstädte                 noch keine Städte
entwickelte                  junge
Wirtschaft                   Wirtschaft
```

> **Die statische Inhaltsversion sagt, was in dieser Realm-Version existieren kann.**

> **Realm-State sagt, was in dieser konkreten Welt tatsächlich passiert ist.**

---

# 16. Charaktererstellung

Nach der Realm-Auswahl kann der Spieler:

* einen vorhandenen, für diesen Realm gültigen Charakter verwenden
* einen neuen Charakter erstellen

Bei einem Fresh-Start-Realm kann die Verwendung bzw. Übertragung älterer Charaktere entsprechend der Realmregeln gesperrt sein.

VERBINDLICH:

* Die Charaktererstellung ist **nicht** Teil des Realm-Einstiegs. HELLO ist Login und Lookup und legt keinen Charakter an (siehe Abschnitt 6 „Charakter-Lookup beim Einstieg“).
* Die `character_id` wird bei der Erstellung **serverseitig** vergeben und ist eine positive Datenbank-ID; der Client vergibt sie nicht und kann sie nicht erzwingen.
* Die Erstellung ist ein **separater, authentifizierter Ablauf** außerhalb des Realm-Einstiegs. Die fachlichen Anforderungen an diesen Ablauf stehen in `Charaktererstellung_und_Charakterdarstellung.md`.

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
Ruleset
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

Zur späteren Webseite gehört gemäß `Monetarisierung_und_Donations.md` eine freiwillige Möglichkeit zur finanziellen Unterstützung des Projekts (Donation). Diese ist ausdrücklich kein Shop und keine Gameplay-Monetarisierung; Zahlungsanbieter, Beträge, technische Umsetzung und organisatorische/rechtliche Ausgestaltung sind noch nicht festgelegt.

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
* realm-gebundene Einmal-Handoffs statt separater Server-Registrierung
* keine selbstständige Aufnahme fremder Server in die Realmstruktur
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
       ├── Ruleset
       └── Character-Zuordnung
      │
      ▼
sichere Übergabe (Handoff, realm-gebunden, einmalig)
       │
       ▼
Realm-Server des Realms (Handoff prüfen+verbrauchen)
       │
       ▼
Charakter betritt Andora
```

---

# 21. Architektur-Leitsätze

> **Der Account gehört keinem Realm.**

> **Ein Realm ist eine eigenständige persistente Welt.**

> **Realms werden nicht nach Clienttypen getrennt (keine PC-, Pi-, Browser- oder UE-Realms); alle offiziellen Clients verbinden sich mit denselben Realms.**

> **Jeder Realm verwendet genau ein definiertes Ruleset (`normal`, später ggf. `hardcore`/`roleplay`). Rulesets sind Realm-Regelvarianten, keine Client-Varianten. Die Realm-Software wird nicht pro Ruleset geforkt.**

> **Die statische Inhaltsversion eines Realms beschreibt, was existieren kann. Realm-State beschreibt, was tatsächlich passiert ist.**

> **Der Realm-Server führt seinen Realm technisch aus; er definiert nicht dessen dauerhafte Identität. Einen separaten Worldserver-Dienst gibt es nicht.**

> **Neue Realms beginnen als echte neue Welten und können durch eine Fresh-Start-Sperre vor dem unmittelbaren Import alter Machtstrukturen geschützt werden.**

> **Charakterdaten gehören zur Realm-Datenbank ihres Realms; spätere kontrollierte Charaktertransfers bleiben als Migration zwischen Realm-Datenbanken möglich.**

> **Sprache, Region und Latenz informieren den Spieler – die Wahl des Realms trifft der Spieler selbst.**

> **Webseite, Client, Login-Service und Realm-Server greifen ausschließlich über dafür vorgesehene Schnittstellen auf Account-Funktionen zu.**

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

Insbesondere erhalten Login-Service und Realm-Server niemals:

- Klartextpasswörter (der Login leitet nur das eingegebene Passwort weiter)
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

Login-Service und Realm-Server benötigen keine E-Mail-Adressen und bekommen diese daher niemals übertragen.

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

Login-Service (eigener Dienst, `src/login`):
- Benutzername/E-Mail-Lookup (nur zur Weiterleitung an die Auth-API)
- eingegebenes Passwort (nur zur Weiterleitung, nie gespeichert)
- Session- und Handoff-Tokens (nur zur Weiterleitung/Prüfung)

Realm-Server des Realms:
- account_id
- character_id
- handoff_token (einmalig, realm-gebunden)
- session_id (für Elternkontroll-Polling)
- notwendige Spielberechtigungen
- Charakterdaten (in realm_state_<realm>)

Realm-Server erhalten keine sensiblen Accountdaten.

---

## World-Handoff (Realm-Übergabe)

Beim Wechsel vom Login-Service zum Realm-Server werden keine sensiblen Accountdaten übertragen.

Der Realm-Server erhält beispielsweise nur:

account_id
character_id
handoff_token
session_id
permissions

Der Realm-Server prüft, ob der Handoff gültig und an seinen Realm gebunden ist, und verbraucht ihn dabei (einmalig).

Danach wird der Handoff-Token ungültig bzw. läuft nach kurzer Zeit automatisch ab.

---

## Schutz bei kompromittierten Realm-Servern

Ein kompromittierter Realm-Server soll nicht automatisch einen vollständigen Account-Datenverlust ermöglichen.

Da dort keine Passwörter, Passwort-Hashes, E-Mail-Adressen oder Account-Verschlüsselungsschlüssel benötigt werden, sollen diese Daten dort auch niemals vorhanden sein.

Damit gilt:

> Ein Realm-Server kennt den Spieler, aber nicht seine sensiblen Accountdaten.

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
- an Login-/Realm-Server verteilt werden
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

Ein Handoff-Token ist beispielsweise nur für den Übergang zu einem bestimmten Realm gültig (realm-gebunden, einmalig).

---

## RAM-Dump-Szenario

Andora geht davon aus, dass ein Angreifer bei vollständiger Kontrolle über einen laufenden Server möglicherweise auch dessen Arbeitsspeicher auslesen kann.

Eine vollständige Verhinderung sensibler Klartextdaten im RAM ist nicht möglich, wenn ein Dienst diese Daten tatsächlich verarbeiten muss.

Das Sicherheitsziel lautet deshalb:

> Ein kompromittierter Prozess soll nur die sensiblen Daten preisgeben können, die dieser Prozess tatsächlich benötigt.

Ein RAM-Dump eines Login- oder Realm-Servers darf daher beispielsweise keine komplette Accountdatenbank, E-Mail-Liste oder Passwort-Hashes enthalten.

Ein RAM-Dump des Account/Auth-Servers kann dagegen aktuell verwendete sensible Daten enthalten und muss deshalb als besonders kritischer Sicherheitsvorfall behandelt werden.

---

## Architekturregel

> Datenminimierung gilt nicht nur für Datenbanken, sondern auch für Netzwerkverkehr und Arbeitsspeicher.

> Kein Dienst erhält sensible Informationen nur „für den Fall, dass er sie vielleicht irgendwann braucht“.

> Je weniger sensible Daten ein Prozess kennt, desto kleiner ist der mögliche Schaden bei einer Kompromittierung.
