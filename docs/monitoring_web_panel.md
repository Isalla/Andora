# Monitoring-/Admin-Web-Panel für den Andora-Server (implementiert)

Eine Web-Oberfläche auf dem **selben Host** wie der Gameserver, ausschließlich
für den Admin (Spieler haben KEINEN Zugriff). Sie dient der Überwachung und
Steuerung des Servers.

## Getrennte Prozesse

- **Gameserver** (`server/build/main.js`): läuft als systemd-Unit
  `andora-server.service` (Vorlage in `deploy/systemd/`). `Restart=on-failure`
  kümmert sich um den Crash-Neustart.
- **Monitoring-Panel** (`monitor/server.js`): läuft als eigener Node-Prozess
  (systemd-Unit `andora-monitor.service`, Vorlage in `deploy/systemd/`).
  Überlebt einen Gameserver-Crash (zeigt dann Offline + Neustart-Button).
- Das Panel startet/stoppt den Gameserver **nur** über
  `sudo -n systemctl start|stop|restart andora-server.service` — keine
  Shell-Freiheit, keine Root-Berechtigung des Panels (sudoers-Vorlage in
  `deploy/sudoers/andora-monitor`, nur diese drei Befehle, passwordless).

## Ports & Endpunkte

| Port | Prozess | Endpunkte |
|---|---|---|
| 3001 | Gameserver | WebSocket (Spieler) |
| 3002 | Gameserver | `GET /health`, `GET /status`, `GET /players` |
| 3003 | Panel | `GET /` Dashboard, `GET/POST /api/*` |

Panel-Bindung: default `127.0.0.1:3003` (weder LAN noch public);
optional `ANDORA_MONITOR_TOKEN` (Header `x-api-token` oder `?token=`).

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

### Spielerdiagnose (`players[]` in `/status` bzw. `GET /players`)

- `id`, `name`, `zone_id`, `ping_ms` (Client sendet optional `ping_ms` im
  `HEARTBEAT.data`), `last_activity`, `seconds_inactive`, `visible_entities`
  (Entity-/NPC-Anzahl im AOFB-Radius des Spielers).
  Keine Rangliste — nur Diagnosewerte.

## Funktionen des Panels

- Dashboard (ein HTML-File, kein Framework): Status, Auslastung, Spieler,
  Verlauf (~10 min, In-Memory Ring-Buffer, Poll 2 s), Steuerung, Config.
- **Steuerung**: Start / Stop / Restart — erst nach zweiter Bestätigung
  (POST `/api/control` mit `confirm: true`).
- **Verlauf**: `GET /api/history` (In-Memory, max. 360 Punkte à 10 s).
- **Config**: `GET /api/config` + `POST /api/config` — nur Whitelist-Keys von
  `server/config.env` sind editierbar (`PORT_WS`, `PORT_HTTP`, `TICK_MS`,
  `AOFB_RADIUS`, `RENDER_CAP_DEFAULT`, alle `OLLAMA_*` außer
  Secrets); `DB_*` und Passwörter sind **nicht** sichtbar und nicht
  änderbar. Werte werden serverseitig validiert (Port-Bereiche, Tick 16-1000
  ms, Sampling-Bereiche, URL-Format …).
- **Keine Secrets** an den Client: ausschließlich Whitelist-Werte.

## Geänderte / neue Dateien

### Neu
- `monitor/` — eigener Node-Prozess (nur stdlib, keine Dependencies, kein Build):
  - `package.json`, `config.js`
  - `server.js` (HTTP-API + Verlauf)
  - `lib/history.js` (In-Memory Ring-Buffer)
  - `lib/systemctl.js` (start/stop/restart via `sudo -n`, is-active ohne sudo)
  - `lib/envconfig.js` (lesen/validieren/schreiben von `config.env`)
  - `public/index.html` (Dashboard)
- `server/src/metrics.ts` — Tick-Statistik + CPU/Heap/RSS
- `server/src/events.ts` — Event-Loop-Lag-Messung
- `deploy/` — Vorlagen (NUR Vorlagen, nicht installiert):
  - `deploy/systemd/andora-server.service`, `andora-monitor.service`
  - `deploy/conf/monitor.conf` (EnvironmentFile des Panels)
  - `deploy/sudoers/andora-monitor` (3 Befehle, passwordless, nur eine Unit)
  - `deploy/README.md` (Installation, Rechte, systemd-Befehle, Verifikation, Rollback)

### Geändert
- `server/src/health.ts` — `/health` + `/status` + `/players`
- `server/src/world.ts` — Tick-Dauer-Messung (`recordTick`)
- `server/src/types.ts` — `Player.pingMs`, `Player.zoneId`
- `server/src/handlers/hello.ts` — neue Felder initialisieren
- `server/src/handlers/heartbeat.ts` — liest optionales `ping_ms`
- `readme.md` — Abschnitt „Monitoring & Admin-Panel"
- `.gitignore` — `.tmp/`

## Start (lokale Entwicklung, ohne systemd)

```bash
cd server && npm run dev          # Gameserver (3001 WS, 3002 Health/Status)
cd monitor && node server.js      # Panel auf 127.0.0.1:3003
```

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
