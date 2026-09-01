# Architektur: Andora (2D-MMORPG)

## Aufbau
- **Client**: Godot 3.5, GLES2, Ziel = Raspberry Pi 4 (4GB/8GB)
- **Server**: Node.js 20 + TypeScript auf x86 (Ryzen 9 5900X, Debian 13)
- **DB**: MariaDB 10 (Accounts, Charaktere, Inventar, Quests, Gilden, Auktionshaus)
- **Netz**: WebSocket, später vieleicht nicht jetzt ( binäre Pakete (ID + Felder)), 10 Hz Server-Tick
- **Client-Schutz**: RENDER_CAP (48/64 Entities), auto perf_mode, Chunk-Texture-Batching

## Verzeichnisse
- `shared/`   – Protokoll + Definitionsdaten (Client UND Server lesen)
- `i18n/`     – Sprache-JSONs (de, en, ...), beide Seiten teilen
- `server/`   – Node/TS-Server (Autorität: Combat, Loot, AH, NPC-AI via Ollama)
- `src/`      – Godot-Client-Logik (Rendering, Input, Interpolation)
- `client`
- `monitor`
- `deploy`
- `docs`

## Skalierung
- Kanäle (Zonen-Kopien, 40-70 Spieler/Kanal), Gateway verteilt
- Raids/Dungeons = private Instanzen
- Auktionshaus global (quer über Kanäle, Server-Layer + MariaDB)

## i18n
- Alle Text-Keys in `i18n/<lang>.json`, Client UND Server laden dieselben Dateien
- Niemals harte Texte im Code: immer `t("key", args)`

## Expansion
- Dateien mit Präfix exp1_, exp2_ usw. gehören zu geplanten Erweiterungen und sind keine Anforderungen an das Grundspiel. Sie dürfen nur implementiert werden, wenn die entsprechende Expansion ausdrücklich als aktueller Entwicklungsumfang festgelegt wurde.