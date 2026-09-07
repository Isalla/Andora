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
| 5 | Realm-Grundsystem | Teilweise | Einstieg, Charakter, Chat, Parental fertig; fehlen u. a. persistente Welt, Kampf, Loot, NPC, Auktion |
| 6 | Coordinator | Eingebaut | Go-Dienst `src/coordinator`: dateibasierte KI-Queue, Priorisierung, Cooldown, mehrsprachige Input-/Output-Sperrwortfilter (Sprachdateien), Ollama-Anbindung (angeschlossener lokaler Provider; Architektur providerunabhängig, siehe `docs/Coordinator.md` §3.1), Ergebnis-Callback/Poll; Realm-Anbindung offen |
| 7 | Voice | Später vorgesehen | Kein Code, kein Dienst |
| 8 | Client | Grundgerüst | Godot-Rahmen mit Demo; Netzwerk und Login fehlen |
| 9 | Realm (Node.js) | Legacy | Übergang, wird durch den Rust-Realm abgelöst |
| 10 | Monitor-/Web-Panel | Eingebaut | PHP-Panel unter `web/andora-monitor` funktionsfähig (Status/Spieler/History/Config/Control); Node-Panel Legacy |
| 11 | Protokoll-/i18n-Module (shared) | Grundgerüst | Gemeinsame Protokoll-IDs und Übersetzungsrahmen |
| 12 | Andora-Agent | Eingebaut | Go-Dienst `src/agent`: lokaler Verwaltungsdaemon (systemctl/journalctl via `sudo -n`, Health/Version, Token-Auth Zwischenlösung, mTLS-Vorbereitung, Tests); Panel-Anbindung in `web/andora-monitor` (`via: agent`, Legacy-Fallback) |
