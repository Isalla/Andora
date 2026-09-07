# Monitoring-/Admin-Web-Panel für den Andora-Server

## Status

**Übergangs-/Legacy-Status:** Das unten beschriebene lokale Panel auf dem Gameserver-Host ist der **derzeit umgesetzte Stand** und wird von der neuen Betriebsarchitektur abgelöst.

Zusätzlich gibt es eine PHP-Implementierung des lokalen Panels unter `web/andora-monitor/` (derzeit fertiggestellt, siehe Dokumentation unten). Beide Varianten dienen als Übergang zur Zielarchitektur.

**Zielarchitektur:** Das Admin- und Deployment-Panel wird zentralisiert. Auf jedem verwalteten Andora-Server läuft ein eigener **Andora-Agent**. Die Kommunikation zwischen Panel und Agenten erfolgt über einen dedizierten Port mit **mTLS**, ohne generische Remote-Shell. Alle Andora-Komponenten laufen unter dem dedizierten Nicht-Root-Benutzer `andora`. Updates übernimmt der **`andora-updater`** (signierte Manifeste, Prüfsummen, Healthchecks, Rollback) für alle Dienste inklusive des Agenten selbst. Realm-Updates laufen automatisiert im Wartungsmodus ab.

Die verbindliche Beschreibung der Zielarchitektur steht in:

```text
docs/Deployment_Betriebsarchitektur.md
```

Der folgende Abschnitt beschreibt den bisherigen lokalen Panel-Stand (Übergang).

---

## Lokales Panel (derzeit umgesetzt, Übergangsstand)

Es gibt zwei lokale Panel-Implementierungen:

1. **PHP-Panel** (`web/andora-monitor`): Eine PHP-basierte Web-Oberfläche auf demselben Host wie der Gameserver, ausschließlich für den Admin (Spieler haben KEINEN Zugriff). Sie dient der Überwachung und Steuerung des Servers. Der PHP-Built-In-Server oder Apache/Nginx wird als Router genutzt. Sicherheit:
   - optional Token (`ANDORA_MONITOR_TOKEN`): bei gesetztem Token muss jeder Request davon betroffen sein (Header `x-api-token` oder `?token=`)
   - fail-closed: wenn Token konfiguriert ist und Header/Param fehlt/invalid → 401 Unauthorized
   - keine Secrets an den Client (nur Whitelist-Keys)
   - nur feste Aktionen via systemctl (start/stop/restart), nie Shell-Freheit
   - gefährliche Aktionen (restart/stop) erst nach expliziter Bestätigung (`confirm: true`)
   - History-Speicherung in `data/history.json` mit 5-Sekunden-Drosselung und echten Serverstatus-Werten
   - Control: fehlgeschlagene systemctl-Aktionen melden `ok=false` mit generischer Fehlermeldung, interne Fehler werden nur intern protokolliert

2. **Node-Panel** (`monitor/`): Legacy-Referenz (Node-Prozess). In Produktion
   wird es nicht mehr eingesetzt; der Betrieb läuft über das PHP-Panel.

Das PHP-Panel ist die aktuell umgesetzte und produktive Variante; das
Node-Panel bleibt als Legacy-Referenz erhalten (siehe `deploy/README.md`).

## Ports & Endpunkte

| Port | Prozess | Endpunkte |
|---|---|---|
| 3001 | Gameserver | WebSocket (Spieler) |
| 3002 | Gameserver | `GET /health`, `GET /status`, `GET /players` |
| 3003 | Panel | `GET /` Dashboard, `GET/POST /api/*` |
| 9443 | Andora-Agent | `GET /health`, `GET /status`, `GET /api/v1/services…` (Management-API, Token-Auth) |

Der **Andora-Agent** (`src/agent`) ist der lokale Verwaltungsdaemon
(Management-Port strikt getrennt von den Spiel-/Service-Ports). Er wird
über einen Token authentifiziert (`AGENT_TOKEN`, Zwischenlösung; mTLS ist
architektonisch vorbereitet) und steuert Start/Stop/Restart, Status,
Healths, Logs und Versionen nur für konfigurierte Andora-Units. Das Panel
nutzt ihn als bevorzugten Weg (`via: agent`), mit Legacy-Fallback auf
`sudo -n systemctl` (`via: systemd`), sobald `ANDORA_AGENT_URL` gesetzt ist.

