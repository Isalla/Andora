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

**Stand:** Beide Anlassfälle sind umgesetzt, einschließlich Passwort-Reset/Recovery. Die Umsetzung ist im folgenden Abschnitt mit Belegen beschrieben; dieser Abschnitt beschreibt die **Soll**-Semantik.

### Stand der Umsetzung (Commit `38e1fbf8e405ce8c79c78199d9b476032f222d40`)

Die oben festgelegte Sollsemantik ist umgesetzt. Belege aus dem Repository:

**Persistenter Marker.** Migration `src/api/db/auth/migrations/014_sessions_revoked_at.sql` ergänzt `sessions.revoked_at TIMESTAMP NULL`; Bestandszeilen bleiben `NULL` und damit gültig. Ein Widerruf **markiert** die Zeile, statt sie zu löschen (`src/api/store.go:720`, `:903`, `:997`, `:1035`, jeweils `WHERE … AND revoked_at IS NULL`, also idempotent). Ein physisches `DELETE FROM sessions` existiert im produktiven Quellcode **nicht** mehr; verbleibender Löschweg ist ausschließlich der Foreign-Key-Cascade `fk_sessions_account … ON DELETE CASCADE` (`002_sessions.sql:12`).

**Vier Statuswerte.** `SessionStatus` (`store.go:36-49`): `valid`, `expired`, `revoked`, `missing`. Die Klassifikation `sessionStatusOf` (`store.go:618`) prüft `revoked` **vor** dem Ablaufzeitpunkt, damit ein gleichzeitig abgelaufener Token nicht als bloßer Ablauf fehlklassifiziert wird.

**Kompatibler Einzel-Validate-Pfad.** `POST /session/validate` meldet unverändert nur `valid: true` oder `valid: false`; der boolesche Vertrag und das Antwortformat bleiben unverändert. Der Status wird dort **nicht** ausgegeben.

**Batchstatus-Endpunkt.** `POST /session/status/batch` (`endpoints.go:511`, Route `server.go:97`) gibt je Eintrag `index`, `status` und die **autoritative** `account_id` zurück. Die Zuordnung erfolgt über die Position (`index`), nicht über ein Geheimnis.

**Berechtigung.** Es gilt die **bestehende** `permSessionValidate` (`endpoints.go:520`); es wurde **keine neue Permission** eingeführt. Das Mengengerüst des Endpunkts (`session.validate`) in Abschnitt 10 bleibt unverändert.

**Anlassfälle.** Passwortänderung (`ChangePasswordRevokeAll`, `store.go:986`), Passwort-Reset/Recovery (`RecoverPassword`, `store.go:888`) und 2FA-Reset (`RevokeAllSessions` aus `twofactor.go`) markieren jeweils alle Sessions des Accounts. `RecoverPassword` schreibt zusätzlich das Security-Event `password_reset` (`store.go:254`, `:915`) — **nicht** `password_changed`.

**Recovery-Transaktionswirkung.** Der gesamte Reset läuft in **einer** Transaktion in dieser Reihenfolge: Passwort und `ban_until` → Sessions markieren → Trusted Devices löschen → Security-Event → Recovery-Token verbrauchen → Commit. Jeder Fehlerzweig führt zum vollständigen Rollback, sodass altes Passwort, Sessions, Trusted Devices und die Wiederverwendbarkeit des Recovery-Tokens erhalten bleiben.

**Grenzen.** Maximal `MaxSessionStatusBatch = 250` Einträge je Request (`endpoints.go:448`); eine Überschreitung wird mit HTTP 413 **fail-closed abgelehnt**, nie gekürzt. Ein Body über `maxSessionStatusBatchBytes` (32 KiB) wird bei bekannter `Content-Length` **vor** der Autorisierung abgelehnt, sodass er gar nicht erst gelesen wird. Das serverweite `maxBodyBytes` (64 KiB, `auth.go:20`) bleibt die äußere Schranke. Session-Tokens sind exakt 64 Hex-Zeichen (`isHexToken`, `store.go:1291`) und werden **vor** Hashing und Datenbankzugriff geprüft. Der Batchpfad führt **eine** `SELECT … WHERE token_hash IN (…)` aus (`store.go:676-678`), kein N+1.

