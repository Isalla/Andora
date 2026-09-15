# Andora – Andora Studio (Grundkonzept)

## Status

**Konzeptdokument – NICHT implementiert.**

Dieses Dokument beschreibt das langfristig geplante **Andora Studio**: ein speziell für Andora entwickeltes Content- und Worldbuilding-Werkzeug auf Basis von Godot.

Es gelten klare Kennzeichnungen:

* **Bereits entschieden:** was dieses Dokument verbindlich als Ziel festhält.
* **Langfristiges Ziel:** was angestrebt, aber noch nicht umgesetzt ist.
* **Noch offen:** was bewusst erst bei der späteren Entwicklung entschieden wird.
* **Nicht implementiert:** Andora Studio existiert heute nicht und darf nicht den Eindruck erwecken, es gebe es bereits.

Es wird **kein Code** beschrieben, der bereits existiert.

Die Detailkonzepte der Content-KI (Arbeitsbereiche, Content-Katalog, READ/COMPOSE/CREATE, Pipeline, kein produktiver DB-Zugriff) sind in `Content-Studio.md` festgehalten und werden hier **bewusst nicht dupliziert**. Dieses Dokument beschreibt die Studio-Plattform als Ganzes und referenziert das Content-Konzept an den passenden Stellen.

---

## 1. Ziel von Andora Studio

Andora Studio ist ein speziell für Andora entwickeltes Content- und Worldbuilding-Werkzeug auf Basis von Godot.

Es ist **kein allgemeiner Game Creator**.

Das Studio soll langfristig verschiedene Entwicklungsbereiche von Andora unter einer gemeinsamen Oberfläche zusammenführen.

### Grundprinzip: Mensch und KI nutzen dieselben Werkzeuge

Mensch und KI benutzen dieselben kontrollierten Studio-Werkzeuge.

Die KI bearbeitet Inhalte nicht durch unkontrollierte direkte Änderungen irgendwo im Projekt, sondern über definierte **Studio-Operationen**.

Andora Studio ist ein **Entwicklungswerkzeug**.

Es muss **nicht** auf der Raspberry-Pi-4-Referenzhardware des Spielclients laufen.

> **Bereits entschieden:** Andora Studio ist ein separater Entwicklungsbereich, kein Teil des Spielclients.

---

## 2. Architekturrolle

Andora Studio gehört zur **Content-/Entwicklungsseite** von Andora.

### Grundsätzliche Trennung

```text
Andora Studio
    ↓
validierte Andora-Daten
    ↓
Rust Realm / MariaDB / Godot Client
```

### Bestehende Autoritätsregel bleibt unverändert

* **Rust/Realm** ist für den Spielzustand autoritativ.
* **MariaDB** speichert persistente/autoritative Daten entsprechend der bestehenden Andora-Architektur (`Datenbank_Architektur.md`).
* **Godot** präsentiert den Spielzustand.
* **Lua** beschreibt/reagiert innerhalb seiner dokumentierten Grenzen (`Lua-Scripting-System.md`).
* Die **Studio-KI** erhält keine Spielserver-Autorität.

Das Studio darf diese Trennung **nicht umgehen**.

> **Bereits entschieden:** Das Studio liegt zwischen Entwickler-Werkzeug und validierten Daten; es greift niemals direkt in Realm, Datenbank oder laufende Spielzustände ein.

---

## 3. Referenz: Atavism 2.6.1

Als Architektur-/Editorreferenz wurden vorhandene **Atavism-2.6.1-Dateien** untersucht.

Dabei wurden unter anderem getrennte Editor-/Datenbereiche für folgende MMO-Inhalte gefunden:

* Items
* Quests
* Dialoge
* Loot Tables
* Skills
* Crafting Recipes
* Merchant Tables
* Mob Spawn Data

### Beispiele aus der untersuchten Unity-Editorstruktur

