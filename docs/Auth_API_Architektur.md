# Auth-API-Architektur

## 1. Ziel

Die Auth-API bildet die zentrale Sicherheitsgrenze zwischen sensiblen
Accountdaten und den übrigen Andora-Diensten.

> Nur der Auth/API-Service besitzt direkten Zugriff auf die
> Auth-Datenbank.

Loginserver, Realm-Server, Webseite und spätere externe Dienste
erhalten keinen direkten Zugriff auf sensible Accounttabellen. Sie
kommunizieren ausschließlich über definierte API-Endpunkte.

## 2. Grundarchitektur

``` text
Webseite ───────┐
Loginserver ────┼──► Auth/API-Service ───► auth DB
Realmserver ────┘
```

Jeder aufrufende Dienst besitzt eigene API-Credentials mit begrenzten
Berechtigungen.

## 3. Auth/API-Service

Der Auth/API-Service ist der einzige reguläre Andora-Dienst mit direktem
Zugriff auf `auth`.

Nur seine Konfiguration enthält:

``` text
AUTH_DB_HOST
AUTH_DB_PORT
AUTH_DB_USER
AUTH_DB_PASSWORD
AUTH_DB_NAME
```

Diese Zugangsdaten dürfen nicht in Loginserver, Realmserver, Webseite
oder Client vorhanden sein.

## 4. Verantwortlichkeiten

Der Auth/API-Service übernimmt unter anderem:

-   Account anlegen und suchen
-   Passwort prüfen und ändern
-   Passwort-Hashes erzeugen
-   E-Mail verschlüsseln und bei Bedarf entschlüsseln
-   E-Mail-Lookup
-   Account- und Banstatus prüfen
-   Sessions und Login-Tokens verwalten
-   Handoff-Tokens verwalten
-   Realms verwalten
-   (LEGACY: registrierte World-Server verwalten, World-Server
-   authentifizieren, deren Heartbeats verarbeiten — kein separater
-   Worldserver mehr, Endpunkte nur kompatibel erhalten)

Er gibt nur die Informationen zurück, die der anfragende Dienst
tatsächlich benötigt.

## 5. Service-Authentifizierung

Webseite, Loginserver und Realmserver erhalten jeweils eigene
Service-Credentials.

``` text
web-service
login-service
realm-de1-service
realm-de2-service
```

Es gibt kein gemeinsames universelles Secret.

> Ein Service-Credential darf nur die API-Funktionen verwenden, die der
> jeweilige Dienst tatsächlich benötigt.

## 6. Webseite

Die Webseite besitzt keinen direkten Zugriff auf `auth`.

Mögliche API-Berechtigungen:

-   Accountregistrierung anfragen
-   Login anfragen
-   Passwortänderung anstoßen
-   Passwort-Wiederherstellung anstoßen
-   ausgewählte Accountinformationen abrufen
-   E-Mail-Änderung anstoßen

Die Webseite darf niemals Passwort-Hashes, verschlüsselte
E-Mail-Rohdaten, Verschlüsselungsschlüssel, DB-Credentials, vollständige
Accounttabellen oder beliebige SQL-Zugriffsmöglichkeiten erhalten.

> Eine kompromittierte Webseite darf nicht automatisch zu einer
> kompromittierten Account-Datenbank führen.

## 7. Loginserver

Der Loginserver besitzt keinen direkten Zugriff auf `auth`.

``` text
Client
  ↓
Loginserver
  ↓
Auth/API-Service
  ↓
auth DB
```

Die Auth-API sucht den Account, prüft Passwort, Accountstatus und
Banstatus, verwaltet die notwendige Session und gibt nur das notwendige
Ergebnis zurück.

Der Loginserver benötigt beispielsweise:

``` text
account_id
session_id
permissions
login_status
```

Er benötigt keine Passwort-Hashes, verschlüsselten E-Mail-Daten,
Lookup-Hashes oder Verschlüsselungsschlüssel.

### Session-Widerruf (ausdrücklich, VERBINDLICH)