**Datenminimierung.** Tokens werden ausschließlich zum Hashen und für den Lookup verwendet, erscheinen in keiner Antwort und in keinem Log. Die von der Anforderung mitgelieferte `account_id` wird vom Server nicht für die Abfrage verwendet; die Antwort nennt stets die der Datenbank.

**Klassifizierung des Vorgangs `/session/revoke` (VERBINDLICH).** Der einzige produktive Aufrufer ist der **freiwillige Logout** des Login-Dienstes (`src/login/handlers.go:62`, Aufruf `:74`); der Kommentar dort hält fest, dass der **Besitz des präsentierten Tokens** diesen Aufruf autorisiert. Ein **administrativer oder sicherheitsmotivierter** Einzelwiderruf existiert derzeit **nicht**. Der Vorgang ist deshalb **kein eigenständiges Sicherheitsereignis, sondern ein normaler Session-Lebenszyklusvorgang**. Für diesen freiwilligen Logout ist **kein dauerhafter Eintrag in `security_events` erforderlich**; es wird **kein** neuer Eventtyp `session_revoked` eingeführt und **kein** Event je regulärem Logout geschrieben. Dadurch entsteht **keine** zusätzliche unbegrenzte Wachstumsquelle in `security_events`. Das Schema von `security_events` bleibt unverändert; es werden **kein** Token, **kein** Token-Hash, **keine** Session-ID, **keine** Roh-IP, **kein** Actor und **keine** zusätzlichen personenbezogenen Metadaten gespeichert. Diese Festlegung gilt für den **heutigen** Pfad und **trifft keine** Aussage darüber, dass Einzelwiderrufe grundsätzlich nie auditpflichtig wären.

**Bedeutung und Lebensdauer von `revoked_at` (VERBINDLICH).** `revoked_at` ist für den heutigen Pfad ein **technischer Laufzeitmarker**. Er ermöglicht der Auth-API, `revoked` von `valid` und `expired` zu unterscheiden (`store.go:618-628`), und ist für `AUTH-02b` erforderlich, **solange die Sessionzeile existiert**. Er ist **kein** dauerhaftes, von der Sessionzeile unabhängiges Auditprotokoll: Wird die Sessionzeile später gelöscht, **verschwindet auch dieser Nachweis vollständig**; er ist danach **nicht** rekonstruierbar. Für den heutigen freiwilligen Logout wird das **fachlich akzeptiert**, weil dafür kein dauerhafter Sicherheitsnachweis verlangt wird. `revoked_at` bleibt gleichwohl zwingend und darf **nicht** entfernt werden.

**Entwicklungsgrenze für künftige administrative Einzelwiderrufe (VERBINDLICH).** Wird später ein administrativer oder sicherheitsmotivierter Einzelwiderruf eingeführt, muss dessen Auditsemantik **vor** der Freigabe dieses Pfads **separat entschieden** werden; er darf **nicht** stillschweigend die heutige Logout-Semantik übernehmen. Zu entscheiden wären dann mindestens **Ereignisart**, **Accountbezug**, **Actor-Nachweis**, **Transaktionssemantik** und **Aufbewahrung**. Eine konkrete technische Lösung oder Eventstruktur wird hier **nicht** vorweggenommen.

**Abgrenzung zu den bestehenden Sicherheitsereignissen (VERBINDLICH).** Passwortänderung, Passwort-Reset/Recovery und 2FA-Reset bleiben fachlich von einem freiwilligen Logout **getrennt**; ihre vorhandenen `security_events`-Nachweise werden durch diese Festlegung **nicht ersetzt und nicht abgeschwächt**. Die Festlegung erlaubt ausschließlich, widerrufene Sessionzeilen **des heutigen Logout-Pfads** später ohne zusätzlichen Logout-Event zu bereinigen. Sie legt **keine** Retentionfrist, **keine** Batchgröße und **keine** Cleanup-Frequenz fest.