Panel-Bindung: default `127.0.0.1:3003` (weder LAN noch public);
`ANDORA_MONITOR_BIND` wählt die Interfaces (`""`/`auto` = 127.0.0.1,
`ipv4`/`4` = 0.0.0.0, `ipv6`/`6` = nur IPv6, `dual`/`both` = dual-stack,
sonst IP-Literal oder Hostname); optional `ANDORA_MONITOR_TOKEN` (Header
`x-api-token` oder `?token=`). `ANDORA_GAME_SERVER_URL` darf IPv6-Literal
ungeklammert als `host:port` enthalten (wird zu `[host]:port` normalisiert).

## Metriken des Gameservers (`GET /status` auf 3002)

- `ok`, `server_up`, `uptime_s`
- `players`, `max_players` (Obergrenze aus der Architektur 40-70/Kanal)
- `zones` / `zone_count` (aktive Gebiete/Instanzen, aus `characters.zone_id`)
- `npc_active`, `instances` (aktuell 0 bzw. 1 – NPC-/Instanz-Systeme werden
  erst in späteren Milestones; Felder bestehen aber schon)
- `tick`: `interval_ms`, `last_ms`, `max_ms`, `avg_ms`, `count` (Tick-Dauer)
- `cpu_percent`, `heap_mb`, `rss_mb`
- `event_loop`: `current_lag_ms`, `avg_lag_ms`, `max_lag_ms` — Event-Loop-Blockade
  (Lag-Indikator), gemessen im gleitenden 1-Minuten-Fenster (60 Messwerte à 1 s,
  Ringpuffer — alte Ausreißer fallen automatisch raus, kein Array-Wachstum)

### Spielerdiagnose (`GET /players` auf 3002)

- `/status` liefert in `players` nur die **Anzahl** der Spieler (Zählung).
- Die Spielerliste kommt ausschließlich aus `GET /players`:
  `{ "ok": true, "players": [ { "id", "name", "zone_id", "ping_ms",
  "last_activity" } ] }` (kein `seconds_inactive`, kein `visible_entities`
  im derzeitigen Payload).
- `last_activity`: Node-Implementierung liefert Unix-Zeitstempel in ms,
  Rust-Implementierung verstrichene Sekunden (`elapsed().as_secs()`). Das
  PHP-Panel unterscheidet beides über den Zahlenbereich (>1e10 = ms).
- Keine Rangliste — nur Diagnosewerte.

## Funktionen des Panels

- Dashboard (ein HTML-File, kein Framework): Status, Auslastung, Spieler,
  Verlauf (~10 min, Datei-basiert, Poll 2 s), Steuerung, Config.
- **Steuerung**: Start / Stop / Restart — erst nach zweiter Bestätigung
  (POST `/api/control` mit `confirm: true`).
  - PHP-Implementierung: fehlgeschlagene `systemctl`-Aktionen melden `ok=false`
    mit generischer Fehlermeldung; interne Fehler werden nur intern protokolliert,
    nicht an den Client weitergegeben. Exit-Code wird geprüft (fail-closed).
  - **Agent-Pfad**: Ist `ANDORA_AGENT_URL` gesetzt, wird die Aktion zuerst über
    den Andora-Agent ausgeführt (POST `/api/v1/services/<key>/<action>`).
    Antwort enthält `via: "agent"` plus `state`/`active`. Schlägt der Agent fehl
    (nicht erreichbar/HTTP-Fehler), fällt das Panel auf `sudo -n systemctl`
    zurück (`via: "systemd"`) und meldet den Folgezustand der Unit.
  - `/api/status` liefert zusätzlich den Block `agent` mit
    `enabled`, `connected`, `key`, `unit`, `state`, `healthy`, `version`,
    `health_url` (nur wenn `ANDORA_AGENT_URL` gesetzt, sonst `enabled: false`).
- **Verlauf**: `GET /api/history` (Datei-basiert in `data/history.json`,
  max. 360 Punkte, 5-Sekunden-Drosselung anhand letzten Timestamp).
  - PHP-Implementierung: speichert echte Werte des abgefragten Serverstatus
    (online, players, cpu_percent, heap_mb, tick-Werte, loop_lag_ms). Es werden
    keine pauschalen `online=true`, `players=0` und alle Messwerte auf 0 gesetzt.
