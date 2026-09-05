# Andora – Projekt-Status (tatsächlicher Implementierungsstand)

**ZWECK:** Kompakter, fortlaufend gepflegter Überblick über den **tatsächlich in Code
vorhandenen** Implementierungsstand aller Andora-Dienste und -Systeme. Keine Architektur-
oder Design-Dokumentation (dafür die Detaildokumente in `docs/`); hier zählt nur, was im
Repository realisiert ist.

**MASSSTAB:** Status wird am Code, an Tests und an DB-Migrationen gemessen. Eine
ausformulierte Spezifikation ist kein Implementierungsstand – „Nur spezifiziert" bleibt
„Nur spezifiziert", bis Code existiert.

**PFLEGE:** Die Coding-KI aktualisiert diese Datei bei jeder Änderung des tatsächlichen
Implementierungsstands selbstständig (Pflicht laut `docs/ai_jobs.md`).

## Statusstufen

| Stufe | Bedeutung |
|---|---|
| **Fertig implementiert** | Im Code umgesetzt und durch Tests/Verifikation belegt |
| **Teilweise implementiert** | Kernteil umgesetzt, wesentliche Teile fehlen |
| **In Entwicklung** | Aktiver, noch nicht abgeschlossener Ausbau |
| **Grundgerüst vorhanden** | Rahmen existiert (Scaffolding/Protokoll/Hello-World), Funktion fehlt |
| **Nur spezifiziert** | Nur Design/Spezifikation in `docs/`, kein Code |
| **Später vorgesehen** | Geplant, weder spezifiziert noch implementiert |
| **Legacy/Transition** | Alter Übergangsbestand; wird durch Zielimplementierung abgelöst |

## Gesamtübersicht

| Bereich | Ort | Status |
|---|---|---|
| Auth/API-Service (Go) | `src/api` | **Fertig implementiert** |
| Login-Service (Go) | `src/login` | **Fertig implementiert** |
| Realm-Service (Rust, Ziel) | `src/realm-rs` | **Teilweise implementiert** |
| Realm-Service (Node.js, Übergang) | `src/realm` | **Legacy/Transition** |
| Coordinator (KI-Queue/Ollama) | `src/coordinator` | **Nur spezifiziert** |
| Voice | – | **Später vorgesehen** |
| Client (Godot) | Repo-Root (`project.godot`) | **Grundgerüst vorhanden** |
| Monitor-/Web-Panel (lokal) | `monitor/` | **Grundgerüst vorhanden** (Übergang, Ziel: `Deployment_Betriebsarchitektur.md`) |
| Protokoll-/Übersetzungsmodule | `shared/`, `i18n/` | **Grundgerüst vorhanden** |

## 1. Auth/API-Service (Go, `src/api`) – Fertig implementiert

Zentraler Auth-/Sicherheitsservice; einziger Dienst mit direktem Zugriff auf die
`auth`-Datenbank. Routen in `src/api/server.go`, Architektur: `docs/Auth_API_Architektur.md`.
`go test ./...` grün (DB-freie Integrationstests über AuthStore-Fake + TOTP-/Trusted-
Device-/Verfall-Tests + Listener-/Bind-/Client-IP-Tests `ipnet_test.go`).
Listener-Konfiguration: `AUTHAPI_BIND_HOST` (leer = bisheriges `tcp :port`; `ipv4`/`4`,
`ipv6`/`6`, `dual`/`both`, IP-Literal, Hostname), `TRUSTED_PROXIES` (X-Forwarded-For/X-Real-IP
nur aus dieser Fail-closed-Liste), MariaDB-DSN mit geklammertem IPv6-Host
(`net.JoinHostPort`). Alle Funktionen der Tabelle: **Fertig implementiert**, Fehlt/für
nächstes: –.

