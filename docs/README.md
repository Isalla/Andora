# Andora – Inhaltsverzeichnis der Projektdokumentation

**ZWECK:** Zentraler Einstiegspunkt für KI-/Coding-Aufgaben. Zuerst diese Übersicht lesen, danach nur die für die Aufgabe relevanten Dokumente öffnen. Keine Dokumente anfangen, bevor die passende Kategorie unten identifiziert ist.

**PFLICHT VOR JEDEM CODING-AUFTAG:** [ai_jobs.md](./ai_jobs.md) (Autonomie-/Kontextreserve-Regeln) lesen. Projektinformationen beziehen neue KI-Sessions ausschließlich aus den aktuellen Dokumenten dieser Übersicht (`docs/`) und, soweit Fachliteratur relevant ist, aus [references/README.md](../references/README.md) – nicht aus `.tmp/ai-context` oder sonstigen Übergabedokumenten.

## Referenzbibliothek (references/)

Unter `references/` befindet sich die lokale technische Referenzbibliothek; Einstiegspunkt und Index ist [references/README.md](../references/README.md). Bei technischen Aufgaben prüft die KI selbstständig, ob dort passende Fachliteratur vorhanden ist, insbesondere zu Themen wie Go, Rust, Netzwerkarchitektur, Datenbanken und Migrationen, Sicherheit, Nebenläufigkeit, Performance, verteilte Systeme, APIs und Microservices. Die Literatur dient ausschließlich als technische Unterstützung: Die Andora-Dokumentation unter `docs/` bleibt für alle projektspezifischen Architektur-, Design- und Systementscheidungen verbindlich und hat bei Widersprüchen Vorrang. Die Nutzung der Referenzbibliothek ist damit eine dauerhafte Regel dieses Dokuments und muss nicht in jedem einzelnen Arbeitsauftrag ausdrücklich erwähnt werden.

## Einstieg

| Schritt | Datei | Warum |
|---|---|---|
| 1. Projektübergreifend | [project_overview.md](./project_overview.md) | Grundidee, Plattformen, Server-Autorität, alle Kernsysteme in übergeordneter Form |
| 2. Technisches Fundament | [architecture.md](./architecture.md) | Stack (Godot 3.5, fünf Serverdienste: Go-API/Auth, Login, Realm/Rust, Coordinator, Voice; MariaDB, WebSocket), Zielplattformen (linux-amd64/arm64), Verzeichnis- und exp1_/exp2_-Präfix-Regeln |
| 3. KI-Grundregel | [ai_system.md](./ai_system.md) | Zentrale KI-Architektur: Server ist Autorität, AI-/Narrative-Context, spezialisierte KI-Systeme, Budget, Fallbacks |

## Kategorien

### 1. KI-Backend & Dev-Prozess
- **ai_jobs.md** – Verhaltensregeln für die Entwicklungs-KI bei langen Aufträgen: autonom weiterarbeiten bei Fehlern/leeren Anfragen, nur bei zwingenden fachlichen Entscheidungen fragen, Kontextreserve (~20k Tokens) vor dem Limit wiederherstellen. Enthält außerdem die Toolchain-/Temporärdatei-Regeln: Go-Toolchain liegt im Projekt unter `.tmp/go` (jeder neue Auftrag nutzt sie für gofmt/vet/build/test; temporäre Toolchains und Downloads ausschließlich unter `.tmp/`), `/etc` und `/tmp` werden für Coden und Kompilieren NICHT benutzt, OS-Info via `.tmp/os-release` statt `/etc/os-release`; Test-Binärdateien ARM64, Produktions-Binärdateien ARM64 UND AMD64 (Cross-Compilation). Außerdem die Kurzregeln zur OpenCode-Session- und Datenbankpflege (Details in `OpenCode_Session_Pflege.md`).
- **OpenCode_Session_Pflege.md** – Pflege der OpenCode-Sessions/Datenbank (`~/.local/share/opencode/opencode.db`): Sessions sind temporäre Arbeitsdaten; automatische Löschung ohne Rückfrage bei letzter Aktivität älter als 3 Tage, Ausnahme bei nicht in Git gesicherter relevanter Projektarbeit; Datenbankpflege nur ohne parallelen OpenCode-Prozess (Backup, Freelist-/Kompaktierungsprüfung, Integritätsprüfung, Teststart); offizielle OpenCode-Löschmechanismen haben Vorrang; keine Konfiguration, Skills, Provider-/Modell-Einstellungen oder sonstigen nicht sessionspezifischen Daten entfernen.
- **ai_system.md** – Zentrale KI-Architektur von Andora (Interpreter-Prinzip, Kontexte, asynchrone Verarbeitung, spezialisierte KI-Module).
- **Coordinator.md** – KI-Queue-/Ollama-Service: Sicherheitsgrenze, Priorisierung, Spam- und Kontextbudget-Schutz, dateibasierte Queue, Recovery, Crafting-Zuordnung.
- **monitoring_web_panel.md** – Doku des Admin-/Monitoring-Web-Panels: **derzeit als lokales Panel umgesetzt (Übergangsstand)**, Ports 3001–3003, `/status`/`/players`, systemd-Start, offene Punkte; Zielarchitektur (zentrales Panel + Agent) in `Deployment_Betriebsarchitektur.md`.
- **Deployment_Betriebsarchitektur.md** – Verbindliche Betriebs-/Deployment-Architektur: fünf getrennte Serverdienste (eigenständig betreibbar, linux-amd64 + linux-arm64), zentrales Admin-/Deployment-Panel, Andora-Agent pro Server (mTLS, ohne Remote-Shell), Nicht-Root-Benutzer `andora`, `andora-updater` (signierte Manifeste, Prüfsummen, Healthchecks, Rollback, inkl. Agent), automatisierte Realm-Updates (Wartungsmodus → Shutdown → Backup → Update → Migration → Healthcheck → Freigabe) und parallele Realm-Versionen (Live, Classic, Test, Event).

