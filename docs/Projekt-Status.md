# Andora – Projekt-Status

**Zweck:** Sehr kurze Übersicht, welche größeren Funktionen/Systeme des Andora-Projekts bereits
umgesetzt sind und welche noch fehlen. Technische Details bleiben bei den
Detaildokumenten in `docs/` (Architektur, Spezifikationen, Betrieb).

**Maßstab:** Status wird am Code gemessen, der tatsächlich im Repository existiert –
nicht an Design-Dokumenten.

**Pflege:** Die Coding-KI aktualisiert diese Datei bei jeder Änderung des tatsächlichen
Implementierungsstands automatisch selbst (Pflicht laut `docs/ai_jobs.md`).

| Nr | Funktion / System | Status | Hinweis |
|---|---|---|---|
| 1 | Elternsystem | Eingebaut | In Auth (Konten) und Realm (Chat, Parental-Panel) |
| 2 | IPv4/IPv6/Dual-Stack | Eingebaut | Alle Server-Dienste; Node-Bestand statisch geprüft (kein Node-Toolchain) |
| 3 | Sicherheitsebene / Auth | Eingebaut | Einziger Dienst mit direktem Auth-DB-Zugriff |
| 4 | Loginserver | Eingebaut | Brücke zu Auth, kein eigener DB-Zugriff |
| 5 | Realm-Grundsystem | Teilweise | Einstieg, Charakter, Chat, Parental fertig; **Grundattribute eingebaut** (`attributes.rs`: 7 Attribute, DB via Migration 011 – neue Spalten neutral Default 10, **keine Start-/Rassenverteilung beschlossen**; aktive Wirkungen Kraft/Con/Int/Wis/Luck/End an Combat/Ability/Regeneration; Rassenverteilung + konkrete Ausrüstungsboni offen); **HP-/Mana-Regeneration eingebaut** (gemeinsames Tick-System `regen.rs`: Grundformel `(Basis + additive Boni) × Zustandsmultiplikator`, absolute Werte/s, Zustände 15 %/100 %/125 %, Klassen-Basiswerte + Levelwachstum, f64-Carry, Deckelung bei Max); **Combat V1 eingebaut** (Treffer/Block/Krit mit Attributs-Bonus, Rüstungsreduktion mit Endurance und Klassen-Caps); **Combat V2 eingebaut** (NPC/Monster: Aggro, Evade/Return, Boss-Claim, Content via Migration 009); **Combat V3 eingebaut** (Ability-Engine: Cast, Mana, Cooldowns, AoE, Effekte, DoT mit Intelligenz-Skalierung, Migration 010); fehlen u. a. persistente Welt, Loot, Auktion, rassische Attributverteilung |
| 6 | Coordinator | Eingebaut | Go-Dienst `src/coordinator`: dateibasierte KI-Queue, Priorisierung, Cooldown, mehrsprachige Input-/Output-Sperrwortfilter (Sprachdateien), Ollama-Anbindung (angeschlossener lokaler Provider; Architektur providerunabhängig, siehe `docs/Coordinator.md` §3.1), Ergebnis-Callback/Poll; Realm-Anbindung offen |
| 7 | Voice | Später vorgesehen | Kein Code, kein Dienst |
| 8 | Client | Grundgerüst | Godot-Rahmen mit Demo; Netzwerk und Login fehlen |
| 9 | Realm (Node.js) | Legacy | Übergang, wird durch den Rust-Realm abgelöst |
| 10 | Monitor-/Web-Panel | Eingebaut | PHP-Panel unter `web/andora-monitor` funktionsfähig (Status/Spieler/History/Config/Control); Node-Panel Legacy |
| 11 | Protokoll-/i18n-Module (shared) | Grundgerüst | Gemeinsame Protokoll-IDs und Übersetzungsrahmen; Locales de/en/zh-Hans/zh-Hant (`i18n/*.json`, en = Master), Client (Godot `Localizer.gd`) und Node-Server (`src/realm/i18n.js`) laden dieselben Dateien |
| 12 | Andora-Agent | Eingebaut | Go-Dienst `src/agent`: lokaler Verwaltungsdaemon (systemctl/journalctl via `sudo -n`, Health/Version, Token-Auth Zwischenlösung, mTLS-Vorbereitung, Tests); Panel-Anbindung in `web/andora-monitor` (`via: agent`, Legacy-Fallback) |
