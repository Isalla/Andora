# Login-Service (Go)

Separater Login-Dienst der Zielkette `Auth/API → Login → Realm`
(siehe `docs/Login_Realm_Architektur.md`, `docs/architecture.md`).

Der Service besitzt **keine Datenbank-Zugangsdaten** und **keinen
direkten DB-Zugriff**. Er bedient den Spielclient (HTTP+JSON) und
spricht für alle Account-Funktionen ausschließlich mit dem
Auth/API-Service — mit seiner **eigenen** Service-Credential
(`login-service`).

Benötigte Auth-API-Berechtigungen:

```text
account.authenticate, session.validate, session.revoke,
realm.list, handoff.create
```

## Ablauf

```text
Client --POST /login--> Login --POST /auth/verify--> Auth/API
Client <--{valid, account_id, session_id, ...}-- Login
Client --GET /realms?session_id=..--> Login --session/validate + /realms--> Auth/API
Client <--{realms: [{..., http_url}]}-- Login
Client --GET /characters?realm_id=..&session_id=..--> ausgewählter Realm
Realm --session/validate--> Auth/API; eigene RealmDB: id + name für diesen Account
Client wählt einen Namen (ID intern)
Client --POST /handoff {session_id, realm_id}--> Login --handoff/create--> Auth/API
Client <--{handoff_token, expires_at, ws_url}-- Login
Client --WebSocket(HELLO + handoff_token)--> Realm --handoff/validate--> Auth/API
```

Die `account_id` für `/handoff` stammt immer aus der validierten
Session, nie vom Client. Charakterdaten und Listenabruf gehören zum
Realm (Charakterdaten liegen in `realm_state_<realm>`). Der Listenabruf
ist vor HELLO implementiert; Charaktererstellung ist weiterhin ein Folgeschritt.

## Endpunkte

| Methode | Pfad | Beschreibung |
|---|---|---|
| GET | `/health` | Liveness (offen) |
| GET | `/status` | Liveness + Uptime (offen) |
| POST | `/login` | `{username\|email_lookup_hash, password, ...2FA-Felder}` → Antwortform wie `/auth/verify` |
| POST | `/logout` | `{session_id}` → `{revoked}` |
| GET/POST | `/realms` | `session_id` → `{realms: [...]}` inklusive je Realm `http_url` (nur bei gültiger Session) |
| POST | `/handoff` | `{session_id, realm_id}` → `{handoff_token, expires_at, realm_id, ws_url}` |

Fehler des Auth/API-Dienstes werden fail-closed als `503 auth
service unavailable` beantwortet (kein Detail-Leak). Ungültige
Sessions → `401`; unbekannter Realm → `404`; deaktivierter Realm → `403`.

## Konfiguration

`config.env` neben dem Binary (Pfad auch per Arg oder
`LOGIN_CONFIG`); Vorlage: `config.env.example`. Rate-Limit pro
IP+Pfad, No-Cache-Header und Request-Logging wie beim Auth-Service.
TLS optional (`TLS_CERT_FILE` + `TLS_KEY_FILE`, min. TLS 1.2).

`REALM_HTTP_URLS` ordnet Realm-IDs explizite, für den Client erreichbare HTTP-Basen
zu (z. B. `1=http://realm-host:3002`). Ohne Zuordnung wird `http_url:""` geliefert;
es gibt keine Portheuristik aus `REALM_WS_URLS`. Die Sessionprüfung und die
kontogebundene Liste erfolgen direkt beim ausgewählten Realm, ohne Login-DB-Zugriff
und ohne Service-Credential im Client. Vertrag siehe
`docs/Login_Realm_Architektur.md`, Abschnitt 20.

## Bauen/Testen

```bash
go build ./...
go test ./...
```

## Bewusste Folgeschritte (kein Bestandteil dieses Stands)

- Charaktererstellung je Realm (separat vom implementierten Listenabruf und HELLO).
- Realm-Regelprüfung (Fresh-Start, Transferberechtigung) beim
  Charaktertransfer — erst relevant, wenn Transfers implementiert sind.
- Realm-Liveness über Heartbeat hinaus (Login meidet deaktivierte /
  nicht gelistete Realms bereits heute).