| Funktion | Funktioniert | Verifikation |
|---|---|---|
| Service-Auth (HMAC-SHA256, ±90 s, 64 KiB, Permission-Mapping, pro-Endpoint 403) | X-Andora-Header; `/health`+`/status` offen | Tests |
| `POST /auth/verify` (Session-Anlage, `last_login_at`, 2FA-Flow) | Nur Erfolg legt Session an; alles Ablehnungen `{valid:false}` | Tests |
| `POST /account/register` (Username 3–32, PW 8–72, konserv. E-Mail) | Duplikat → 409; Fresh-Start-Ban | Tests |
| `POST /account/password/change` | Transaktional, revokiert Sessions + trusted devices | Tests |
| `POST /account/recovery/request` / `confirm` | Einmaliger Token, `RECOVERY_TTL_MINUTES` | Tests |
| `POST /account/permissions` | Geschlossene Berechtigungsmenge pro Account | Tests |
| `POST /session/create` / `validate` / `revoke` | Volle Session-Verwaltung | Tests |
| `GET/POST /realms` | Nur Betriebsdaten (Name, Sprache, Region, Status) | Tests |
| `POST /handoff/create` / `validate` | Bindung Account→Realm; Validieren verbraucht Token | Tests |
| `POST /world/authenticate` / `heartbeat` | **LEGACY** (kein Worldserver mehr), kompatibel erhalten | Tests |
| `POST /twofactor/status` / `setup` / `enable` / `disable` / `reset` | TOTP, Sekret verschlüsselt, 10 Recovery-Codes, disable/reset revokiert Geräte | Tests |
| `POST /devices/list` / `revoke` (max. 3, 30-Tage-Inaktivitätsverfall) | `last_used_at`-Logik; Verfall entzieht Bypass und befreit Slot | Tests |
| `POST /security/events` | Sicherheits-Log des Accounts (neueste zuerst) | Tests |
| `POST /parental/*` (status, setup, update, remove, pin/change, pin/verify, extension, periods±, exceptions±, notifications±deliver) | Accountgebunden; Extension 1×/Tag (3600 s); Poll-Limit 120 s | Tests |
| Auth-DB-Migrationen 001–013 (accounts … parental_control) | Automatischer Runner beim Start; Fehler → kein Start | Startlogik |

## 2. Login-Service (Go, `src/login`) – Fertig implementiert

Front-End-Dienst für Login/Handoff; **kein** direkter DB-Zugriff, ruft Auth/API mit eigenen
Service-Credentials. Architektur: `docs/Login_Realm_Architektur.md`. `go test ./...` grün
(+ Listener-/Bind- und Client-IP-Tests `ipnet_test.go`). Listener-Konfiguration:
`LOGIN_BIND_HOST` (leer = bisheriges `tcp :port`; `ipv4`/`4`, `ipv6`/`6`, `dual`/`both`,
IP-Literal, Hostname), `AUTHAPI_URL`/`REALM_WS_*` akzeptieren IPv6-Literal ungeklammert als
`host:port` (werden zu `[host]:port` normalisiert). Alle Funktionen: **Fertig
implementiert**, Fehlt: –.

| Funktion | Funktioniert |
|---|---|
| `GET /health`, `GET /status` (Uptime) | Health-/Status-Endpunkte |
| `POST /login` (Password + Username **oder** E-Mail-Lookup-Hash) | Reicht 2FA-Felder an Auth-API durch (keine Einzelauswertung im Login) |
| `POST /logout` | Session-Logout |
| `GET/POST /realms` | Realm-Auswahl-Daten |
| `POST /handoff` | Brücke zur Auth-API-Handoff-Erzeugung |

## 3. Realm-Service (Rust, `src/realm-rs`) – Teilweise implementiert

Zieldienst (`andora-realm` v0.1.0, Module: auth_api, config, db, handlers, health,
migrations, net, parental, protocol, world). Gemeinsames Protokoll:
`shared/protocol.js` / `shared/protocol.gd`. Architektur: `docs/Login_Realm_Architektur.md`,
`docs/Datenbank_Architektur.md`. Tests: 27 grün (config, handlers, health, migrations, parental, protocol, world).
Listener-Konfiguration: `WS_BIND_HOST` (WS) und `HEALTH_BIND_HOST` (Health-HTTP):
leer = bisheriges `*:port`, `ipv4`/`4`, `ipv6`/`6`, `dual`/`both` bzw. IP-Literal/Hostname
(`src/realm-rs/src/net.rs::bind_addrs`, HTTP-Health via `src/realm-rs/src/health.rs`);
DSN-/AuthAPI-URLs normalisieren IPv6-Literal zu `[host]:port` (`config.rs`).