### 2. Architektur, Auth & Datenbank
- **architecture.md** – Technische Gesamtarchitektur (siehe Einstieg).
- **Deployment_Betriebsarchitektur.md** – Betriebs-/Deployment-Architektur der fünf Serverdienste (Panel, Agent, mTLS, Updater, Realm-Updates; siehe auch Kategorie 1).
- **Auth_API_Architektur.md** – Go-Auth-/API-Sicherheitsservice: Service-Auth, Berechtigungen, Datenminimierung, Secrets, Auth-DB-Migrationen.
- **Login_Realm_Architektur.md** – Account-, Login- und Realm-Architektur: Account-DB, World-Server-Auth, Realm-Auswahl, Realm-Versionen, Charakter-Transfer, Fresh-Start-Sperre.
- **Datenbank_Architektur.md** – MariaDB-Aufteilung (auth, realm_state_<realm> mit statischen + dynamischen Realm-Daten; keine zentrale world_data), Realm-Isolation, Realm-Versionen, Charaktertransfer, Crafting-Jobs, DB-Benutzer/Verbindungen.
- **parental_control.md** – Elternkontrolle: accountgebunden, serverseitig, 30-Min-Warnung, Tagesausnahmen/Ferien, optionale Eltern-E-Mail mit Änderungsbenachrichtigungen (ohne PINs/Geheimnisse), BLOCKED/Puffer (15 Min. So–Do, 30 Min. Fr–Sa, kein Re-Login im Puffer), temporäre Session-Ausnahmen (Ingame-Elternpanel: +1 h einmal pro Kalendertag, temporäre Mechanismus-Freischaltungen), Berechtigungen in Sonderzeiträumen, Voice-Berechtigung und elterliche Voice-Sperre (Spieler-Voice; getrennt von NPC/KI-Sprachinteraktion), Datenstruktur.
- **Temporäre_Dateien.md** – Dev-Konvention: temporäre Dateien nur in `.tmp/` des Projekts, atomare Schreibvorgänge, Queue-/Recovery-Dateien.

