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
| POST | `/auth/verify` | `account.authenticate` | `{valid, account_id, session_id, expires_at}` — nur bei Erfolg wird die Session angelegt und `last_login_at` gesetzt; alle Ablehnungen (kein Account, Ban, falsches PW) sind unverwechselbar `{valid:false}` |
| POST | `/account/register` | `account.register` | 201 `{account_id}` — Username `3–32 [A-Za-z0-9_]`, PW `8–72`, E-Mail konservativ validiert; Duplikat (Username oder E-Mail-Hash) → 409; frischer Account wird `NEW_ACCOUNT_BAN_SECONDS` gebannt |
| POST | `/account/password/change` | `account.password_change` | `{changed}` — altes PW muss verifizieren; kein Account → 404 |
| POST | `/account/recovery/request` | `account.recovery` | `{recovery_token, expires_at}` — einmalig gültiger Token, `RECOVERY_TTL_MINUTES` |
| POST | `/account/recovery/confirm` | `account.recovery` | `{recovered}` — setztes neues PW, klärt den Ban und verbraucht den Token in einer Transaktion |
| POST | `/account/permissions` | `account.permissions` | `{permissions: [...]}` — pro Account freigegebene Berechtigungen (geschlossene Menge) |
| POST | `/session/create` | `session.create` | `{session_id, expires_at}` — nur für nicht-gebannte Accounts |
| POST | `/session/validate` | `session.validate` | `{valid, account_id, expires_at}` |
| POST | `/session/revoke` | `session.revoke` | `{revoked}` |
| GET/POST | `/realms` | `realm.list` | `{realms: [...]}` — nur Betriebsdaten (Name, Sprache, Region, Status) |
| POST | `/handoff/create` | `handoff.create` | `{handoff_token, expires_at}` — bindet Account→Realm, `HANDOFF_TTL_SECONDS` |
| POST | `/handoff/validate` | `handoff.validate` | `{valid, account_id, realm_id}` — validiert **und** verbraucht das Token (zweiter Aufruf → `{valid:false}`) |
| POST | `/world/authenticate` | `world.authenticate` | `{valid, realm_id, name}` — constant-time Abgleich des World-Server-Secrets |
| POST | `/world/heartbeat` | `world.heartbeat` | `{recorded}` — erneut authentifiziert und schreibt `last_heartbeat`, `status` (online/offline), `version`, `current_players` |
| GET | `/status` | — (offen) | `{status, uptime}` |
| GET | `/health` | — (offen) | `{status}` |

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
Die echte DB wird nicht für Tests benötigt.
