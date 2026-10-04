# Auth/API-Service (Go)

Der zentrale Andora-Service. Er ist der **einzige** Dienst mit direktem
Zugriff auf die Auth-Datenbank (`auth`) und stellt fachliche
API-Operationen bereit — niemals eine generische SQL-Schnittstelle.

> Implementierung: `docs/Auth_API_Architektur.md` (Architektur),
> `docs/Datenbank_Architektur.md` (DB-Einstufung).

## Build & Start

```sh
go build -o build/authapi .
./build/authapi [path/to/config.env]    # oder AUTHAPI_CONFIG=/pfad
```

Startvoraussetzungen (sonst Exit 1, kein Start):

- gültige `config.env` (siehe `config.env.example`)
- erreichbare Auth-DB mit funktionierenden Credentials
- `ENCRYPTION_KEY` gesetzt (32 Bytes hex)
- Schema aktuell: aufgestandene Migrations werden beim Start
  automatisch angewendet (`db_version`); schlägt eine fehl, kein Start.

## Authentifizierung jedes Aufrufs

Jeder Consuming-Service ruft die API nur mit **eigenen** Credentials auf
(das `SERVICE_<name>_ID/_SECRET/_PERMISSIONS`-Tupel in `config.env`)
— es gibt kein gemeinsames Secret. Request-Header:

```
X-Andora-Service:  <service id>
X-Andora-Timestamp: <unix-seconds>          (Fenster ±90 s)
X-Andora-Signature: hex(HMAC-SHA256(secret, payload))
```

mit

```
payload = METHOD \n PATH \n RAW_QUERY \n TIMESTAMP \n SHA256(BODY)
```

Ein fehlender/gekürzter Körper wird abgelehnt (`maxBodyBytes` = 64 KiB).
Ein Service erhält pro Endpoint 403, dessen Berechtigung ihm fehlt.
`/health` und `/status` sind offen (keine Signatur, keine Account-Daten).

## Endpoints

