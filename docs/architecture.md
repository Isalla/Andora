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
- `src/realm/`      – Node/TS-Server (Realm-/World-Server, Autorität: Combat, Loot, AH, NPC-AI via Ollama)
- `src/api/`        – Go-API-/Security-Service (einziger Service mit Auth-DB-Zugriff)
- `src/login/`      – separater Login-Server (Struktur vorgesehen, kein Code vorhanden)
- `src/coordinator/`– separater Coordinator-Service (Struktur vorgesehen, kein Code vorhanden)
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

## Ergänze die bestehende Andora-Dokumentation um die Zielplattformen für den API-Service.

Prüfe zuerst die vorhandenen relevanten Dokumente, insbesondere:
- docs/architecture.md
- deploy/README.md
- vorhandene API-Dokumentation unter src/api/

Dokumentiere an der fachlich passenden Stelle:

Der Andora API-/Security-Service wird in Go entwickelt und muss beim späteren produktionsreifen Build für zwei Linux-Zielplattformen bereitgestellt werden:

- Linux ARM64 (`GOOS=linux`, `GOARCH=arm64`)
  - insbesondere für Raspberry Pi 64-Bit
- Linux x86-64 (`GOOS=linux`, `GOARCH=amd64`)
  - für klassische x86-64 Server/VMs

Beide Binaries müssen aus demselben Quellstand erzeugt werden und funktional identisch sein.

Ziel ist, den API-Service zunächst auch auf ARM64/Raspberry-Pi-Hardware testen und betreiben zu können. Sollte deren Leistung später nicht ausreichen, muss derselbe Service ohne Architekturänderung auf einen x86-64-Linux-Server verschoben werden können.

Diese Vorgabe ist eine dauerhafte Deployment-/Release-Anforderung und soll Qwen bei der späteren Fertigstellung des Produkts eindeutig erkennen lassen, dass beide Plattformen gebaut und getestet werden müssen.

Noch keine Release-Binaries erstellen, sofern dies nicht Bestandteil der aktuell laufenden Aufgabe ist.

Ändere nur die fachlich passenden Dokumentationsstellen und vermeide doppelte oder widersprüchliche Dokumentation.

Am Ende kurz auf Deutsch berichten, welche Datei(en) und Abschnitte ergänzt wurden.

Keinen Git-Commit erstellen.


