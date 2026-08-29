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

## Lizenz
MIT