| Method | Path | Permission | Ergebnis (kurz) |
|---|---|---|---|
| POST | `/auth/verify` | `account.authenticate` | `{valid, account_id, session_id, expires_at, device_token?}` — nur bei Erfolg wird die Session angelegt und `last_login_at` gesetzt; alle Ablehnungen (kein Account, Ban, falsches PW, verbrauchter Recovery-Code) sind unverwechselbar `{valid:false}`. Optional: `device_token` (bestätigtes Gerät → nur PW nötig), `totp_code`/`recovery_code` (2. Faktor bei unbekanntem Gerät, → `{valid:false, two_factor_required:true}` wenn fehlt/falsch/Replay),   `trust_device`+`device_label` bestätigt das Gerät (max 3, sonst 409 `max_devices_reached`; nach 30 Tagen Inaktivität verfällt das Gerät und der 2FA-Bypass entfällt) |
| POST | `/account/register` | `account.register` | 201 `{account_id}` — Username `3–32 [A-Za-z0-9_]`, PW `8–72`, E-Mail konservativ validiert; Duplikat (Username oder E-Mail-Hash) → 409; frischer Account wird `NEW_ACCOUNT_BAN_SECONDS` gebannt |
| POST | `/account/password/change` | `account.password_change` | `{changed}` — altes PW muss verifizieren; kein Account → 404. Revokiert in einer Transaktion alle Sessions + trusted devices (Event `password_changed`) |
| POST | `/account/recovery/request` | `account.recovery` | `{recovery_token, expires_at}` — einmalig gültiger Token, `RECOVERY_TTL_MINUTES` |
| POST | `/account/recovery/confirm` | `account.recovery` | `{recovered}` — setztes neues PW, klärt den Ban und verbraucht den Token in einer Transaktion |
| POST | `/account/permissions` | `account.permissions` | `{permissions: [...]}` — pro Account freigegebene Berechtigungen (geschlossene Menge) |
| POST | `/session/create` | `session.create` | `{session_id, expires_at}` — nur für nicht-gebannte Accounts |
| POST | `/session/validate` | `session.validate` | `{valid, account_id, expires_at}` |
| POST | `/session/revoke` | `session.revoke` | `{revoked}` |
| GET/POST | `/realms` | `realm.list` | `{realms: [...]}` — nur Betriebsdaten (Name, Sprache, Region, Status) |
| POST | `/handoff/create` | `handoff.create` | `{handoff_token, expires_at}` — bindet Account→Realm, `HANDOFF_TTL_SECONDS` |
| POST | `/handoff/validate` | `handoff.validate` | `{valid, account_id, realm_id}` — validiert **und** verbraucht das Token (zweiter Aufruf → `{valid:false}`) |
| POST | `/world/authenticate` | `world.authenticate` | **LEGACY** (kein separater Worldserver mehr) — `{valid, realm_id, name}`, nur kompatibel erhalten |
| POST | `/world/heartbeat` | `world.heartbeat` | **LEGACY** (kein separater Worldserver mehr) — `{recorded}`, nur kompatibel erhalten |
| POST | `/twofactor/status` | `account.two_factor` | `{enabled, recovery_codes_available}` — 2FA-Zustand ohne geheime Daten |
| POST | `/twofactor/setup` | `account.two_factor` | `{enabled, provisioning_uri, recovery_codes[10]}` — aktiviert 2FA, speichert den verschlüsselten TOTP-Secret, erzeugt 10 einmalige Recovery-Codes; bereits aktiviert → 409 |
| POST | `/twofactor/enable` | `account.two_factor` | `{enabled}` — schaltet 2FA an, behält Secret/Codes |
| POST | `/twofactor/disable` | `account.two_factor` | `{enabled:false}` — löscht Secret + Codes + trusted devices, **Sessions bleiben** |
| POST | `/twofactor/reset` | `account.two_factor` | `{enabled, provisioning_uri, recovery_codes[10]}` — neues Secret + Codes, revokiert Sessions und trusted devices |
| POST | `/devices/list` | `device.list` | `{devices:[{id,label,confirmed_at}]}` — keine token_hash/Secrets |
| POST | `/devices/revoke` | `device.revoke` | `{revoked}` — widerruft ein bestätigtes Gerät; unbekannt → 404 |
| POST | `/security/events` | `security.events` | `{events:[{id,event_type,created_at}]}` — Sicherheits-Log des Accounts (neueste zuerst) |
| GET | `/status` | — (offen) | `{status, uptime, sessions:{…}}` — `sessions` ist das read-only Monitoring (P-33) |
| GET | `/health` | — (offen) | `{status}` |

## Session-Monitoring auf `/status` (`P-33`)

`/status` bleibt offen und liefert zusätzlich den Block `sessions`.
Ausgegeben werden **ausschließlich aggregierte** Werte: Zähler, ein Alter in
Tagen, eine Netto-Bestandsänderung, InnoDB-**Schätzwerte** und Laufzeiten.
Kein Token, kein Token-Hash, keine Session-ID, keine `account_id`, kein
Benutzername, keine Einzelzeile und **kein Rohfehler der Datenbank** — die
Fehlerausgabe ist ein neutraler Zustand, der Text geht nur ins Log.

Ablauf: Die Erhebung ist **bedarfsgetriggert** über `/status`. Der Handler
 rendert zuerst den Cache und **wartet nie** auf die Datenbank; danach wird
höchstens **ein** Worker für den Versuch zugelassen (ein Versuch zurzeit,
kein Ticker, keine History, keine unbegrenzte Goroutine-Vermehrung). Vor der
ersten erfolgreichen Erhebung sind alle Zähler `null` (unbekannt), niemals `0`.
Bei Fehler bleibt der letzte erfolgreiche Stand erhalten; `state`, `in_flight`,
`collected_at`, `last_attempt_at`, `last_attempt_ok`, `age_seconds` und
`attempts` unterscheiden „noch nie erhoben", „läuft", „erfolgt", „veraltet"
und „fehlerhaft". Metadatenfehler (`size_state`) machen erfolgreiche Zähler
nicht ungültig; `no_access` erscheint nur bei tatsächlich erkannbarer
Berechtigungsverweigerung, eine fehlende Metadatenzeile bleibt `unknown`.