**Retention: ausdrücklich nicht umgesetzt, und nicht einheitlich entscheidbar.** Abgelaufene **und** auch widerrufene Zeilen bleiben erhalten; ein Cleanup-Mechanismus existiert nicht. Der Punkt ist als `BESTÄTIGT` unter `P-33` in `docs/Security.md` geführt, Priorität `GERING`. Festgehalten ist dort:

* **Widerrufene Zeilen sind technisch grundsätzlich bereinigbar.** Nach ihrer Löschung liefert der Status `missing`, was für aktive Verbindungen fail-closed ist und dieselbe Trennwirkung wie `revoked` hat. Auth-API-Ausfälle ändern das nicht: nach Wiedererreichbarkeit führt `missing` weiterhin zur Trennung. **Offen** ist allein die **Audit-Semantik** — `/session/revoke` erzeugt keinen eigenen `security_events`-Eintrag, sodass `revoked_at` heute der einzige persistente Nachweis eines Einzelwiderrufs ist. Solange das nicht entschieden ist, wird **keine** Löschfrist, **kein** Batchumfang und **kein** Cleanup-Takt festgelegt.
* **Abgelaufene, nicht widerrufene Zeilen dürfen nicht automatisch gelöscht werden.** Nach einer Löschung lieferte der Batch `missing`, und der Realm behandelt `missing` wie einen Widerruf — das würde eine nach `AUTH-02a` weiterhin zulässige Verbindung schließen. Eine Bereinigung ist erst zulässig, wenn zuverlässig feststeht, dass keine aktive Realm-Verbindung die Session mehr verwendet; dieser Nachweis ist technisch offen und wird hier **nicht** vorweggenommen.
* **Monitoring fehlt vollständig** und ist Voraussetzung einer späteren Entscheidung: weder Zeilenzahlen nach Status noch Tabellen-/Indexgröße noch Wachstumsrate noch Laufzeit der Statusabfrage werden erhoben.

Dieser Abschnitt stellt **keine** Retention als vorhanden dar und legt **keine** Frist, Batchgröße, Ausführungsfrequenz oder gesetzliche Aufbewahrungsdauer fest.

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

## 18. Fachliche Atomarität der Security-Events

Dieser Abschnitt legt fest, **welche** sicherheitsrelevanten Vorgänge ein
verbindliches Auditereignis in `security_events` erzeugen und **wie** dieses
mit dem Sicherheitszustand verbunden sein muss. Festgehalten als `P-34`
(HOCH) in `docs/Security.md`.

**Geltungsbereich:** Die Regeln gelten für **sicherheitsrelevante
Zustandswechsel in der lokalen Auth-DB**, nicht für jeden Vorgang, der eine
Session betrifft. Der freiwillige Einzel-Logout über `/session/revoke` ist
**ausdrücklich nicht** eingeschlossen; er bleibt nach `P-33` ein normaler
Session-Lebenszyklusvorgang **ohne** dauerhaftes Ereignis. Die genaue
Abgrenzung steht am Ende dieses Abschnitts.

**Verbindliche Sollsemantik:**

1. Ein **sicherheitsrelevanter lokaler Auth-DB-Zustandswechsel und sein
   erforderliches `security_events`-Ereignis bilden eine fachlich atomare
   Einheit.** Schlägt das Event-Schreiben fehl, muss die zugehörige
   Zustandsänderung zurückgerollt werden.
2. Dem Aufrufer darf **nur dann** ein Fehler gemeldet werden, wenn der
   Sicherheitszustand ebenfalls nicht committet wurde. Ein bereits
   vollzogener Zustandswechsel darf **nicht** nachträglich als vollständig
   fehlgeschlagen dargestellt werden — insbesondere darf ein verbrauchter
   Single-Use-Code nicht als fehlgeschlagene Anmeldung mit unverbrauchtem
   Code erscheinen.