| Funktion | Status | Funktioniert | Fehlt / Nächstes | Verifikation |
|---|---|---|---|---|
| Netzwerk (WebSocket, Frame-/Protokollverarbeitung) | Teilweise | Verbindung, Framing, unbekannte Typen geloggt | – | Tests |
| Handoff-Einstieg (fail-closed: Token+Session, Account-Gleichheit, Realm-Mismatch) | Fertig | Nur validierter Einstieg; Charakter laden, WELCOME, Nachbar-Spawn | – | Tests |
| C2S `HELLO` (1) | Fertig | verify_entry, Charakter-Load, WELCOME, Nachbarn | – | Tests |
| C2S `HEARTBEAT` (10) | Fertig | Poll-Mechanik | – | Tests |
| C2S `MOVE` (2) | Fertig | Positionswechsel mit Speed-Cap | – | Tests |
| C2S `CHAT` (5) | Fertig | Eltern-Gate, 240-Zeichen-Truncate, AOFB-Broadcast | – | Tests |
| C2S `PARENTAL` (11) | Fertig | Status/PIN/Extension/Periods/Exceptions/Notifications, 10-s-Poll, Force-Logout | – | Tests |
| Welt-Grundgerüst (Tick, AOFB-Radius, Nachbarn-Spawn) | Teilweise | Tick + Radius laufen | Persistente Welt, NPC-Bevölkerung, Zonen-Logik | Tests |
| Realm-DB-Migrationen | Fertig | Läuft beim Start | – | Tests |
| C2S `ATTACK` (3) | Nur spezifiziert | – | Kampfsystem implementieren (Design: `Kampfsystem.md`) | – |
| C2S `PICKUP` (4) | Nur spezifiziert | – | Loot/Claim umsetzen (Design: `Lootsystem.md`) | – |
| C2S `NPC_TALK` (6) | Nur spezifiziert | – | NPC-Dialog/-KI (Design: `Ki-NPC.md`) | – |
| C2S `AUCTION_LIST/BID/BUY` (7–9) | Nur spezifiziert | – | Auktionshaus/Marktplatz (Design: `Auktionshaus und Marktplatz`) | – |

## 4. Realm-Service (Node.js, `src/realm`) – Legacy/Transition

Übergangsdienst, wird vom Rust-Realm (`src/realm-rs`) abgelöst. Funktionierend:
- Handler für `hello`, `heartbeat`, `move`, `chat`, `parental` vorhanden.
- C2S `ATTACK`/`PICKUP`/`NPC_TALK`/`AUCTION_*` nur als Protokoll-IDs, keine Handler.
- DB-Bestand mit `character`/`world_data`/`realm_state` (Übergangs-Schema ohne zentrale
  `world_data` in der Zielarchitektur, siehe `docs/Datenbank_Architektur.md`).
- Bind-Konfiguration: `WS_BIND_HOST`/`HEALTH_BIND_HOST` (leer = bisheriges
  `*:port`, `ipv4`/`4`, `ipv6`/`6`, `dual`/`both` bzw. IP-Literal/Hostname über
  `src/realm/src/bind.ts`), WebSocket via `noServer` + `handleUpgrade` pro Listener
  (`net.ts`), Health-HTTP multi-Listener (`health.ts`); `AUTHAPI_URL`/`OLLAMA_URL`
  normalisieren IPv6-Literal zu `[host]:port` (`config.ts`).
- Quellen und manuell synchronisierte `build/*.js` (gitignored) statisch geprüft;
  kein Node-Toolchain in der Umgebung, daher **nicht** laufzeitgetestet.

## 5. Coordinator (KI-Queue/Ollama) – Nur spezifiziert

`src/coordinator/` enthält nur `README.md`, kein Code. Design: `docs/Coordinator.md`,
`docs/ai_system.md`, `docs/Deployment_Betriebsarchitektur.md` (Queue, Priorisierung,
Spam-/Kontextbudget-Schutz, dateibasierte Queue, Recovery, Crafting-Zuordnung,
andora-updater/Agent).