### 3. Worldbuilding, Lore & Rassen
- **characters_world.md** – Kurze Charakter-/Welt-Übersicht (Helen, 5 Rassen, Level 40, Story-Grundzüge).
- **Rassen-Fraktionen.md** – Rassen-/Fraktionen-Design: spielbare Rassen, Rasse ≠ Fraktion, 3 gemischte Fraktionen, Wechsel, Verrats-Quest, Ruf.
- **Rasse_Menschen.md** – Worldbuilding Doku: Herkunft, Lebensweise, Landschaft, Architektur, spielmechanische Ausrichtung.
- **Rasse_Elfen.md** – Worldbuilding Doku: Herkunft, Lebensweise, elfische Hauptstadt, Architektur, Fraktionsprägung.
- **Rasse_Andorer.md** – Worldbuilding Doku: Handelsdorf, Handel, Handwerk, Jäger, Glück/Weisheit, visuelle Identität.
- **exp1_Rasse_Luzilla.md** – Worldbuilding Doku Rasse Luzilla (Exp 1): Herkunft, unterirdische Hauptstadt, Schmiedekunst, Kultur, Mechaniken.
- **exp2_Rasse_Mandalonier.md** – Worldbuilding Doku Rasse Mandalonier (Exp 2): Herkunft, Hauptstadt, Gesellschaft, Kampfkunst, Handwerk, Patrouillen.
- **Politik-Herrschaftssystem.md** – Politik-/Herrschafts-Design: Machterhalt/-übernahme, Königsamt, Fraktionskriege (spätere PvP-Phase).
- **Welt_Reisesystem.md** – Welt-/Reise-Design: nahtlose Großgebiete, Portalsteine, Sonderportale, Ladebildschirme.
- **exp1_Unterwelt.md** – Unterwelt-Expansion (Exp 1, Luzilla): Zugang, Ebenen, Wächter-/Raidbosse, Portale, Ökosysteme, unterirdisches Housing.
- **exp2_Region_Mandalonien_Gildenstadt_Wirtschaft.md** – Gildenstadt-/Wirtschaftssystem (Exp 2): Gebäudeausbau, Steuern, Handel, Transport/Karren, Spieleraufträge.
- **exp2_Region_Mandalonien_Ruf_und Woechentliches_Event.md** – Ruf-/Patrouillen-/Wochen-Event (Exp 2): persönlicher/Gildenruf, wöchentlicher Zyklus, Angriffsarmee, Belagerung.
- **MMO-Systeme-Ideensammlung.md** – Ideensammlung für spätere Systeme (Backlog, keine Entwicklungsreihenfolge): Skills, Lore, Explorations-, Fraktions-, PvE-/Raid- und Wirtschaftsideen.

### 4. Charakter, Klasse & Progression
- **Charaktererstellung_und_Charakterdarstellung.md** – Charaktererstellung + clientseitige Darstellung: serverseitige Daten, kosmetische Character-Sets, Mod-Unterstützung, Architekturgrenzen.
- **Klassensystem.md** – Klassenbaum: Abenteurer → 4 Grundklassen à 2 Unterklassen, Rollen, nicht rassen-/fraktionsgebunden.
- **Tier-Progression.md** – Progressionsprinzip: keine feste Straße, Level als Ausgleich, Zeit vs. Ausrüstung, Spielertypen (Solo/Gilden), Wege (Dungeon-Finder, Crafting/AH).
- **Lootsystem.md** – Lootdesign: Berechtigung/Claim, Truhen, Gruppen-Loot (FFA/Group-LE/Würfeln), Tabellen, gebundene Gegenstände.

### 5. Kampf, Bosse & PvP-Systeme
- **Kampfsystem.md** – Kampfdesign: Anvisieren, Angriffe, Fähigkeiten, Bewegung, Ressourcen, Tempo, Aggro/Rollen, Tod/Respawn, Fraktionskämpfe.
- **Boss-System.md** – Bosskämpfe (Gebiet/Dungeon/Raid): Claim, Respawn, Wellen, Gegner pro Welle, Boss-Loot.
- **Arena.md** – Arena-System (PvP-Instanz): Match-Kontext im RAM, Sieg-/Niederlagenregeln, 0 HP ≠ Welt-Tod, Söldner, Spectator-Modus.
- **ai_cutscene_system.md** – Arena-Doku plus Cutscene-/Scene-Lock-Bereich (NPC-Szene sperren, Freigabe). Siehe „Konflikte“ unten.
- **Dungeon-Finder.md** – Gruppensuche: Anmeldung, beidseitige Zustimmung, Serverautorität, Abgrenzung zu Event-Matchmaking.
- **Event-Matchmaking.md** – Event-Matchmaking: zufällige Gruppen, Wettkampf-Formate, Belohnungen.