3. Mehrteilige Vorgänge wie der **2FA-Reset** (neues Secret, neue
   Recovery-Codes, 2FA aktivieren, Sessions widerrufen, Trusted Devices
   widerrufen) sind **ein fachlicher Vorgang** und müssen entweder
   vollständig wirksam oder vollständig unwirksam sein. Es darf kein
   Zwischenzustand entstehen, in dem 2FA aktiviert ist, die Sessions aber
   nicht widerrufen wurden.
4. Das Event gehört dem **fachlichen Verbund**. **Generische Hilfsmethoden
   bestimmen keine eigenen fachlichen Eventtypen.** `RevokeAllSessions`
   und `RevokeAllTrustedDevices` werden von mehreren fachlichen Vorgängen
   genutzt und dürfen deshalb **nicht** selbstständig einen Anlass erfinden;
   der Anlass wird vom aufrufenden fachlichen Vorgang festgelegt und
   innerhalb dessen Transaktion geschrieben.
5. Vorgänge, die **außerhalb** der lokalen Auth-DB wirken, sind davon
   getrennt zu behandeln und hier **nicht** festgelegt.
6. **Keine sensiblen Eventfelder:** Das Schema von `security_events` bleibt
   unverändert. Es werden **keine** neuen Spalten, **kein** Token, **kein**
   Token-Hash, **keine** Session-ID, **keine** Roh-IP und **keine** weitere
   personenbezogene Information ergänzt.
7. Eventfehler werden **strukturiert protokolliert**, ohne Token,
   Session-ID, Recovery-Code, TOTP-Secret, Roh-IP oder sensible Daten.

**Bereits regelkonform** und damit Referenz für die Sollsemantik:
Passwortänderung, Passwort-Reset/Recovery und die Bestätigung eines Trusted
Device schreiben Zustand und Ereignis bereits in derselben Transaktion.

### Abgrenzung: zwei verschiedene Arten von Session-Widerruf

**1. Sicherheitsrelevanter accountweiter Widerruf — gehört in den
fachlichen Verbund.** Ein accountweiter Session-Widerruf ist Bestandteil
eines **sicherheitsrelevanten Gesamtvorgangs** und wird zusammen mit
Zustandsänderung und Ereignis **innerhalb derselben fachlichen Einheit**
behandelt. Das zugehörige Ereignis beschreibt dabei **den fachlichen
Gesamtvorgang**, nicht den Widerruf als solchen:

* **Passwortänderung** — Widerruf und `password_changed` bereits atomar.
* **Passwort-Reset/Recovery** — Widerruf und `password_reset` bereits atomar.
* **2FA-Reset** — Widerruf ist Teil des Reset-Vorgangs; derzeit **nicht**
  atomar, siehe `P-34`.
* **2FA-Deaktivierung** — widerruft im bestehenden Pfad **keine Sessions**
  (`src/api/twofactor.go:191-193`: „sessions remain, per the revocation
  policy“), sondern nur Trusted Devices. Sie ist deshalb hier **nicht** als
  Session-Widerrufsvorgang aufgeführt; für die Deaktivierung selbst gilt
  Regel 1 mit `two_factor_disabled`.

Generische Hilfsmethoden wie `RevokeAllSessions` bestimmen **keinen** eigenen
Anlass und **keinen** eigenen Eventtyp. **Ein eigener Eventtyp für einen
Session-Widerruf wird nicht gefordert.**

**2. Freiwilliger Einzel-Logout — ausdrücklich nicht Gegenstand.** Aus dem
aktuellen Code gilt unverändert: `/session/revoke` wird **ausschließlich** vom
freiwilligen Logout des Login-Dienstes genutzt; der Besitz des präsentierten
Tokens autorisiert ihn. Dafür ist nach `P-33` **kein** dauerhafter
`security_events`-Eintrag erforderlich. Der freiwillige Logout ist **kein**
Bestandteil von `P-34`, und `P-34` verlangt für ihn **weder** ein Ereignis
**noch** einen Eventtyp `session_revoked`. `P-34` fordert damit **nicht**, dass
jeder Session-Widerruf ein atomar zu protokollierendes Sicherheitsereignis
wird: Erforderlich sind nur die Sicherheitsvorgänge unter Ziffer 1.

