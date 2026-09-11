# Andora Agent (src/agent)

Lokaler Verwaltungsdaemon für Andora-Komponenten auf jedem Server.
Der Agent stellt eine versionierte Management-API bereit, über die das
zentrale Panel (oder das lokale Monitoring-Panel) Start, Stopp,
Neustart, Status, Healthchecks, Logs und Versionen abfragen und
steuern kann.

## Status

Debug-Build / Zwischenlösung. Authentifizierung über Token (shared
secret). mTLS ist architektonisch vorbereitet (CA- und
Client-Zertifikats-Konfiguration), aber noch nicht aktiviert.

## Architektur

- Go-Modul `andora/agent`, `package main`, `go 1.25.0`.
- Nur Standardbibliothek, keine externen Abhängigkeiten.
- Fail-closed bei Authentifizierungs- und Konfigurationsfehlern.
- Nur whitelisted systemctl/journalctl-Befehle (konfigurationsgesteuert).
- Keine Remote-Shell, keine generischen Befehle.
- Testbare `Controller`-Abstraktion mit austauschbarem `Runner`.

## Konfiguration

Über EnvironmentFile (`config.env`), direkt neben dem Binary oder per
`AGENT_CONFIG`-Umgebungsvariable oder CLI-Argument.

```
./agent /path/to/config.env
```

Siehe `config.env.example` für alle Keys.

### Verwaltete Dienste

Dienste werden über `AGENT_SERVICES` (komma-getrennte Keys) und
pro-Key `AGENT_SERVICE_<KEY>_UNIT` / `_URL` / `_HEALTH_PATH`
konfiguriert. Beispiele: `realm` (andora-realm.service),
`coordinator` (andora-coordinator.service), `auth` (andora-auth.service),
`login` (andora-login.service).

## API

| Endpunkt | Methode | Auth | Beschreibung |
|---|---|---|---|
| `/health` | GET | — | Agent-Liveness (`{"status":"ok"}`) |
| `/status` | GET | — | Agent-Status + Versionsinfo |
| `/api/v1/services` | GET | ✓ | Liste aller verwalteten Dienste |
| `/api/v1/services/{key}` | GET | ✓ | Detail (State, Health, Version) |
| `/api/v1/services/{key}/start` | POST | ✓ | Start via systemctl |
| `/api/v1/services/{key}/stop` | POST | ✓ | Stop via systemctl |
| `/api/v1/services/{key}/restart` | POST | ✓ | Restart via systemctl |
| `/api/v1/services/{key}/logs?lines=N` | GET | ✓ | Journal-Logs (begrenzt) |
| `/api/v1/services/{key}/health` | GET | ✓ | Health-Probe |
| `/api/v1/services/{key}/version` | GET | ✓ | Versionsabfrage |

### Authentifizierung

Token über einen der drei Wege:
- `X-Andora-Token: <token>` Header
- `x-api-token: <token>` Header
- `?token=<token>` Query-Parameter

Constant-time Vergleich. Bei leerem `AGENT_TOKEN` (Konfigurationsfehler)
verweigert der Agent alle Management-Requests.

## Tests

```bash
cd src/agent
go test -v ./...
go build .
go vet ./...
```

## Deployment

Vorlagen unter `deploy/`:
- `deploy/systemd/andora-agent.service` — systemd-Unit
- `deploy/conf/agent.conf` — EnvironmentFile-Vorlage
- `deploy/sudoers/andora-agent` — NOPASSWD sudo-Regel für whitelistete Units

Produktionsbetrieb: User `andora`, WorkingDirectory
`/opt/andora/agent`, EnvironmentFile `/opt/andora/agent/config.env`.

## Sicherheitsmodell

- Keine Shell, keine Remote-Shell.
- Nur konfigurierte Units werden über systemctl/journalctl verwaltet.
- Start/Stop/Restart erfordern `sudo -n` (NOPASSWD-Sudoers-Regel
  für genau die konfigurierten Units).
- `is-active` und `systemctl show` ohne sudo (read-only).
- Logs: `journalctl -u <unit>` mit fester Zeilen-/Byte-Obergrenze.
- Health-Proben nur auf konfigurierte URLs.
- Fail-closed: unbekannter Service → 404, ungültige Anfrage → 400.