### 6. Crafting, Items, Handwerk & Wirtschaft
- **Crafting_Grundprinzip.md** – Grundprinzip der Handwerksberufe: gemeinsames System, aktiver Herstellungsprozess, persönlicher Crafting-Wert, Qualitätsstufen, Berufszuteilung.
- **Crafting.md** – Quality-/Overcap-Regeln: Quality-Stufen 1–6, Loot-Range, Master-Chest, Crafting-Influencen, Overcap-Grenze.
- **Handwerksystem.md** – Spezialisierungs-/Handwerksbaum: Startgebiet als Orientierungsphase, Abenteurer/Kunsthandwerker, Berufswahl.
- **Handwerks_und_Sammelsystem.md** – Handwerk + Sammeln kombiniert: Sammelskills, drei Versuche, Rare-Rohstoffe, Minispiel, Fertigungsqualität.
- **Sammelsystem.md** – Sammelgrundprinzip: Mindestskill/Erfolgschance, Vorkommen, Leitprinzip.
- **item_properties.md** – Item-Eigenschaften: Kernattribute, Quality-/Rarity, Equipment-Ausprägungen (Waffen/Rüstung/Amboss), Lifecycle, Bindung.
- **inventory_system.md** – Inventarsystem: Item-Struktur, Quality-Color-System, Level-Anforderungen, Equipment-Tiers, Rucksack-Expansion, Integration (Boss/Quest/Cutscene/Crafting).
- **[Auktionshaus und Marktplatz](./Auktionshaus%20und%20Marktplatz)** (Achtung: keine `.md`-Endung) – Design des Auktionshauses/Marktplatzes: Kaufgesuche, (Teil-)Erfüllung, AH-Guthaben, asynchroner/Offline-Handel.

### 7. Quests & Story
- **Quest-System.md** – Quest-Architektur: Lua-/Realm-Server-(Rust)-/MariaDB-Aufteilung, Questzustände, eventbasierter Fortschritt, dynamische Verfügbarkeit, Klassen-/Gruppenquests.
- **quests_stories.md** – Quest-/Story-Inhalt: Hauptgeschichte, Questdefinitionen, strukturierte Ziele (Kill/Collect/Talk/…), Fortschritt, Belohnungen, KI-/Scene-Integration.

### 8. NPC, KI-Dialog & Szenen
- **Ki-NPC.md** – Doku/Aufgabe für dynamisches NPC-, Informations- und Beziehungssystem: NPCs als Einmal-Personen, Beziehungs-/Wissens-/Nachrichten-/Reisesystem, Raid-Übergang, Ollama-Aufgabe, Fehlerfälle.
- **cutscene_system.md** – Architektur für Cutscenes/Dynamic Scenes: Auslösung (Trigger, Gebietstrigger, Quest/Boss/World-Event), Scene-States (PENDING/RUNNING/PAUSED/FINISHED), alte vs. aktuelle Architektur.
- **communication-voice-npc-commands.md** – Kommunikations-/Voice-spezifikation: Chat-Kanäle (Say/Nähe/Lokal/Gruppe/Gilde), Voice-Regeln, private NPC-/Söldnerbefehle, Companion Push-to-Talk.
- **voice_system.md** – Zentrale Voice-Doku: fasst alle festgelegten Voice-Regeln zusammen (Trennung von Spieler-Voicechat und sprachbasierte Spiel-/KI-Steuerung, serverseitige maximale Rechte, Deaktivierung erlaubter Funktionen, serverseitige Speicherung/Geräteunabhängigkeit, nur Übertragung für aktivierte+erlaubte Kanäle, Kanal-Mechanik, private Begleiter-/Söldnerbefehle + Companion-PTT, SPI-Kette, Voice unter Elternkontrolle); verweist auf communication-voice-npc-commands.md (Kanäle, Companion-Details), chat_system.md (Rechte/Logging) und parental_control.md.
- **chat_system.md** – Chat- und Kommunikationsregeln: serverseitige Speicherung der Kommunikationsrechte und Voice-Kanäle (Server definiert maximale Rechte, Spieler kann erlaubte Funktionen deaktivieren, Voice-Streams nur für aktivierte+erlaubte Kanäle, bei Gerätewechsel erhalten), Chatfilter + Public-Chat/Voice-Deaktivierung unter Elternkontrolle, Private Nachrichten nur mit Freundesliste & Systemnachrichten, Spieler-Voice nur mit elterlicher Freigabe, Chat-Logging (nur interne Account-IDs, keine Namen/E-Mails, ID-Auflösung für berechtigtes Verwaltungs-/Moderationssystem) – offene Bereiche als „nicht definiert“ markiert.

### 9. Platzhalter
- **Housing.md** – Nur Status „Geplant“ (späterer Abschnitt): erst Welt, NPCs, Items, Quests, Combat, Persistenz.

