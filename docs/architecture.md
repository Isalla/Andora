# Architektur: Andora (2D-MMORPG)

## Aufbau
- **Client (aktuell)**: Godot 3.5, GLES2, Ziel = Raspberry Pi 4 (4GB/8GB); Godot-/Raspberry-Pi-Client ist der verbindliche Gameplay-Referenzclient (Godot-First); Darstellung = 2D/vorgerenderte Welt, 3D als Akzente & Produktionswerkzeug; bei Fähigkeiten, Zaubern und kurzlebigen Kampfeffekten 2D-/3D-Darstellung spielerseitig in den Grafikoptionen wählbar – rein clientseitig, ohne Gameplay-Auswirkung, Realm für Effekte autoritativ (Details in `Clientdarstellung_und_Performance.md`)
- **Weitere offizielle Clients**: Andora darf langfristig mehrere offizielle Clients besitzen (Browser, möglicher Unreal-Engine-PC-Client, weitere); alle Clients verbinden sich mit denselben normalen Realms (keine PC-/Pi-/Browser-/UE-Realms), bieten identische Inhalte und unterscheiden sich ausschließlich in Darstellung/Präsentation ohne Gameplay-Vorteil; Realm autoritativ; Entwicklungsrichtung Realm → Godot-Referenzclient → weitere Clients; Unreal-Client ist spätere Option, kein aktueller Implementierungsauftrag (verbindliche Entscheidung in `Mehrere_Offizielle_Clients.md`)
- **Serverdienst (fünf getrennte Andora-Dienste; dezentral auf unterschiedlichen Servern betreibbar):**
  - `API/Auth` – Go-Service, einziger Dienst mit direktem Auth-DB-Zugriff
  - `Login` – separater Login-Service
  - `Realm` – Realm-Server in Rust (Autorität: Combat, Loot, AH, NPC-AI via Coordinator/KI-Provider (lokal: Ollama); genau ein konfigurierbares Ruleset je Realm, kein Fork der Realm-Software pro Ruleset — siehe `Login_Realm_Architektur.md`)
  - `Coordinator` – zentrale KI-Queue/Provider-Schnittstelle (Go, implementiert; angeschlossener lokaler Provider: Ollama)
  - `Voice` – Voice-Server (späterer Release; Teil der fünf-Dienste-Zielarchitektur)
- **Zielplattformen**: mindestens `linux-amd64` und `linux-arm64` (Debian/Linux); Realm-Server als Rust-Binary
- **DB**: MariaDB 10 (Auth-DB `auth`, Realm-DB `realm_state_<realm>` mit statischen + dynamischen Realm-Daten; keine zentrale `world_data`)
- **Netz**: WebSocket, spätere binäre Pakete (ID + Felder), 10 Hz Server-Tick
- **Client-Schutz**: RENDER_CAP (48/64 Entities), auto perf_mode, Chunk-Texture-Batching
- **Client-Performance-Grundsatz:** Ziel 60 FPS, Untergrenze 30 FPS unter definierter hoher Last (Raspberry Pi 4); gilt für den Godot-Referenzclient; weitere Clients sind nicht darauf beschränkt, dürfen aber keine gameplayrelevanten Unterschiede erzeugen (`Mehrere_Offizielle_Clients.md`). „Definierte hohe Last" = Last innerhalb der oben genannten Client-Schutzmechanismen (RENDER_CAP, auto perf_mode, Chunk-Texture-Batching, aktive Effekte im jeweiligen perf_mode-Budget); konkrete perf_mode-Degradationswerte werden später durch Performance-Tests auf dem Raspberry Pi 4 festgelegt. Details: `Clientdarstellung_und_Performance.md`.