```text
ItemsPlugin.cs
ServerQuests.cs
ServerDialogues.cs
ServerLootTables.cs
ServerSkills.cs
ServerCraftingRecipes.cs
ServerMerchantTables.cs
ServerMobSpawnData.cs
```

Dazu existieren entsprechende Datenstrukturen wie beispielsweise:

```text
ItemData
QuestsData
DialogueData
LootTable
SkillsData
CraftingRecipeData
MobSpawnData
```

### Geltungsbereich der Referenz

Diese Untersuchung dient **ausschließlich als Referenz** dafür, welche Probleme ein MMO-Content-Editor lösen muss.

Andora übernimmt **NICHT automatisch**:

* Atavism-Code
* Atavism-Datenformate
* Atavism-Serverarchitektur
* Unity-Abhängigkeiten
* Atavism-Gameplayregeln

Andora Studio wird **passend zur bestehenden Andora-Architektur** eigenständig entwickelt (vgl. `Content-Studio.md` §11: historische Systeme sind ausschließlich konzeptionelle Referenzen).

> **Bereits entschieden:** Atavism ist reine Inspirationsquelle für die Problemdefinition, keine Vorlage für Code, Daten oder Regeln.

---

## 4. Modulares Studio

Andora Studio soll **nicht** aus einem einzigen universellen Editor bestehen.

Vorgesehene spezialisierte Arbeitsbereiche sind langfristig beispielsweise:

* World Generator / World Editor
* NPC Editor
* Item Editor
* Equipment/Visual Editor
* Quest Editor
* Dialogue Editor
* Loot Editor
* Merchant Editor
* Crafting Editor
* Skill Editor
* Spawn Editor
* Asset Library / Asset Composer

Diese Liste ist eine geplante **Studio-Struktur** und kann später erweitert werden.

Nicht alle Module müssen in Version 1 implementiert werden.

> **Langfristiges Ziel:** mehrere spezialisierte Module unter einer gemeinsamen Studio-Oberfläche.

---

## 5. KI als fester Bestandteil des Studios

KI-Unterstützung ist **kein** nachträglich aufgesetztes Chatfenster, sondern **Bestandteil der Studio-Architektur**.

Jeder geeignete Studio-Arbeitsbereich kann einen **spezialisierten KI-Agenten** besitzen.

Die KI soll **Studio-Operationen** benutzen können, die auch der menschlichen Editoroberfläche zugrunde liegen.

### Beispiel

| Weg | Vorgehen |
|---|---|
| **Mensch** | Gebäude auswählen → Position wählen → platzieren |
| **KI** | `place_building(building_id, position, rotation, ...)` |

Beide Wege benutzen **dieselbe** darunterliegende Studio-Operation.

Dadurch gelten für Mensch und KI dieselben:

* Datenstrukturen
* Validierungen
* Platzierungsregeln
* Referenzregeln
* Undo-/Redo-Mechanismen

> **Bereits entschieden:** KI und Mensch arbeiten über dieselben kontrollierten Studio-Operationen – nicht über separate, KI-spezifische Sonderpfade.

---

## 6. Harte KI-Bereichstrennung

### Zentrale Architekturregel

Die KI erhält **ausschließlich Zugriff auf die Werkzeuge des aktuell aktiven Studio-Arbeitsbereiches**.

Diese Einschränkung darf **NICHT nur durch einen Prompt** erfolgen.

Nicht erlaubte Operationen sollen dem jeweiligen KI-Agenten **technisch nicht zur Verfügung stehen** (d. h. über die modul-übergreifende Operations-/Rechtevergabe, nicht allein über sprachliche Anweisung).

### Beispiel: WORLD GENERATOR

**Darf** beispielsweise:

* Terrain erzeugen/bearbeiten
* Wege und Straßen erzeugen
* Vegetation platzieren
* Gebäude platzieren
* Props platzieren
* Ressourcenpunkte platzieren
* vorhandene NPCs in der Welt platzieren
* Spawnpunkte platzieren
* vorhandene World-Assets verwenden

**Darf NICHT:**

