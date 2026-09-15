# Bericht: Andora – Questsystem

**Stand:** 13.09.2026
**Basis:** `docs/Quest-System.md`, `docs/quests_stories.md` sowie quer referenzierte Dokumente in `docs/`.

---

## 1. Zusammenfassung

Das Questsystem von Andora befindet sich im **Konzept-/Planungsstadium** (Status „🟡 Konzept“ / „Architektur / Planung“). Es ist umfassend dokumentiert, aber **code-technisch noch nicht implementiert**: Es existiert weder eine Quest-Engine im Rust-Realm-Server noch Lua-Questdefinitionen oder Quest-UI im Godot-Client. Einziger Datenbank-Artefakt ist die Migration `004_quests.sql`.

Kernidee: Quests sollen nicht nur Aufgabenlisten sein, sondern auf Charakter, Klasse, Beziehungen, Gruppen und Ereignisse der Welt reagieren – ohne die Serverautorität aufzugeben.

---

## 2. Grundarchitektur (vierschichtig)

Das System trennt Inhalt, Ausführung, Speicherung und Darstellung strikt:

| Ebene | Rolle |
|---|---|
| **Lua** | Beschreibt, *was* eine Quest ist (ID, Titel, Ziele, Voraussetzungen, Belohnungen, Trigger, i18n-Keys, Klassenbedingungen, Kampf-Überschreibungen). |
| **Realm-Server (Rust)** | Autorität: prüft Voraussetzungen, verarbeitet Events, aktualisiert Fortschritt, wechselt Abschnitte, entscheidet über Abschluss/Fehlschlag, vergibt Belohnungen, setzt Story- und Weltzustände. |
| **MariaDB** | Speichert den **dynamischen Zustand** pro Charakter (Queststatus, aktueller Abschnitt, Fortschrittswerte als JSON). |
| **Godot (Client)** | Reine Darstellung: Questlog, Ziele, Marker, Belohnungsanzeige. Entscheidet nie über Fortschritt oder Abschluss. |

> **Grundregel:** *„Lua beschreibt die Aufgabe. Der Realm-Server (Rust) prüft die Aufgabe. MariaDB merkt sich den Fortschritt. Godot zeigt ihn dem Spieler.“*

Die Runtime-KI (Ollama) kann Dialoge und Hinweise erzählerisch unterstützen, besitzt aber **keinerlei Autorität** über Questzustände (Questlogik bleibt vollständig serverseitig).

---

## 3. Questzustände

| Zustand | Bedeutung |
|---|---|
| `HIDDEN` | Existiert, wird mangels Voraussetzungen nicht angezeigt. |
| `AVAILABLE` | Alle Voraussetzungen erfüllt, kann angenommen werden. |
| `ACTIVE` | Vom Spieler angenommen. |
| `COMPLETED` | Erfolgreich abgeschlossen. |
| `FAILED` | Fehlgeschlagen (nur wo vorgesehen). |

(`quests_stories.md` nennt vereinfachend nur `AVAILABLE / ACTIVE / COMPLETED / FAILED`; `Quest-System.md` ist der Architektur-Anker.)

---

## 4. Questziele

Strukturierte Zieltypen statt reinem Text, damit der Server Fortschritt eindeutig prüfen kann:

```
kill, collect, talk, visit, escort, deliver, discover, craft,
event, heal, protect, taunt, destroy, interact
```

Beispiel (Deliver):
```
type       = deliver
item       = borin_letter_001
target_npc = elara
```

Weitere Zieltypen sind später ergänzbar. Ein Gespräch allein schließt z. B. bei `talk` kein Ziel ab – der Server prüft NPC, Spieler, Questzustand und Bedingungen. Bei `deliver` prüft der InventoryService die Gegenstände und entfernt sie serverseitig erst nach Bestätigung.

---

## 5. Fortschritt über das Eventsystem

Questfortschritt läuft möglichst zentral über das Eventsystem:

```
MonsterKilled → QuestService → aktive Quests suchen → Bedingungen prüfen
→ Fortschritt aktualisieren → Ziel erfüllt? → Abschnitt/Quest fortsetzen
```

Mögliche weitere Ereignisse: `NPCInteraction, ItemCollected, ItemDelivered, AreaEntered, ItemCrafted, NPCHealed, EnemyTaunted, ObjectDestroyed, WorldEventStarted, RegionEventStarted, QuestCompleted`. So entsteht keine parallele Questlogik je Spielmechanik.