## 6. Voice – Später vorgesehen

Kein Code, kein Dienst. Spezifikation: `docs/voice_system.md`,
`docs/communication-voice-npc-commands.md`, `docs/chat_system.md`, `docs/parental_control.md`.

## 7. Client (Godot, Repo-Root) – Grundgerüst vorhanden

`project.godot` = „Andora", Godot 3.5, GLES2; Autoload `I18n = res://i18n/Localizer.gd`.
- **Vorhanden:** Ollama-HTTP-Demo (`OllamaClient.gd` → `http://localhost:11434`, Modell
  `llama3`; `OllamaDemo.gd`; `demo_scene.tscn`); leere `Node2D.tscn`; Addons `midi`,
  `modplayer`, `AS2P`; i18n `Localizer.gd` + `de.json`/`en.json`.
- **Fehlt:** WebSocket-Client, `shared/protocol.gd`-Nutzung, Login-UI, Realm-Auswahl,
  Weltdarstellung/Charakter-Rendering, Netzwerk-Lebenszyklus.
- Grundlage für Client-Architektur: `docs/Charaktererstellung_und_Charakterdarstellung.md`.

## 8. Protokoll-/Übersetzungsmodule – Grundgerüst vorhanden

- `shared/protocol.gd` + `shared/protocol.js`: Protokoll-IDs (C2S 1–11 definiert; ohne
  Handler: 3, 4, 6, 7, 8, 9 – siehe Realm-Status oben).
- `i18n/`: `Localizer.gd`, `de.json`, `en.json` (Grundgerüst, Umfang überschaubar).

## 9. Monitor-/Web-Panel (lokal, `monitor/`) – Grundgerüst vorhanden (Übergang)

Lokales Panel (`server.js`, `lib/`: config, envconfig, history, systemctl; Ports 3001–3003).
Wird durch die Zielarchitektur (zentrales Panel + Andora-Agent, mTLS, `andora-updater`)
abgelöst: `docs/monitoring_web_panel.md`, `docs/Deployment_Betriebsarchitektur.md`.
- Bind-Konfiguration: `ANDORA_MONITOR_BIND` (leer/`auto` = 127.0.0.1, `ipv4`/`4`,
  `ipv6`/`6`, `dual`/`both` bzw. IP-Literal/Hostname, `server.js`), Config-Whitelist
  um `WS_BIND_HOST`/`HEALTH_BIND_HOST` erweitert; `ANDORA_GAME_SERVER_URL` normalisiert
  IPv6-Literal zu `[host]:port` (`lib/config.js`).
- Node-Bestand statisch geprüft; kein Node-Toolchain in der Umgebung, daher nicht
  laufzeitgetestet.

## Bereiche nur spezifiziert / später (Auswahl der Design-Doku)

Folgende Spielsysteme sind als Design-Doku vorhanden, aber **noch nicht als Code umgesetzt**
(incl. Realm-/Client-Implementierung, soweit dort kein Eintrag): Klassen &
Charakterprogression (`Klassensystem.md`, `Tier-Progression.md`, `Charaktererstellung_und_Charakterdarstellung.md`),
Items/Inventar/Crafting/AH (`item_properties.md`, `inventory_system.md`, `Crafting*.md`,
`Handwerksystem.md`, `Handwerks_und_Sammelsystem.md`, `Sammelsystem.md`), Quests
(`Quest-System.md`, `quests_stories.md`), Boss/PvP/Arena/Dungeon (`Boss-System.md`,
`Arena.md`, `Dungeon-Finder.md`, `Event-Matchmaking.md`, `Kampfsystem.md`), NPC/KI-Szenen
(`Ki-NPC.md`, `cutscene_system.md`, `ai_cutscene_system.md`), Rassen/Fraktionen/Politik
(`Rassen-Fraktionen.md`, `Rasse_*.md`, `Politik-Herrschaftssystem.md`), Welt/Reisen/
Expansionen (`Welt_Reisesystem.md`, `exp1_*`, `exp2_*`). Housing: **Später vorgesehen**
(`Housing.md`).