`total_delta` ist die **Netto-Bestandsänderung** gegenüber dem letzten
erfolgreichen Stand (keine Neuanlagen); `delta_window_seconds` ist das
tatsächlich verstrichene Fenster. `session_status_batch_*` beschreibt den
produktiven `BatchSessionStatus`-DB-Aufruf, **nicht** die HTTP-Latenz; das
Maximum gilt prozessseitig seit Start.

`stale_after_seconds` ist das **Alter, ab dem ein Stand als veraltet gilt**,
und zwar **dieselbe** Schwelle, nach der auch der Zustand `stale` vergeben
wird: das Doppelte des wirksamen Mindestabstands, ausgegeben in Sekunden.
Definition und Ausgabe stammen aus **einer** Quelle, können also nicht
auseinanderlaufen. Das Feld ist eine **reine Darstellungskonvention** dieser
lesenden Sicht: **keine** Aufbewahrungsfrist, **keine** Aktionsschwelle und
**kein** Löschsignal. Solange `stale` gilt, bleibt der letzte erfolgreiche
Stand sichtbar und nutzbar; es wird nichts gelöscht, widerrufen oder
zurückgesetzt.

Konfiguration in `config.env`:

| Variable | Default | Einheit | Bedeutung |
|---|---|---|---|
| `SESSION_STATS_ENABLED` | `true` | bool | Abschaltung; der Zustand `disabled` wird ausgegeben |
| `SESSION_STATS_MIN_INTERVAL_SECS` | `60` | Sekunden | Mindestabstand zwischen zwei Versuchen (ab Versuchsbeginn, also auch nach Fehlern) |
| `SESSION_STATS_TIMEOUT_MS` | `5000` | Millisekunden | Timeout eines **gesamten** Versuchs |

**Auswertung der Werte (wie implementiert):** `SESSION_STATS_ENABLED` wird als
Bool gelesen. Nicht gesetzt, leer oder nicht parsbar bedeutet **Default**, also
**`true`** — die Erhebung bleibt dann **an**. `1`, `true`, `yes` und `on`
(jeweils ohne Beachtung der Groß-/Kleinschreibung) bedeuten `true`; `0`,
`false`, `no` und `off` bedeuten `false`. Jeder **andere** Wert (z. B. `ja`,
`enabled`, `2`) ist **nicht** als Fehler erkennbar und fällt deshalb auf den
Default `true` zurück; es wird **keine** Warnung ausgegeben. Für die beiden
Zahlenwerte gilt: nicht parsbar oder `<= 0` → der jeweilige Default
(`60` Sekunden bzw. `5000` Millisekunden). Die wirksamen Werte werden
zusätzlich als `min_interval_seconds` ausgegeben.

Das Monitoring ist **rein lesend**: es löscht nichts, widerruft nichts und legt
keine Aufbewahrungsfrist fest.

**Shutdown-Verhalten:** `close()` sperrt die Zulassung sofort und unter
derselben Sperre, bricht einen laufenden Versuch über dessen Kontext ab und
wartet ihn mit einer **begrenzten** Grace-Zeit aus, bevor der Pool geschlossen
wird. Ein Store, der den **Kontextabbruch nicht beachtet**, wird dadurch
**nicht** zwangsweise beendet: Die Goroutine läuft weiter, das Warten endet
nach der Grace-Zeit und der Pool wird geschlossen. Ein wiederholtes `close()`
erzeugt **keine** weiteren Versuche, **keinen** weiteren Abschluss und **keinen**
zweiten Pool-Schluss.

## Datenminimierung

- Account-Suche über `username` **oder** den nicht reversiblen
  E-Mail-Lookup-Hash (`email_lookup_hash`, BINARY(32)) — nie die E-Mail
  selbst.
- Session-/Handoff-/Recovery-Tokens liegen in der DB nur als SHA-256
  (64-hex); der rohe Token wird nur einmal an den Caller rausgegeben.