---

## 6. Voraussetzungen & dynamische Verfügbarkeit

Lua definiert, *wann* eine Quest verfügbar sein **kann**; der Server entscheidet anhand des tatsächlichen Welt-/Spielerzustands. Mögliche Bedingungen: Level, Klasse, Rolle, vorherige Quest, Questabschnitt, Story-Flag, NPC-Beziehung (z. B. `Beziehung >= 60`), Ruf, Fraktion, Region, World Event, Regions-Event, Weltzustand, besondere Entscheidung, Ereignis.

---

## 7. Klassen-, Gruppen- und Tier-Aspekte

- **Klassenabhängige Quests:** Dasselbe Ereignis erzeugt je Klasse andere Aufgaben (Tank provoziert, Heiler heilt, DD besiegt, Magier zerstört magische Türme).
- **Gruppenquests:** Gemeinsames Ereignis, aber individuelle, zur Klasse passende Ziele – kein gegenseitiger Fortschrittskonflikt, die Gruppe spielt die Quest einmal durch.
- **Klassenspezifische Questreihen:** Eigene Geschichten/Entwicklung pro Klasse (Mechaniken, Fähigkeiten, Orte, NPCs).
- **Tier-Klassenquests:** Zu Tierbeginn; führen in die Tier-Anforderungen ein und geben **Basic-Ausrüstung** des Tiers (nicht wertvoll/außergewöhnlich). Ziel: Solo-Fortschritt ohne Gildenpflicht – „bessere Ausrüstung spart Zeit, schaltet aber nichts exklusiv frei“.

---

## 8. Belohnungen

Mögliche Belohnungen (alle serverseitig validiert und vergeben): Erfahrung, Geld, Gegenstände, Ausrüstung, Ruf, Beziehung, Titel, Freischaltungen, neue Quests, Story-Fortschritt, Bereichszugang, Rezepte, Fähigkeiten (sofern vom Klassensystem vorgesehen).

Die Quest-EXP wird **von der Quest selbst** festgelegt – es gibt keine globale EXP-Formel (siehe `Erfahrung_und_Progressionssystem.md` §10).

---

## 9. Integration mit anderen Systemen

- **NPC-Wissenssystem:** Getrennt, aber kommunikativ – NPCs reagieren nur auf Informationen, die sie tatsächlich erhalten haben (z. B. erst nach Übergabe eines Briefes).
- **Dynamic Scenes:** `QuestStarted`, `QuestStageChanged`, `QuestCompleted`, `QuestFailed` können Szenen auslösen; das Quest-System startet keine Cutscene direkt, sondern gibt über das Eventsystem an das Scene-System ab.
- **Crafting/Hausbau:** Werden als eigene Systeme definiert, aber als Questziele genutzt (CraftingService, BuildingService, InventoryService, NPC-Interaktion).
- **Kampfsystem:** Quest-/Dialogkontext kann Kampf-Uberschreibungen setzen (Angriffserlaubnis, Aggressivität, Verhalten); Questmonster respawnen in 3 Minuten.
- **Koordinierung:** `quest_event = 70` als AI-Priorität für Quest-/Event-NPCs; „Quest annehmen“ ist normaler Realm-Code, keine KI-Entscheidung.

---

## 10. i18n

Sichtbare Questtexte liegen nicht im Server oder Client, sondern als i18n-Keys:

```lua
title_key       = "quest.fieldcamp_defense.title"
description_key = "quest.fieldcamp_defense.description"
```

Godot zeigt die jeweils eingestellte Sprache. UI-Strings (`quest_available`, `quest_completed`, `quest_failed`, …) existieren bereits in `i18n/*.json` (de/en/zh-Hans/zh-Hant).

---

## 11. Persistenz (MariaDB)

Migration `src/realm-rs/migrations/004_quests.sql` (einzige quest-bezogene Migration von 18):

```sql
CREATE TABLE IF NOT EXISTS quests (
  char_id   INT NOT NULL,
  quest_id  VARCHAR(48) NOT NULL,
  state     TINYINT NOT NULL DEFAULT 0,
  data      JSON NULL,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (char_id, quest_id),
  FOREIGN KEY (char_id) REFERENCES characters(id) ON DELETE CASCADE
);
```

