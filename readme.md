# Andora

Ein 2D-Fantasy-MMORPG, entwickelt auf Basis von Godot 3. Zielplattform des
Clients ist der Raspberry Pi 4 (8 GB) – der Server läuft auf einem separaten
x86-System.

- **Client**: Godot 3.5 (GLES2), optimiert für den Raspberry Pi
- **Server**: Node.js + TypeScript (MariaDB 10)
- **Netz**: WebSocket, 10 Hz Tick, Client-Server-Sync + Interpolation
- **Lokalisierung**: Deutsch & Englisch von Anfang an (später erweiterbar)

## Verzeichnisse
- `shared/` – Protokoll & gemeinsame Definitionen (Client + Server)
- `i18n/` – Übersetzungen (de, en)
- `server/` – Node/TS-Server, Auktionshaus, Kanäle, NPC-AI
- `src/` – Godot-Client-Logik
- `docs/architecture.md` – Architektur & Bauplan

## Start (Client)
Projekt mit Godot 3 öffnen und ausführen. Autoload `I18n` liefert `t("key")`.

## Start (Server)
```bash
cd server
cp config.env.example config.env   # DB + Ollama-Werte eintragen
npm i && npm run dev
```

## Monitoring & Admin-Panel (nur für Admin, keine Spieler)
```bash
cd monitor
node server.js                     # Panel auf 127.0.0.1:3003
```
- Gameserver liefert `GET /health`, `GET /status`, `GET /players` auf Port 3002
  (Spielerzahl, Tick-/Event-Loop-Last, CPU/RAM, Zonen/NPC/Instanzen,
  Spielerdiagnose inkl. Ping & sichtbarer Entities).
- Panel: eigenes Node-Tool (keine Dependencies): Dashboard, Verlauf
  (~1 min, In-Memory), Steuerung start/stop/restart (mit Bestätigung,
  nur via `sudo -n systemctl`), Config-Editor (Whitelist, keine Secrets).
- Produktions-Installation: `deploy/` (systemd-Units + sudoers-Vorlage +
  Anleitung) — Details in `docs/monitoring_web_panel.md`.

## Lizenz
MIT
