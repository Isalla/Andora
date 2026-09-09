# Andora

Andora ist ein Fantasy-MMORPG. Die dauerhafte Spielwelt wird überwiegend als 2D bzw. vorgerenderte Assets dargestellt (2D trägt die Welt; 3D setzt die Akzente – 3D dient dabei auch als Produktionswerkzeug; Präsentationsstil, z. B. isometrisch, bleibt für die Weltdarstellung offen). Der Client wird mit Godot 3.5 entwickelt und ist von Anfang an auf eine ressourcenschonende Darstellung ausgelegt. Primäre Referenzplattform ist der Raspberry Pi 4 (8 GB) mit einem Performance-Grundsatz von 60 FPS (Ziel) und 30 FPS (Untergrenze unter definierter hoher Last); Details in `docs/Clientdarstellung_und_Performance.md`. Die Serverdienste laufen auf separaten Debian-/Linux-Systemen (Zielarchitekturen mindestens `linux-amd64` und `linux-arm64`).

## Technik

* **Client:** Godot 3.5 (GLES2), optimiert für den Raspberry Pi; das Projekt liegt im Repository-Root (`project.godot`)
* **Server:** fünf getrennte Andora-Dienste:
  - `API/Auth` – Go-Service, einziger Dienst mit direktem Auth-DB-Zugriff (`src/api`, implementiert)
  - `Login` – separater Login-Service in Go (`src/login`, implementiert)
  - `Realm` – Realm-/World-Server in Rust (`src/realm-rs`, Zielimplementierung; Autorität: Combat, Loot, AH, NPC-AI via Coordinator/Ollama)
  - `Coordinator` – zentrale KI-Queue/Ollama-Schnittstelle in Go (`src/coordinator`, implementiert)
  - `Voice` – Voice-Server (späterer Release, noch nicht angelegt)
* **Legacy/Transition:** `src/realm/` (Node.js/TypeScript) ist der ältere Realm-Stand, der aktiv bleibt, bis die Migration nach `src/realm-rs/` abgeschlossen ist; das aktuelle Admin-/Monitoring-Panel ist die PHP-Implementierung `web/andora-monitor/` (Node-`monitor/` ist Legacy-Referenz). Details: `docs/architecture.md`, `docs/monitoring_web_panel.md`.
* **Datenbank:** MariaDB 10 (`auth`, `realm_state_<realm>` pro Realm mit statischen + dynamischen Realm-Daten; keine zentrale `world_data`, Charakterdaten liegen in `realm_state_<realm>`)
* **Netzwerk:** WebSocket, serverautoritativ, 10-Hz-World-Tick
* **Lokalisierung:** Deutsch, Englisch, vereinfachtes Chinesisch (zh-Hans) und traditionelles Chinesisch (zh-Hant) von Anfang an; Clienttexte grundsätzlich nicht hart codiert, sondern über `i18n/` (Details in `i18n/README.md`)
* **KI:** lokale KI-Dienste (Ollama / Sprachmodelle) für die dafür vorgesehenen Spielsysteme

## Verzeichnisse

* `src/` – Server-Dienste: `realm-rs/` (Rust-Zielimplementierung), `realm/` (Node.js-/TypeScript-Übergangsstand), `api/` (Go-Auth-/API-Service), `login/` (Go-Login-Service), `coordinator/` (Go-KI-Queue-Service)
* (Repository-Root) – Godot-Client (`project.godot`, Scenes, Scripts)
* `shared/` – gemeinsames Netzwerkprotokoll und Definitionen (Client und Server lesen dieselben Dateien)
* `i18n/` – Übersetzungen und Lokalisierung
* `monitor/` – lokales Monitoring-/Admin-Panel (Übergangs-/Legacy-Stand; aktuell: PHP-`web/andora-monitor`)
* `web/andora-monitor/` – PHP-Implementierung des Admin-/Monitoring-Panels (aktueller Stand)
* `deploy/` – Deployment-Vorlagen (NUR Vorlagen: systemd-Units, Konfiguration, sudoers; Zielbetrieb in `docs/Deployment_Betriebsarchitektur.md`)
* `docs/` – Architektur-, System- und Spieldesign-Dokumentation; Einstiegspunkt: `docs/README.md`

Hinweis: Diese Liste nennt nur die im Git-Repository enthaltene Struktur. Lokal existierende, aber durch `.gitignore` ausgeschlossene Arbeitsbereiche (z. B. Toolchains, Caches, temporäre Dateien, technische Referenzbibliothek) werden hier nicht aufgeführt; diese lokalen Regeln sind in `docs/ai_jobs.md` bzw. `docs/Temporäre_Dateien.md` dokumentiert.