Die Auth-API verwaltet die Session. Ein **ausdrücklicher Widerruf** unterscheidet sich fachlich vom bloßen Ablaufzeitpunkt der Session (das Lebenszyklusverhalten für den Realm ist in `Login_Realm_Architektur.md`, Abschnitt „Session-Lebenszyklus nach dem Einstieg", verbindlich festgelegt).

VERBINDLICH gilt:

* Ein ausdrücklicher Widerruf einer Session muss **alle** über diese Session bestehenden Realm-Verbindungen beenden, wenn der Realm erreichbar ist; die Frist beträgt **spätestens 30 Sekunden** nach dem Widerruf.
* Ein Widerruf **aller** Sessions eines Accounts muss **alle** aktiven Realm-Verbindungen dieses Accounts beenden.
* **Kein** Widerruf und **kein** Ablauf darf **anderweitig** wirken, insbesondere nicht über Bann, Sperre oder zusätzliche Sanktionen. Der Widerruf beendet ausschließlich die zugehörigen Verbindungen.

Anlassfälle, für die die API einen Widerruf auslösen muss:

* **expliziter Widerruf einer einzelnen Session**;
* **Passwortänderung** — der bisherige Passwortbesitz entfällt, alle zugehörigen Anmeldungen sind zu beenden;
* **Passwort-Reset beziehungsweise Account-Recovery** — der Passwortbesitz wurde ohne Kenntnis des bisherigen Passwortinhabers ersetzt; alle zugehörigen Anmeldungen sind ebenfalls zu beenden;
* **2FA-Reset** — die zweite Authentifizierungsstufe wurde zurückgesetzt, alle zugehörigen Anmeldungen sind zu beenden;
* **weitere administrative Session-Widerrufe**, soweit die API solche anbietet.

**IST-ABWEICHUNG (dokumentiert, nicht umgesetzt):** Der aktuelle Code beendet Sessions bei Passwortänderung und beim 2FA-Reset, **nicht** jedoch beim Passwort-Reset/Recovery. Der Recovery-Pfad setzt das Passwort zurück, ohne die zugehörigen Sessions zu widerrufen. Das widerspricht der oben festgelegten Sollsemantik und ist als Umsetzungsdefizit in `docs/Security.md` unter `AUTH-02b` geführt. Dieser Abschnitt beschreibt die **Soll**-Semantik; er behauptet nicht, dass der heutige Code sie bereits erfüllt.

### Ausfallverhalten der Auth-API

* Neue Logins bleiben bei Ausfall der Auth-API **fail-closed**; ein Einstieg ohne erfolgreiche Session-Prüfung erfolgt nicht.
* Ein Erreichbarkeitsproblem der Auth-API ist **kein** Sicherheitsereignis und beendet **keine** bereits laufenden Realm-Verbindung.
* Während eines Ausfalls kann ein ausdrücklicher Widerruf im Realm vorübergehend nicht wirksam werden. Das ist ein bewusst akzeptiertes Restrisiko zugunsten der Verfügbarkeit laufender Verbindungen und keine Revocation-Garantie.
* Fehler und Wiederherstellung werden ohne Session-ID, Token und Roh-IP protokolliert (siehe Abschnitt 12, Datenminimierung).

## 8. Login-/Realm-Server

Login- und Realmserver besitzen keinen direkten Zugriff auf `auth`.

Der Login-Service (`src/login`) verwendet beispielsweise:

-   Login prüfen (`account.authenticate`)
-   Session verwalten/prüfen
-   Realm-Liste abrufen
-   Handoff-Token ausstellen

Realmserver verwenden beispielsweise:

-   Handoff-Token validieren und verbrauchen (realm-gebunden)
-   Session validieren
-   Elternkontroll-Status/PIN/Extension abfragen
-   notwendige Berechtigungen prüfen

(LEGACY: World-Server-Authentifizierung gegenüber dem Auth-System —
kein separater Worldserver mehr.)

> Ein Realm-Server kennt den Spieler, aber nicht seine sensiblen
> Accountdaten.

## 9. Realm-spezifische Service-Credentials

Realmserver erhalten eigene Service-Credentials:

``` text
realm-de1-service
realm-de2-service
realm-en1-service
```

Dadurch kann ein kompromittierter Realmserver nicht automatisch die
API-Berechtigungen anderer Realmserver verwenden.

Die Auth-API kann zusätzlich Service-Credential, registrierten
Realm-Server, Realm-Zuordnung und Serverstatus prüfen.

## 10. API-Berechtigungen

Beispiel:

``` text
WEB
├── account.register
├── account.login
├── account.password_change
└── account.recovery

LOGIN (Login-Service, `src/login`)
├── account.authenticate
├── session.validate
├── session.revoke
├── realm.list
└── handoff.create

REALM (Realm-Server, `src/realm-rs`; Übergangsstand `src/realm`)
├── handoff.validate
├── session.validate
├── account.permissions
├── parental.status
└── parental.pin
```

Die Liste wird nur erweitert, wenn ein konkreter Dienst zusätzliche
Rechte benötigt.

## 11. Keine generische Datenbank-API

Verboten sind allgemeine Funktionen wie:

``` text
execute_sql
query_table
get_any_account_field
database_query
```

Die API bietet stattdessen fachliche Operationen wie:

``` text
authenticateAccount()
validateSession()
validateHandoff()
registerAccount()
changePassword()
getRealmList()
```

> Die API stellt Funktionen bereit, keine SQL-Fernsteuerung.

## 12. Datenminimierung

Ein Realmserver benötigt zur Spielerübergabe möglicherweise:

``` text
account_id
character_id
session_id
permissions
handoff_token
```

Er benötigt keine E-Mail, Passwort-Hashes, E-Mail-Lookup-Hashes,
Recovery-Daten oder Verschlüsselungsschlüssel.

> Sensible Daten dürfen nur in den Prozessen und nur für die Dauer
> vorhanden sein, in der sie tatsächlich benötigt werden.

Dies gilt für Datenbanken, Netzwerkverkehr, Logs und Arbeitsspeicher.

## 13. Secrets

Service-Credentials und Verschlüsselungsschlüssel werden:

-   nicht im Git-Repository gespeichert
-   nicht in SQL-Dateien geschrieben
-   nicht an Clients übertragen
-   nicht in normalen Logs ausgegeben

Jeder Dienst erhält nur seine eigenen Secrets. Secrets müssen rotierbar
sein.

## 14. Auth-DB-Migrationen

Nur der Auth/API-Service prüft und migriert die `auth`-Datenbank.

``` text
Auth/API-Service startet
        ↓
Verbindung zur auth DB
        ↓
db_version prüfen
        ↓
fehlende Migrationen ermitteln
        ↓
Migrationen der Reihe nach anwenden
        ↓
db_version aktualisieren
        ↓
Auth/API-Service freigeben
```

Schlägt eine notwendige Migration fehl, darf der Auth/API-Service nicht
normal starten.

Dateiformat: `NNN_name.sql` in `src/api/db/auth/migrations/`. Der Name wird
am **ersten** Unterstrich getrennt (Version + Tag); der Tag darf weitere
Unterstriche enthalten (z. B. `004_world_servers.sql` → Version 4, Tag
`world_servers`). Dieselbe Konvention gilt für die Realm-Migrationen
(`src/realm-rs/migrations/<NNN>_<tag>.sql`, Runner in `src/realm-rs/src/`
mit eigener `db_version`-Tabelle in der Realm-Datenbank `realm_state_<realm>`).

Loginserver, Realmserver und Webseite führen keine Migrationen auf
`auth` aus.

Bereits angewendete Migrationen werden nicht nachträglich verändert.
Änderungen erfolgen durch neue, fortlaufend nummerierte Migrationen.

## 15. Verhältnis zu anderen Datenbanken

Die Andora-Datenbanken bleiben fachlich getrennt:

``` text
auth
realm_state_<realm>
```

Eine separate `character`- oder `world_data`-Datenbank existiert nicht mehr; statische Weltdefinitionen und Charakterdaten liegen in der jeweiligen `realm_state_<realm>`-Datenbank.

Die Auth-API ist keine allgemeine Andora-Datenbank-API.

Für `realm_state_<realm>` wird separat festgelegt, welcher Dienst direkten Zugriff benötigt. Es ist ausschließlich der für den jeweiligen Realm zuständige Realm-Server.

## 16. Sicherheitsziel

``` text
Webserver kompromittiert
        ↓
möglicherweise Web-Service-Credential kompromittiert
        ↓
nur erlaubte Web-API-Funktionen
        ↓
kein direkter auth-DB-Zugriff
        ↓
keine Passwort-Hashes
keine DB-Credentials
keine Verschlüsselungsschlüssel
```

Ein kompromittiertes Service-Credential bleibt ein Sicherheitsproblem
und muss gesperrt bzw. rotiert werden können. Die API begrenzt jedoch
den erreichbaren Datenumfang und verhindert beliebigen direkten
Datenbankzugriff.

## 17. Leitsätze

> Nur der Auth/API-Service besitzt direkten Zugriff auf die
> Auth-Datenbank.

> Webseite, Loginserver und Realmserver greifen niemals direkt auf
> sensible Accounttabellen zu.

> Jeder Dienst besitzt eigene API-Credentials und nur die minimal
> notwendigen Berechtigungen.

> Die API liefert nur die Informationen zurück, die der anfragende
> Dienst tatsächlich benötigt.

> Die API stellt fachliche Funktionen bereit und niemals eine allgemeine
> SQL-Schnittstelle.

> Ein Realm-Server kennt den Spieler, aber nicht seine sensiblen
> Accountdaten.

> Eine kompromittierte Webseite darf nicht automatisch zu einer
> kompromittierten Account-Datenbank führen.

> Datenminimierung gilt für Datenbanken, Netzwerkverkehr, Logs und
> Arbeitsspeicher.