- **Config**: `GET /api/config` + `POST /api/config` — nur Whitelist-Keys von
  `src/realm/config.env` sind editierbar (`PORT_WS`, `PORT_HTTP`,
  `WS_BIND_HOST`, `HEALTH_BIND_HOST`, `TICK_MS`,
  `AOFB_RADIUS`, `RENDER_CAP_DEFAULT`, alle `OLLAMA_*` außer
  Secrets); `DB_*` und Passwörter sind **nicht** sichtbar und nicht
  änderbar. Werte werden serverseitig validiert (Port-Bereiche, Tick 16-1000
  ms, Sampling-Bereiche, URL-Format …).
- **Keine Secrets** an den Client: ausschließlich Whitelist-Werte.
- Token-Authentifizierung: wenn `ANDORA_MONITOR_TOKEN` gesetzt ist, müssen alle
  `GET/POST /api/*`-Requests den Token im Header (`x-api-token`) oder als
  Query-Parameter (`?token=`) übergeben werden (fail-closed). Das Root-Dashboard
  `/` erfordert ebenfalls den Token, wenn er konfiguriert ist.

## Geänderte / neue Dateien

### Neu
- `monitor/` — eigener Node-Prozess (nur stdlib, keine Dependencies, kein Build):
  - `package.json`, `config.js`
  - `server.js` (HTTP-API + Verlauf)
  - `lib/history.js` (In-Memory Ring-Buffer)
  - `lib/systemctl.js` (start/stop/restart via `sudo -n`, is-active ohne sudo)
  - `lib/envconfig.js` (lesen/validieren/schreiben von `config.env`)
  - `public/index.html` (Dashboard)
- `src/realm/src/metrics.ts` — Tick-Statistik + CPU/Heap/RSS
- `src/realm/src/events.ts` — Event-Loop-Lag-Messung

- `web/andora-monitor/` — PHP-Implementierung des lokalen Monitoring-Admin-Panels:
  - `public/index.php` — Front-Controller, Routing, API-Endpunkte
  - `lib/config.php` — Laden der Panel-Konfiguration
  - `lib/envconfig.php` — Lese-/Schreib-Logik für `config.env` (Whitelist-basiert,
    validateChanges, writeChanges, read_env_file)
  - `lib/agent.php` — HTTP-Client für den Andora-Agent (`ANDORA_AGENT_URL`,
    `ANDORA_AGENT_TOKEN`, `ANDORA_AGENT_SERVICE_KEY`, `ANDORA_AGENT_TIMEOUT_MS`);
    Status-/List-/Action-Helfer, `via: agent|systemd`
  - `data/` — History-Datenverzeichnis (history.json, Datei-basiert, CAP 360)
  - `config.php` — Projektkonfiguration (Umgebungsvariablen, sensible Defaults)
  - `public/index.html` — Dashboard-HTML
- `src/agent/` — Andora-Agent (Go, `package main`, Stdlib-only, Debug-Build):
  - `config.go`, `bind.go`, `urlutil.go`, `controller.go`, `security.go`,
    `server.go` — Konfiguration (fail-closed, Token-Pflicht, Services-Registry),
    dual-stack Listener, Controller-Abstraktion (systemctl/journalctl via `sudo -n`,
    Health/Version-Proben), Auth (Token, constant-time)/Rate-Limit/Logging,
    HTTP-API + main
  - `config_test.go`, `controller_test.go`, `handler_test.go` — go test/vet
  - `config.env.example`, `README.md`
- `deploy/` — Vorlagen (NUR Vorlagen, nicht installiert):
  - `deploy/systemd/andora-server.service`, `andora-monitor-fpm.service`,
    `andora-monitor-apache.service` (Produktionsbetrieb: Apache + PHP-FPM)
  - `deploy/systemd/andora-agent.service` (Agent als systemd-Unit; ohne
    `NoNewPrivileges`, da `sudo -n` erforderlich)
  - `deploy/conf/monitor.conf` (EnvironmentFile des Panels)
  - `deploy/conf/agent.conf` (Konfigurationsvorbild des Agent)
  - `deploy/conf/monitor-fpm.conf` (PHP-FPM-Pool, Unix-Socket)
  - `deploy/conf/monitor-apache.conf` + `andora-monitor-site.conf`
    (Apache-Instance inkl. expliziter Listen für Bind/Dual-Stack)
  - `deploy/sudoers/andora-monitor` (3 Befehle, passwordless, nur eine Unit)
  - `deploy/sudoers/andora-agent` (start/stop/restart + journalctl, feste Units)
  - `deploy/README.md` (Installation, Rechte, systemd-Befehle, Verifikation, Rollback)