## Serverarchitektur

Andora trennt unterschiedliche Verantwortungsbereiche bewusst voneinander; jeder Dienst kann eigenständig auf einem eigenen Linux-Server betrieben werden.

Zielkette für den Spieleinstieg: `Auth/API → Login → Realm`. Einen separaten Worldserver-Dienst gibt es nicht; der Realm-Server führt seinen Realm direkt aus (Handoff-Übergabe, realm-gebunden, einmalig).

Die persistente Datenhaltung ist in zwei Datenbankbereiche gegliedert:

* `auth` – Accounts, Authentifizierung, Sessions und Serverautorisierung (nur der API/Auth-Dienst greift darauf zu)
* `realm_state_<realm>` – persistenter Zustand eines einzelnen Realms inklusive Charakterdaten und statischer Weltdefinitionen

Der Realm-Server besitzt keinen direkten Zugriff auf die Auth-Datenbank. Authentifizierungsfunktionen werden über eine getrennte Auth/API-Grenze bereitgestellt.

## Start der Serverdienste

Alle lokale Konfiguration liegt in `config.env` (Vorlagen `config.env.example`), die keinerlei produktive Zugangsdaten in das Git-Repository dürfen.

* **API/Auth (Go):** `cd src/api && go build -o build/authapi . && ./build/authapi [config.env]`
* **Login (Go):** `cd src/login && go build ./... && go test ./...`
* **Coordinator (Go):** `cd src/coordinator && go build ./... && go test ./...`
* **Realm (Rust, Zielimplementierung):** `cd src/realm-rs && cargo build && cargo test`
* **Realm (Node.js, Übergangsstand):** `cd src/realm && cp config.env.example config.env && npm install && npm run dev`

Build- und Test-Toolchains werden aus `.tmp/` des Projektroots verwendet (`docs/ai_jobs.md`).

## Monitoring & Admin-Panel

Das aktuelle Admin-/Monitoring-Panel ist die **PHP-Implementierung** unter
`web/andora-monitor/` (Status, Spieler, History, Config, Control). Das Node-Panel
(`monitor/`) ist ein Übergangs-/Legacy-Stand und nur Referenz. Das Panel ist
ausschließlich für Administration und Betrieb vorgesehen, nicht Bestandteil des
Spielerclients. Die verbindliche Zielarchitektur (zentrales Panel, Andora-Agent
pro Server, mTLS, `andora-updater`) steht in `docs/Deployment_Betriebsarchitektur.md`.

`php -S` ist ausschließlich für die lokale Entwicklung gedacht (ein Listener pro
Prozess, kein echtes Dual-Stack). Produktionsbetrieb: Apache + PHP-FPM.

```bash
# Lokale Entwicklung (Dev):
cd web/andora-monitor
php -S 127.0.0.1:3003 -t public public/index.php
```

Produktionsbezogene Konfigurationen und Vorlagen befinden sich unter `deploy/` (NUR Vorlagen; Installation manuell auf dem Zielsystem, siehe `deploy/README.md`).

Weitere technische Details befinden sich in der Dokumentation unter `docs/` (Eintritt: `docs/README.md`).
```

## Entwicklungsprinzip

Andora wird serverautoritativ entwickelt. Der Server bestimmt den verbindlichen Zustand der Spielwelt; der Client stellt diesen Zustand dar und verarbeitet die Benutzereingaben.

Neue Systeme werden modular entwickelt und sollen vorhandene Komponenten wiederverwenden, anstatt parallele Implementierungen aufzubauen.

Andora ist vollständig kostenlos spielbar und wird nicht auf Geldverdienen durch das Spiel ausgerichtet. Inhalt, Fortschritt und Vorteile werden nie gegen Echtgeld verkauft; freiwillige Spenden dürfen ausschließlich nicht spielrelevante Anerkennung geben (z. B. Donator-Kennzeichnung auf der Website, optionaler rein kosmetischer Ingame-Titel). Die offizielle Andora-Website soll später eine freiwillige Donation-Möglichkeit anbieten – ausdrücklich kein Shop und keine Gameplay-Monetarisierung; Zahlungsanbieter, Beträge, technische Umsetzung und organisatorische/rechtliche Ausgestaltung sind noch nicht festgelegt. Details in `docs/Monetarisierung_und_Donations.md`.

## Lizenz

MIT