## Enge Dokumentepaare / Cluster (zusammen lesen)
- **KI-Layer:** ai_system.md (Zentralarchitektur) ↔ Coordinator.md (Queue/Ollama-Runner) ↔ Ki-NPC.md (Detail-Aufgabe) ↔ ai_cutscene_system.md (Cutscene-/Scene-Lock).
- **Crafting:** Crafting_Grundprinzip.md + Crafting.md + Handwerksystem.md + Handwerks_und_Sammelsystem.md + Sammelsystem.md (eines impliziert die anderen).
- **Items:** item_properties.md + inventory_system.md + Crafting.md (Quality-System dreht sich um dieselben Stufen).
- **Quests:** Quest-System.md (Architektur) + quests_stories.md (Inhalt).
- **Rassen & Fraktionen:** Rassen-Fraktionen.md (Rahmen) + Rasse_*.md / exp*_Rasse_*.md (Details) + Politik-Herrschaftssystem.md (PvP-Phase).
- **Auth/DB:** Auth_API_Architektur.md + Login_Realm_Architektur.md + Datenbank_Architektur.md + Deployment_Betriebsarchitektur.md + parental_control.md.
- **PvP/Kampf:** Kampfsystem.md + Boss-System.md + Arena.md (+ ai_cutscene_system.md) + Dungeon-Finder.md + Event-Matchmaking.md.
- **Kommunikation:** voice_system.md (Zentraldokumentation: Voice-Mechanik, Rechte, Speicherung, Deaktivierung; verweist auf Kanal-/PTT-/Companion-Details, Elternkontrolle, Chat-Logging) + chat_system.md (Chat-/Voice-Regeln, Logging) + communication-voice-npc-commands.md (Kanäle, Voice-Mechanik, NPC-Sprachbefehle) + parental_control.md (Chat-/Voice-Gates unter Elternkontrolle).
- **Expansionen:** exp1_Rasse_Luzilla.md + exp1_Unterwelt.md (Exp 1) und exp2_Rasse_Mandalonier.md + exp2_* (Exp 2) jeweils zusammen.

## Inhaltliche Überlappungen / mögliche Konflikte
- **Arena.md vs. ai_cutscene_system.md:** `ai_cutscene_system.md` enthält im Wesentlichen Arena-Inhalt PLUS die Section „Cutscene/Scene-Lock“. Doppelte Wartungsrisiken; beim Ändern beider konsistent halten.
- **cutscene_system.md vs. ai_cutscene_system.md:** Getrennte Zuständigkeiten (Architektur/Trigger vs. Arena/Scene-Lock), aber beide betreffen NPC-Szenen und Boss-Cutscenes – bei Änderungen in einer prüfen, ob die andere betroffen ist.
- **Crafting-Cluster:** Vier Dokumente beschreiben dieselben Berufe (Schmied, Schneider, Alchemist, Juwelier) und Qualitätsstufen; bei Wert-/Namenänderungen in allen vier konsistent halten.
- **Quality-/Rarity-Bezeichnungen:** `item_properties.md`, `inventory_system.md` und `Crafting.md` definieren alle ein Quality-System – Prüfen, ob Stufenbezeichnungen/-werte (z. B. Poor/Common/… vs. Gray/Green/Blue/…) überall identisch sind, bevor Code geschrieben wird.
- **Fraktionen/Politik:** `Rassen-Fraktionen.md` (3 Fraktionen, spielernah) vs. `Politik-Herrschaftssystem.md` (spätere PvP-/Königsphase) – unterschiedliche Entwicklungsphasen; nicht parallel implementieren.
- **Quests:** `Quest-System.md` (Zustände/Ausführung) und `quests_stories.md` (Ziele/Story) definieren beide Questziele – bei Zieltypen-Diskrepanz `Quest-System.md` als Architektur-Anker nehmen.
- **AI-Job-Verhalten:** `ai_jobs.md` (Meta-/Dev-Ebene) ↔ `Coordinator.md` (Runtime-Queue) ↔ `ai_system.md` (§ Fehlerbehandlung): inhaltlich überlappend, aber bewusst getrennt – `ai_jobs.md` gilt für die Entwicklungs-KI, die anderen für das Runtime-KISystem.

## Hinweise
- `Auktionshaus und Marktplatz` hat KEINE `.md`-Endung (Link o. a. entsprechend).