- E-Mail-Adresse: nur verschlüsselt (`AES-256-GCM`, Schlüssel `ENCRYPTION_KEY`)
  plus Lookup-Hash. Ein DB-Dump liefert keine brauchbaren Credentials.
- Antworten enthalten nur das Nötige; `password_hash`, `email_*`,
  `ban_until` etc. fließen nie über die API ab.

## Sicherheits-Entscheidungen

- **Rate-Limit** zweigeteilt: vor der Dienst-Authentifizierung nach
  `IP+Pfad`, danach nach `ServiceId+Pfad` (Burst `RATE_LIMIT_BURST`,
  `RATE_LIMIT_PER_MIN`/Minute, 429 + `Retry-After`). `X-Forwarded-For`
  wird **bewusst nicht** vertraut (Spoofing).
- **Trusted-Device-Verfall (30 Tage Inaktivität)**: Die Gültigkeitslogik
  lebt ausschließlich in `trusted_devices.last_used_at` (kein Ablauf-
  Feld in `accounts`; der Account verfällt dadurch nicht). Jeder
  erfolgreiche Login mit einem erkannten Gerät aktualisiert
  `last_used_at`; nach 30 Tagen Inaktivität wird das Gerät nicht mehr
  erkannt (der 2FA-Bypass entfällt und das Gerät verlangt erneut den
  zweiten Faktor) und befreit beim nächsten Geräte-Flow seinen Platz im
  3er-Limit. Bestätigte Geräte erhalten bei der Bestätigung
  `last_used_at = confirmed_at`.
- **Password-Hash**: Argon2id (`x/crypto`), Parameter im Format des
  Hashes selbst, Salt pro Hash zufällig (`HashPasswordSalt`).
- **World-Server-Credentials**: als Secret in `world_servers.credential`
  gespeichert (nur dieser Service liest die Tabelle; constant-time
  Vergleich). Rotierung = Update der Zeile.
- **Recovery-Mail-Lücke**: Der Service hat keine Mail-Engine —
  der Calling-Service (z. B. Login-Service) holt den Token
  selbst und stellt ihn dem Account zu. Diese Lücke wird explizit
  dokumentiert, nicht still verklebt.
- **TLS optional**: `TLS_CERT_FILE` + `TLS_KEY_FILE` aktivieren TLS
  (min. TLS 1.2); sonst plain HTTP.
- **Logging**: eine Zeile pro Request (Method, Pfad, Status, Service-Id,
  Dauer) — keine Bodies, Signatur oder Secrets.
- **Cache-Control**: `no-store, no-cache` auf allen Antworten.

## Migrations

`db/auth/migrations/` (nummerniert: `NNN_name.sql`), Runner bei Start:

1. `db_version` anlegen, angreifbare Versionen laden
2. ausstehende Migrations sortiert ausführen
3. jede angewendete Version in `db_version` eintragen

Bereits angewendete Migrations werden nie modifiziert; Änderungen kommen
als neue Nummer. Ohne laufende DB **kein** Start.

## Tests

`go test ./...` — DB-freie Integrationstests über eine `AuthStore`-Fake
(alle Endpoints, Statuscodes, Einmalnutzung von Handoff/Recovery,
Session-Expire) plus Argon-/Config-/Signatur-Tests (`verify_test.go`).
`twofactor_test.go` deckt TOTP-2FA + trusted devices ab: korrekter/falscher/
Replay-TOTP, Geräte-Bestätigung/Widerruf, Max-3-Limit (409), einmalige
Recovery-Codes, disable/reset und Passwortänderung (Revocation-Policy).
`trusted_expiry_test.go` deckt den 30-Tage-Inaktivitätsverfall ab:
Grenzenwert des Gültigkeits-Checkers, Verfall unterbindet den 2FA-Bypass
(Account bleibt per TOTP einlogbar), `last_used_at`-Refresh bei Login,
Slot-Befreiung im 3er-Limit nach Ablauf und Re-Bestätigung desselben
Tokens. Die echte DB wird nicht für Tests benötigt.