## Verzeichnisse
- `shared/`   – Protokoll + Definitionsdaten (Client UND Server lesen); engineunabhängig zu pflegen, nicht unnötig an Godot-spezifische Darstellungsdetails koppeln – Realm liefert Spielzustand/Informationen, Client entscheidet über deren Darstellung (`Mehrere_Offizielle_Clients.md`)
- `i18n/`     – Sprache-JSONs (de, en, zh-Hans, zh-Hant, ...), beide Seiten teilen
- `src/realm-rs/`   – Realm-Server in Rust (Zielimplementierung; Autorität: Combat, Loot, AH, NPC-AI via Coordinator/KI-Provider (lokal: Ollama)). Genau eine DB (`realm_state_<realm>`), Einstieg per Handoff. Bauen/Testen mit der vorhandenen Toolchain (`~/.cargo`).
- `src/realm/`      – Realm-Server (Node.js/TypeScript) als ÜBERGANGSSTAND: lauffähig, wird schrittweise nach `src/realm-rs/` migriert ( Alt-Annahmen: character-/world_data-Pools, Session statt Handoff). Nicht ausbauen.
- `src/api/`        – Go-API-/Auth-Service (einziger Service mit Auth-DB-Zugriff; Zielplattformen arm64 + amd64)
- `src/login/`      – separater Login-Service (Go, implementiert): Client-Login, Realm-Liste, Handoff-Ausstellung gegen die Auth-API; keine DB-Rechte
- `src/coordinator/`– Coordinator-Service (Go, implementiert): zentrale KI-Queue/Provider-Schnittstelle (providerunabhängig; angeschlossener lokaler Provider: Ollama), dateibasierte Queue ohne DB-Zugriff; Details `src/coordinator/README.md`, `docs/Coordinator.md`
- `src/voice/`      – Voice-Service (noch nicht angelegt, geplant)
- `monitor`         – lokales Monitoring-/Admin-Panel (Übergangs-/Legacy-Status, siehe `monitoring_web_panel.md`)
- `deploy`          – Deployment-Vorlagen (Legacy-Status; Zielarchitektur in `Deployment_Betriebsarchitektur.md`)
- `docs`

## Skalierung
- Kanäle (Zonen-Kopien, 40-70 Spieler/Kanal), Gateway verteilt
- Raids/Dungeons = private Instanzen
- Auktionshaus global (quer über Kanäle, Server-Layer + MariaDB)

## i18n
- Alle Text-Keys in `i18n/<locale>.json`, Client UND Server laden dieselben Dateien
- Niemals harte Texte im Code: immer `t("key", args)`
- Unterstützte Locales: `de`, `en` (Master), `zh-Hans` (简体中文), `zh-Hant` (繁體中文)
- `zh-Hans` und `zh-Hant` sind getrennte Lokalisierungen, keine automatische Schriftkonvertierung
- Eigennamen und noch nicht festgelegte Fantasy-Begriffe bleiben konsistent; Grundlagen und Glossar-Vorbereitung: `i18n/README.md`

## Expansion
- Dateien mit Präfix exp1_, exp2_ usw. gehören zu geplanten Erweiterungen und sind keine Anforderungen an das Grundspiel. Sie dürfen nur implementiert werden, wenn die entsprechende Expansion ausdrücklich als aktueller Entwicklungsumfang festgelegt wurde.

## Betrieb & Deployment
Die Zielbetriebsarchitektur (zentrales Admin-/Deployment-Panel, Andora-Agent auf jedem verwalteten Server, mTLS, `andora`-Nicht-Root-Benutzer, `andora-updater` mit signierten Manifests, Checksummen, Healthchecks und Rollback sowie der automatisierte Realm-Update-Ablauf) ist bindend in:

```text
docs/Deployment_Betriebsarchitektur.md
```

beschrieben. Realm-Versionen und die Realm-Datenhaltung (statische + dynamische Daten pro Realm, keine zentrale `world_data`) sind in `docs/Datenbank_Architektur.md` definiert.

> Jeder Andora-Dienst kann auf einem eigenen Debian-/Linux-Server betrieben werden. Die Zielarchitekturen sind mindestens `linux-amd64` und `linux-arm64`.