Dynamische Beispiele aus den Dokumenten: `objective_progress` als JSON wie `{ "wolves": 4, "soldiers_healed": 7, "tower_destroyed": true }`; Felder `character_id, quest_id, state, objective_progress, started_at, completed_at, relevant_choices`. Statische Questdefinitionen werden bewusst **nicht** pro Spieler gespeichert.

> **Hinweis/Abweichung:** `Datenbank_Architektur.md` §19 nennt die Tabelle `character_quests`; die tatsächliche Migration heißt `quests`. Bei der Implementierung ist diese Diskrepanz aufzulösen.

---

## 12. Erweiterbarkeit / Questtypen

Geplant sind: Soloquests, Gruppenquests, Klassenquests, Tier-Klassenquests, Questketten, verborgene Questketten (ohne sichtbaren Questgeber), Regionsquests, World-Event-Quests, Realm-Event-Quests, NPC-Beziehungsquests, Craftingquests, Entdeckungsquests, Rätsel-Quest, zeitlich begrenzte Quests, Dynamic-Scene-Quests und besondere Storyquests. Neue Typen sollen über Lua-Definitionen und vorhandene Servermechaniken entstehen statt über Sondercode.

---

## 13. Besondere Nutzung in der Welt (exemplarisch)

- **Fraktionsquestreihen** (Rassen-Fraktionen): Aufnahme, Verrats-/Überläufer-, Wiedergutmachungsquests.
- **Spielererzeugte Quests** über die Auftragsgilde (Regions-Dokument Mandalonien).
- **Verborgene Questketten** (Storytelling/Weltgeheimnisse): ausgelöst durch Bücher, Gegenstände, Orte, Hinweise, Artefakte.
- **Quests als Grind-Alternative** und Hinweise auf verborgene Ketten (Unterwelt).
- **Erfolge/Titel** können durch verborgene Questketten freigeschaltet werden.

---

## 14. Implementierungsstatus

| Teilbereich | Status |
|---|---|
| Architektur-/Konzeptdokumentation | Vollständig (`Quest-System.md`, `quests_stories.md`) |
| DB-Migration `quests` | Vorhanden (Migration 004) |
| Quest-Engine (Rust) | **Nicht implementiert** (kein Quest-Modul in `src/realm-rs/src/`) |
| Lua-Questdefinitionen | **Nicht vorhanden** (keine `.lua`-Spieldateien) |
| Quest-UI / Questlog (Godot) | **Nicht implementiert** |
| Quest-EXP | Nicht eingebaut (Projekt-Status) |
| Definitives Beispiel-Quest-ID | Nur exemplarisch: `borin_iron_ore` (Lua), `intro_001`/`monster_001` (Legacy-Prototyp, gitignored) |

`Projekt-Status.md` bestätigt: „Entdeckungs-/Quest-EXP … noch nicht implementiert“, „Loot-/Quest-Aufrufer offen“. Bewusst nicht umgesetzt sind z. B. konkrete Tutorialquests (Klassensystem) und das Questsystem im Gruppensystem-V1 (steht dort auf späterer Agenda).

---

## 15. Empfehlungen / offene Punkte

1. **Namen angleichen:** `character_quests` (Architektur) vs. `quests` (Migration) vereinheitlichen.
2. **State-Legacy:** Docs teils 4, teils 5 Zustände – bei Implementierung auf `Quest-System.md` (5 Zustände inkl. `HIDDEN`) fixieren.
3. **Referenzen:** Zahlreiche weitere Systeme verweisen auf das Quest-System (Kampf-Überschreibungen, Respawn-Zeiten, KI-Prioritäten, Achievement-Bindung) – diese Abhängigkeiten bei der Implementierung mit abdecken.
4. **Exemplarische Questdaten fehlen:** Konkrete Quest-IDs, Ziele und Belohnungen sind bisher nur als Strukturbeispiele dokumentiert.

---

*Quellen: `docs/Quest-System.md` (770 Z., Architektur, Konzept), `docs/quests_stories.md` (681 Z., Inhalt/Planung), `src/realm-rs/migrations/004_quests.sql`, `docs/Projekt-Status.md`, `docs/Datenbank_Architektur.md`, `docs/Erfahrung_und_Progressionssystem.md`, `docs/Kampfsystem.md`, `docs/Coordinator.md`, `docs/Storytelling_und_Weltgeheimnisse.md`, `docs/Rassen-Fraktionen.md`, `docs/item_properties.md`, `i18n/*.json`.*