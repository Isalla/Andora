# Architektur: Andora (2D-MMORPG)

## Aufbau
- **Client**: Godot 3.5, GLES2, Ziel = Raspberry Pi 4 (4GB/8GB)
- **Serverdienst (fünf getrennte Andora-Dienste; dezentral auf unterschiedlichen Servern betreibbar):**
  - `API/Auth` – Go-Service, einziger Dienst mit direktem Auth-DB-Zugriff
  - `Login` – separater Login-Service
  - `Realm` – Realm-/World-Server in Rust (Autorität: Combat, Loot, AH, NPC-AI via Coordinator/Ollama)
  - `Coordinator` – zentrale KI-Queue/Ollama-Schnittstelle
  - `Voice` – Voice-Server (späterer Release; Teil der fünf-Dienste-Zielarchitektur)
- **Zielplattformen**: mindestens `linux-amd64` und `linux-arm64` (Debian/Linux); Realm-Server als Rust-Binary
- **DB**: MariaDB 10 (Auth-DB `auth`, Realm-DB `realm_state_<realm>` mit statischen + dynamischen Realm-Daten; keine zentrale `world_data`)
- **Netz**: WebSocket, spätere binäre Pakete (ID + Felder), 10 Hz Server-Tick
- **Client-Schutz**: RENDER_CAP (48/64 Entities), auto perf_mode, Chunk-Texture-Batching

## Verzeichnisse
- `shared/`   – Protokoll + Definitionsdaten (Client UND Server lesen)
- `i18n/`     – Sprache-JSONs (de, en, ...), beide Seiten teilen
- `src/realm-rs/`   – Realm-Server in Rust (Zielimplementierung; Autorität: Combat, Loot, AH, NPC-AI via Coordinator/Ollama). Genau eine DB (`realm_state_<realm>`), Einstieg per Handoff. Bauen/Testen mit der vorhandenen Toolchain (`~/.cargo`).
- `src/realm/`      – Realm-Server (Node.js/TypeScript) als ÜBERGANGSSTAND: lauffähig, wird schrittweise nach `src/realm-rs/` migriert ( Alt-Annahmen: character-/world_data-Pools, Session statt Handoff). Nicht ausbauen.
- `src/api/`        – Go-API-/Auth-Service (einziger Service mit Auth-DB-Zugriff; Zielplattformen arm64 + amd64)
- `src/login/`      – separater Login-Service (Go, implementiert): Client-Login, Realm-Liste, Handoff-Ausstellung gegen die Auth-API; keine DB-Rechte
- `src/coordinator/`– separater Coordinator-Service (Struktur vorgesehen, kein Code vorhanden)
- `src/voice/`      – Voice-Service (noch nicht angelegt, geplant)
- `monitor`         – lokales Monitoring-/Admin-Panel (Übergangs-/Legacy-Status, siehe `monitoring_web_panel.md`)
- `deploy`          – Deployment-Vorlagen (Legacy-Status; Zielarchitektur in `Deployment_Betriebsarchitektur.md`)
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

## Betrieb & Deployment
Die Zielbetriebsarchitektur (zentrales Admin-/Deployment-Panel, Andora-Agent auf jedem verwalteten Server, mTLS, `andora`-Nicht-Root-Benutzer, `andora-updater` mit signierten Manifests, Checksummen, Healthchecks und Rollback sowie der automatisierte Realm-Update-Ablauf) ist bindend in:

```text
docs/Deployment_Betriebsarchitektur.md
```

beschrieben. Realm-Versionen und die Realm-Datenhaltung (statische + dynamische Daten pro Realm, keine zentrale `world_data`) sind in `docs/Datenbank_Architektur.md` definiert.

> Jeder Andora-Dienst kann auf einem eigenen Debian-/Linux-Server betrieben werden. Die Zielarchitekturen sind mindestens `linux-amd64` und `linux-arm64`.