### Geändert
- `src/realm/src/health.ts` — `/health` + `/status` + `/players`
- `src/realm/src/world.ts` — Tick-Dauer-Messung (`recordTick`)
- `src/realm/src/types.ts` — `Player.pingMs`, `Player.zoneId`
- `src/realm/src/handlers/hello.ts` — neue Felder initialisieren
- `src/realm/src/handlers/heartbeat.ts` — liest optionales `ping_ms`
- `readme.md` — Abschnitt „Monitoring & Admin-Panel"
- `.gitignore` — `.tmp/`

## Start (lokale Entwicklung, ohne systemd)

```bash
cd src/realm && npm run dev      # Gameserver (3001 WS, 3002 Health/Status)
cd web/andora-monitor
php -S 127.0.0.1:3003 -t public public/index.php   # Panel auf 127.0.0.1:3003
```

> `php -S` ist ausschließlich für die lokale Entwicklung gedacht (ein
> Listener pro Prozess, kein echtes Dual-Stack). Produktionsbetrieb:
> Apache + PHP-FPM über `deploy/`-Vorlagen — siehe `deploy/README.md`
> (Units `andora-monitor-fpm.service` + `andora-monitor-apache.service`).

Produktions-Installation: siehe `deploy/README.md` (Units, sudoers, Rechte).

## Offene Punkte / Architekturprobleme

1. **Ping**: Client sendet aktuell noch kein `ping_ms`; das Feld wird nur
   ausgelesen, wenn es im HEARTBEAT kommt. In `shared/protocol` ggf. künftig
   als Option aufnehmen; Server- und Godot-seitig noch ergänzen.
2. **Zonen**: `zone_id` kommt aus `characters.tabelle`; das Schema hat die
   Spalte, der Code liest sie noch nicht aus (heißt deshalb überall 0).
   Sobald Zonen implementiert sind, wird sie in `loadCharacter` eingelesen.
3. **`max_players` = 50**: Placeholder aus der Architektur (40-70/Kanal).
   Sobald Kanäle existieren: Anzahl Kanäle × Kapazität aus dem Config laden.
4. **`npc_active` / `instances`**: Platzhalter-Werte (0 / 1), bis
   NPC-/Instanz-Systeme existieren (M5/M6+). Die Metrik ist aber schon da.
5. **CPU-Last**: aktuell prozess-eigene CPU-Summe (user+system). Bei
   Multi-Instanzen ggf. pro-Instanz-Accounting ergänzen.
6. **systemd/Sudo**: Einmalig auf dem Produktionsserver installieren
   (siehe `deploy/README.md`). Auf dem Dev-Rechner bleibt alles, wie es ist.
7. **Ablösung durch Zielarchitektur**: Dieses lokale Panel ist ein Übergang.
   Die Zielarchitektur (zentrales Panel, Andora-Agent pro Server, mTLS,
   `andora`-Nicht-Root-Benutzer, `andora-updater`, signierte Manifeste und
   automatisierte Realm-Updates) ist verbindlich in
   `docs/Deployment_Betriebsarchitektur.md` dokumentiert.
 8. **PHP-spezifische Optimierungen (Bugfix/Review durchgeführt)**: Die
    History-Drosselung erfolgt ausschließlich über den Timestamp des letzten
    Punkts in `data/history.json` (kein separates `.lastpoint`-File mehr),
    die History wird mit echten Serverstatus-Werten gespeichert (keine
    pauschal `online=true`/`players=0`), und der Token-fail-closed-Schutz
    wird case-insensitiv auf den `x-api-token`-Header angewendet.
9. **Agent-Authentifizierung (Zwischenlösung)**: Der Andora-Agent
   authentifiziert den Panel-Zugriff aktuell per Token (`AGENT_TOKEN`,
   versioniert über `X-Andora-Token`/`x-api-token`). Archivziel bleibt mTLS
   (CA-/Client-Zertifikate sind konfigurationsseitig bereits vorbereitet:
   `AGENT_CA_FILE`, `AGENT_CLIENT_CERT_FILE`, `AGENT_CLIENT_KEY_FILE`,
   Server-Einstellung `RequireAndVerifyClientCert`).