* Quests schreiben
* Items verändern
* Skills verändern
* NPC-Definitionen eigenmächtig umschreiben
* Craftingregeln verändern

Wenn die World-KI erkennt, dass an einer Stelle beispielsweise eine Quest sinnvoll wäre, darf sie dies als **Vorschlag/Marker** hinterlassen.

Sie darf die Quest jedoch **nicht selbst erstellen**.

Die tatsächliche Quest-Erstellung erfolgt erst im **Quest Editor** mit dessen KI-Rechten.

> **Bereits entschieden:** Die Bereichstrennung ist eine technische Grenze, kein Vertrauens-/Prompt-Konzept.

---

## 7. Beispiel weiterer Bereiche

### NPC EDITOR

KI darf innerhalb der später festgelegten NPC-Regeln beispielsweise:

* NPC definieren
* Aussehen zusammenstellen
* vorhandene Ausrüstung zuordnen
* NPC-Eigenschaften bearbeiten

Sie darf **nicht automatisch**:

* Weltgebiete umbauen
* Quests schreiben
* Items neu balancieren

### QUEST EDITOR

KI darf beispielsweise:

* Quests erstellen
* Questschritte strukturieren
* vorhandene NPCs referenzieren
* vorhandene Orte referenzieren
* Ziele und Belohnungen innerhalb der gültigen Andora-Regeln definieren

Sie darf **nicht automatisch**:

* Weltgeometrie verändern
* NPC-Aussehen verändern
* neue Itemregeln erfinden

### ITEM EDITOR

KI darf beispielsweise:

* Items innerhalb des dokumentierten Itemsystems erstellen
* zulässige Eigenschaften zusammenstellen
* Visual-Komponenten zuordnen
* bestehende Material-/Qualitätsregeln verwenden

Sie darf **nicht automatisch**:

* Gebiete verändern
* Quests verändern
* NPCs verändern

> **Noch offen:** Die exakten Rechte jedes Moduls werden dokumentiert, **bevor** dessen KI-Funktionen implementiert werden.

Grundlegendes zur Wiederverwendung vorhandenen Contents, zu READ/COMPOSE/CREATE und zu einem lokalen Content-Katalog: siehe `Content-Studio.md` §3–§5.

---

## 8. KI-Betriebsarten

Langfristig können innerhalb eines Studio-Moduls unterschiedliche Arbeitsweisen unterstützt werden:

### ASSISTIEREN

Der Mensch arbeitet hauptsächlich selbst. Die KI erledigt einen begrenzten Auftrag.

> Beispiel: „Setze entlang dieser Straße passende Laternen."

### GENERIEREN

Die KI erzeugt innerhalb des aktuellen Moduls einen größeren Entwurf.

> Beispiel im World Generator: „Erstelle auf dieser Fläche ein kleines Luzilla-Bergdorf."

### AGENT

Die KI darf mehrere freigegebene Werkzeuge **desselben** Studio-Moduls nacheinander benutzen, ihr Ergebnis validieren und innerhalb dieses Bereichs korrigieren.

Auch im Agent-Modus gelten die **Modulgrenzen unverändert**.

Agent bedeutet **NICHT** Studio-weite Vollberechtigung.

> **Langfristiges Ziel:** drei Betriebsarten; im Agent-Modus bleibt die harte Bereichstrennung (siehe §6) wirksam.

---

## 9. World-Generator-Beispiel

Ein Auftrag könnte lauten:

> „Erstelle im Menschengebiet ein kleines Walddorf für Spieler Level 8–12. Etwa 15 Häuser, Schmied, Händler und Gasthaus. Durch das Dorf führt die Straße zur Hauptstadt. Westlich liegt ein Holzfällerlager."

Die World-KI kann daraus beispielsweise:

* Gelände strukturieren
* Straßen/Wege anlegen
* vorhandene passende Gebäude auswählen
* Gebäude platzieren
* Vegetation verteilen
* vorhandene NPCs platzieren
* Spawn-/Ressourcenpunkte setzen