### Umsetzungsstand (Commit `1eb38a7b33e0fda9ff1cf61640bc46b4bcb55a82`)

Die oben festgelegte Sollsemantik ist umgesetzt und in `docs/Security.md`
als `P-34` mit `ERLEDIGT` abgeschlossen. Die Regeln 1 bis 7 bleiben
unverändert **in Kraft**; der folgende Abschnitt beschreibt nur, wie sie
im Code umgesetzt sind.

**Die sechs betroffenen Vorgänge, jeweils genau eine Transaktion:**

| Vorgang | Transaktion in `src/api/store.go` | Ereignis (letzter Schritt) |
|---|---|---|
| `UseRecoveryCode` | bedingtes `UPDATE ... AND used_at IS NULL` (`:1563`) | `recovery_code_used` (`:1585`) |
| `RevokeTrustedDevice` | `DELETE ... WHERE account_id = ? AND token_hash = ?` (`:1477`) | `device_revoked` (`:1498`) |
| 2FA-Setup | Gate, Secret, Codes, Aktivierung (`:1122`) | `two_factor_enabled` (`:1150`) |
| 2FA-Aktivierung | bedingtes `UPDATE ... AND two_factor_enabled = 0` (`:1165`) | `two_factor_enabled` (`:1189`) |
| 2FA-Deaktivierung | bedingtes `UPDATE ... AND two_factor_enabled = 1`, Secret, Codes, Devices (`:1206`) | `two_factor_disabled` (`:1240`) |
| 2FA-Reset | CAS, Secret, Codes, Aktivierung, Sessions, Devices (`:1267`) | `two_factor_reset` (`:1312`) |

Das Ereignis ist in **allen sechs** Vorgängen der letzte fachliche
Schreibschritt vor dem Commit; `insertSecurityEventTx` (`:1035`) hat genau
sechs Aufrufer. Ein Ereignisfehler rollt den gesamten Vorgang zurück, und
ein Fehler wird **nicht mehr** gemeldet, nachdem der Sicherheitszustand
bereits committet wurde. Die generischen Einzelmethoden bleiben ohne
eigenes Ereignis; die Verbünde verwenden die tx-Varianten
`revokeAllSessionsTx` (`:1095`) und `revokeAllTrustedDevicesTx` (`:1106`).

**Keine** Schemaänderung, **keine** Migration, **keine** neue Abhängigkeit
und **keine** neue öffentliche API wurden eingeführt.

### Die CAS-Grenze des 2FA-Resets

Das erste Statement der Reset-Transaktion ist ein Compare-and-Swap auf dem
verschlüsselten `two_factor_secret`, das der Handler beim Requestbeginn
gelesen hat (`src/api/store.go:1274-1278`):

``` sql
UPDATE accounts
   SET two_factor_secret = ?, last_totp_counter = NULL, last_totp_at = NULL
 WHERE id = ? AND two_factor_secret <=> ?
```

`<=>` ist der NULL-sichere Gleichheitsoperator, damit der CAS auch greift,
solange `two_factor_secret` noch `NULL` ist. Der Erwartungswert stammt aus
dem **bereits vorhandenen** `Account.TwoFactorSecret`; es wurde **keine**
Versionsspalte und **keine** neue Tabelle eingeführt.

* **Gleicher gelesener Ausgangszustand:** Zwei Anfragen, die **denselben**
  gespeicherten Secret-Zustand gelesen haben, können **nicht beide**
  committen. Nur die Erste erhält `RowsAffected == 1`; die Zweite erhält
  `0`, schreibt **nichts** — kein Secret, keine Codes, kein
  Session-Widerruf, kein Device-Widerruf, kein Ereignis — und endet mit
  Konflikt.
