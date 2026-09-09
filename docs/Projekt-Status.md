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
| 5 | Realm-Grundsystem | Teilweise | Einstieg, Charakter, Chat, Parental fertig; **Combat V1 eingebaut** (Auto-Grundangriff, Ziel-/Reichweitenvalidierung, Trefferauflösung Miss/Dodge/Parry/Block/Normal/Krit, Waffenschaden, Rüstungsreduktion mit Klassen-Caps, Tod/KILL; vorläufige Balancingwerte via `COMBAT_*`-Config, siehe `Kampfsystem.md`); **Combat V2 eingebaut** (NPC-/Monster-Ebene: getrennte `attackable`/`aggressive`, kontextabhängige Überschreibungen mit Auslaufen, Home-Zone/Leash + Evade/Return als Combat-Reset, Content-Respawn inkl. Persistenz über Realm-Neustarts, Aggroformen Solo/sozial/Rudel, Boss-Claim mit erstem Schadensverursacher; Content über Migration 009 mit Probe-Spawnzone 0, siehe `Kampfsystem.md` §§18–21, `Boss-System.md` §§2–6); **Combat V3 eingebaut** (Fähigkeits-Engine: Realm-autoritatives Cast-/Ability-System mit Mana, Cooldowns, AoE-Zielauswahl, Effektmodell Buff/Debuff/DoT/HoT/Stun/Silence/Root, Bewegungs-Interrupt, Tod-Reset; Content als DB-Schicht `ability_definitions`; C2S ABILITY=12, S2C ABILITY=16, EFFECT=17; modular in `src/realm-rs/src/combat/`, Migration 010, siehe `Ability-System.md` und `Kampfsystem_V3_Wiederverwendung.md`); fehlen u. a. persistente Welt, Loot, Auktion |
| 6 | Coordinator | Eingebaut | Go-Dienst `src/coordinator`: dateibasierte KI-Queue, Priorisierung, Cooldown, mehrsprachige Input-/Output-Sperrwortfilter (Sprachdateien), Ollama-Anbindung (angeschlossener lokaler Provider; Architektur providerunabhängig, siehe `docs/Coordinator.md` §3.1), Ergebnis-Callback/Poll; Realm-Anbindung offen |
| 7 | Voice | Später vorgesehen | Kein Code, kein Dienst |
| 8 | Client | Grundgerüst | Godot-Rahmen mit Demo; Netzwerk und Login fehlen |
| 9 | Realm (Node.js) | Legacy | Übergang, wird durch den Rust-Realm abgelöst |
| 10 | Monitor-/Web-Panel | Eingebaut | PHP-Panel unter `web/andora-monitor` funktionsfähig (Status/Spieler/History/Config/Control); Node-Panel Legacy |
| 11 | Protokoll-/i18n-Module (shared) | Grundgerüst | Gemeinsame Protokoll-IDs und Übersetzungsrahmen; Locales de/en/zh-Hans/zh-Hant (`i18n/*.json`, en = Master), Client (Godot `Localizer.gd`) und Node-Server (`src/realm/i18n.js`) laden dieselben Dateien |
| 12 | Andora-Agent | Eingebaut | Go-Dienst `src/agent`: lokaler Verwaltungsdaemon (systemctl/journalctl via `sudo -n`, Health/Version, Token-Auth Zwischenlösung, mTLS-Vorbereitung, Tests); Panel-Anbindung in `web/andora-monitor` (`via: agent`, Legacy-Fallback) |