Sie darf **NICHT** aus eigener Initiative die dazugehörigen Quests schreiben.

Questideen dürfen lediglich als **spätere Vorschläge/Marker** dokumentiert werden.

> **Bereits entschieden:** World-KI plant und platziert Weltinhalte; Vorschläge zu Quests bleiben Marker und werden erst im Quest Editor zu Aufgaben.

---

## 10. Asset-Bibliothek

Die KI soll bevorzugt mit einer **kontrollierten Andora-Assetbibliothek** arbeiten.

Beispielhafte Kategorien:

```text
terrain/
roads/
buildings/
vegetation/
props/
characters/
equipment/
resources/
effects/
```

Assets benötigen **stabile IDs**.

Die KI soll vorhandene Assets über IDs/Metadaten auswählen können, anstatt Dateipfade oder beliebige Dateien direkt zu manipulieren.

Beispiel:

```text
human_house_03
luzilla_forge_02
oak_tree_04
```

> **Noch offen:** Die endgültige Struktur (Dateiformat, Metadaten, ID-Vergabe) wird später separat spezifiziert.

---

## 11. Asset- und Equipment-Composer

Andora Studio soll langfristig auch **modulare visuelle Inhalte** zusammensetzen können.

Beispiel Helm:

* Grundform
* Material
* Visier
* Verzierung
* Emblem
* Farb-/Materialvariante

Das passt zum geplanten **modularen Charakter-/Ausrüstungssystem**.

Item-Typ und visuelle Darstellung müssen **technisch trennbar** bleiben.

> **Noch offen:** Die endgültigen Datenformate werden später separat festgelegt.

---

## 12. Validierung

Jedes Studio-Modul soll **eigene fachliche Validatoren** besitzen.

### Beispiele World Generator

* Gebäude innerhalb gültiger Fläche?
* Kollisionen?
* Wege verbunden?
* Eingang erreichbar?
* Spawnpunkt in gültiger Position?
* Ressource auf geeignetem Terrain?

### Beispiele Quest Editor

* Referenzierte NPCs vorhanden?
* Referenzierte Orte vorhanden?
* Questzustände gültig?
* Voraussetzungen gültig?
* Belohnungen gemäß Andora-Regeln gültig?

KI darf Validatorergebnisse verwenden, um Fehler **innerhalb ihres erlaubten Bereiches** zu korrigieren.

Validatoren sind **deterministischer Studio-Code** und nicht bloß KI-Einschätzungen.

> **Bereits entschieden:** Validierung ist deterministischer Code; die KI darf Validatorergebnisse nur in ihrem freigegebenen Bereich nutzen.

---

## 13. Transaktionen / Undo

Größere KI-Aktionen sollen als **nachvollziehbare Studio-Operation bzw. Operationsgruppe** behandelt werden.

### Beispiel: AI Operation „Create Forest Village"

* Terrainänderungen
* Gebäude
* Vegetation
* NPC-Platzierungen
* Spawnpunkte

Der Benutzer soll später mindestens nachvollziehen können:

* was die KI geändert hat
* welche Objekte erzeugt wurden
* welche Objekte verändert wurden

Undo/Redo bzw. vergleichbare sichere Rücknahme soll bei der späteren Architektur berücksichtigt werden.

Eine KI-Aktion mit hunderten Änderungen darf **nicht** dazu führen, dass der Benutzer hunderte Objekte einzeln entfernen muss.

> **Langfristiges Ziel:** Operationsgruppen mit zentraler Undo-/Redo- bzw. Rollback-Fähigkeit; genaue Datenstruktur noch offen (siehe §17).

---

## 14. Planung vs. deterministische Ausführung

Die KI muss **nicht** jede Einzelplatzierung selbst berechnen.

### Bevorzugtes Prinzip

**KI:**

* versteht Auftrag
* erstellt Layout-/Contentplan
* wählt geeignete Studio-Werkzeuge und Assets

**Studio-Code:**