* **Neuer Ausgangszustand:** Eine Anfrage, die **nach** dem vorherigen
  Commit startet, liest das bereits rotierte Secret. Sie ist ein **neuer
  gültiger Reset**, darf committen und erzeugt ein eigenes
  `two_factor_reset`-Ereignis.
* **Letzter Commit gewinnt:** Nach Commit B sind **ausschließlich** die
  Codes von B aktuell. Die von A ausgegebenen Codes können durch B
  **regulär** ungültig werden — auch dann, wenn Response A noch nicht
  beim Client eingegangen oder verarbeitet ist. Das ist die normale
  Semantik zweier erfolgreicher Reset-Vorgänge.
* **Garantiegrenze:** Der CAS schützt **ausschließlich** den gelesenen
  Ausgangszustand. Er ist **kein** Schutz gegen einen späteren Reset auf
  Basis des bereits neuen Zustands und **keine** Garantie über die Dauer
  oder Reihenfolge der HTTP-Antwortübertragung. Die
  Auslieferungsreihenfolge ist **kein** Datenbankbeweis.

### Neutrale Konfliktantwort

Der CAS-Verlierer erhält HTTP `409` mit exakt der Meldung
`two-factor state changed; retry` (`src/api/twofactor.go:273`). Die Meldung
behauptet **nicht**, dass ein Reset noch laufe, und nennt weder Secret noch
Codes noch den Gewinner. Der Konfliktfall liefert **weder**
Provisioning-URI **noch** Recovery-Codes. Ein Konflikt ist **kein** Fehler:
Er entsteht nur, wenn ein **anderer** Vorgang den Zustand bereits
verändert hat, und ein erneuter Versuch ist der vorgesehene Weg.

### Erhaltene Abgrenzungen

* Der **freiwillige Einzel-Logout** `/session/revoke` bleibt unverändert
  und ohne dauerhaftes Ereignis; es existiert **keine** Konstante
  `session_revoked` im Code.
* Die **best-effort-Parental-Pfade** bleiben unverändert und sind weiterhin
  unter `P-35` geregelt. `src/api/parental_handlers.go` war **nicht** Teil
  des **P-34**-Umsetzungscommits `1eb38a7b33e0fda9ff1cf61640bc46b4bcb55a82`;
  die getrennte P-35-Sichtbarmachung erfolgte später in
  `b9622edad914a53fb61c5bec2cb3b0ee9f3abc79`.
*
**MariaDB-/InnoDB-Integrationsgrenze:** Die Umsetzung ist über den
FakeStore **logisch** verifiziert, die echte InnoDB-Atomarität des
Reset-Verbunds (17 Datenbank-Statements, 18 Operationen inklusive Commit),
ein echter Event-Insert-Fehler, `RowsAffected`, das NULL-sichere `<=>`,
parallele Recovery-Code-Verwendung, Sperr-, Deadlock- und Lock-Wait-
Verhalten, Massenupdates, Commitfehler und das 5-Sekunden-DB-Zeitbudget
sind **nicht** gegen eine laufende Datenbank geprüft. Diese Grenzen sind
in `docs/Security.md` unter `P-34` als Abschlussgrenze festgehalten.

**Weitere Abgrenzungen:** `P-35` ist **abgeschlossen** und unter
`docs/Security.md` vollständig dokumentiert. Es protokolliert fehlgeschlagene
best-effort-Historien-Writes als strukturierte Warnung, führt aber **keine**
`P-34`-Atomarität ein: der autoritative Parental-Zustand und die erfolgreiche
Clientantwort bleiben bestehen, und beide Historienziele bleiben getrennt.
Verlorene Einträge werden **sichtbar, nicht verhindert**. Dieser Abschnitt legt
**keine** Go-Funktion, **keine** SQL-Transaktion und **keine** neue API fest.