* Rasterberechnung
* Platzierungsregeln
* Abstände
* Kollisionsprüfung
* Pfad-/Straßenberechnung
* zulässige Zufallsvariation
* technische Validierung

Dadurch bleibt die eigentliche Weltkonstruktion **reproduzierbarer** und **ressourcenschonender**.

> **Bereits entschieden:** Die KI plant, der deterministische Studio-Code führt aus und validiert.

---

## 15. Lokale KI

Andora Studio soll grundsätzlich so entworfen werden, dass **lokale KI-Dienste** angebunden werden können.

Die konkrete Modell-/API-Auswahl wird **NICHT** in diesem Grunddokument festgeschrieben.

Die KI-Anbindung darf daher **nicht unnötig an einen einzelnen Anbieter oder ein einzelnes Modell gekoppelt** werden.

> **Bereits entschieden:** Providerunabhängige Anbindung (vgl. Provider-Prinzip im `Coordinator.md`); konkrete Modelle/Protokolle sind offen (siehe §17).

---

## 16. Sicherheits-/Autoritätsgrenze

Die Studio-KI erzeugt **Entwicklungsinhalte**.

Sie erhält dadurch **keine Autorität über einen laufenden Realm**.

Insbesondere darf sie **nicht** über Studio-Werkzeuge:

* Spielerzustände direkt verändern
* Servervalidierung umgehen
* MariaDB-Autorität umgehen
* Rust-Autorität umgehen
* beliebige Lua-Ausführung außerhalb dokumentierter Schnittstellen erzeugen

Bei späterer visueller Lua-/Event-Erstellung dürfen **ausschließlich dokumentierte und freigegebene Aktionen/Requests** erzeugt werden (vgl. `Lua-Scripting-System.md`, `Content-Studio.md` §6 „Kein produktiver DB-Zugriff").

> **Bereits entschieden:** Die Studio-KI bleibt auf Tool-Operationen beschränkt; Realm-, DB- und Lua-Autorität bleiben unangetastet.

---

## 17. Offene Punkte

Folgende Dinge werden in diesem Grunddokument **bewusst NICHT** festgelegt:

* endgültiges Studio-Dateiformat
* endgültiges DB-Schema
* genaue Godot-Plugin-Struktur
* konkrete KI-Modelle
* konkrete Ollama/API-Protokolle
* endgültige Asset-Metadaten
* endgültige Undo-Datenstruktur
* endgültiges World-Chunk-Format
* endgültiges visuelles Scripting
* konkrete Implementierungsreihenfolge aller Module

Diese Punkte werden **später separat spezifiziert**.

---

## 18. Beziehung zu bestehender Dokumentation

| System | Dokument | Bezug zum Andora Studio |
|---|---|---|
| Content-KI / Content-Bereiche | `Content-Studio.md` | Detailkonzept der Content-KI: Arbeitsbereiche, Content-Katalog, READ/COMPOSE/CREATE, Pipeline, kein produktiver DB-Zugriff – **nicht hier dupliziert** |
| Lua-Content-Schicht | `Lua-Scripting-System.md` | Grundlage dokumentierter Content-/Lua-Operationen; Studio-KI erzeugt nur freigegebene Aktionen/Requests |
| Architektur | `architecture.md`, `project_overview.md` | Einordnung als Entwicklungs-/Content-Seite; Rust-Realm autoritativ für Spielzustand |
| Persistenz | `Datenbank_Architektur.md`, `Projekt-Status.md` | MariaDB nur für persistente/autoritative Daten; Studio ist Konzept, nicht implementiert |
| Client-Darstellung | `Clientdarstellung_und_Performance.md`, `Mehrere_Offizielle_Clients.md` | Godot als Basis des Studios vs. Godot-Referenzclient; Darstellungstrennung und Studio-/Entwicklungsnutzung von 3D als Produktionswerkzeug |
| Provider-Anbindung | `Coordinator.md` | Providerunabhängiges Anbindungsprinzip als Orientierung für lokale KI-Dienste |