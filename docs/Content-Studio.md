# Andora – Content Studio / Content-KI

## Status

**Konzeptdokument – NICHT implementiert.**

Dieses Dokument beschreibt ein langfristig geplantes **Content Studio**: eine lokale Arbeitsumgebung, mit der Menschen zusammen mit einer Content-KI Inhalte für Andora erstellen und bearbeiten können.

Es gelten klare Kennzeichnungen:

* **Bereits entschieden:** was dieses Dokument verbindlich als Ziel festhält.
* **Langfristiges Ziel:** was angestrebt, aber noch nicht umgesetzt ist.
* **Noch offen:** was bewusst erst bei der späteren Entwicklung entschieden wird.
* **Nicht implementiert:** das Content Studio existiert heute nicht und darf nicht den Eindruck erwecken, es gebe es bereits.

Es wird **kein Code** beschrieben, der bereits existiert.

---

## 1. Grundidee

Das Content Studio ist eine lokale Arbeitsumgebung für die Erstellung und Bearbeitung von Andora-Content.

Es soll später sowohl vom Hauptentwickler als auch von anderen Content-Designern verwendet werden können.

Die Content-KI benötigt **keinen Zugriff auf die produktive MariaDB** und bekommt **keine Datenbank-Zugangsdaten**.

Sie arbeitet stattdessen mit kontrollierten, versionierbaren:

* Content-Definitionen
* Dokumentationen
* Content-Katalogen
* freigegebenen Content-/Lua-API

Die produktive Datenbank bleibt für persistenten Runtime-/Spielerzustand zuständig und ist **keine Arbeitsoberfläche** für die Content-KI.

---

## 2. Arbeitsbereiche

Das Content Studio wird in getrennte **Arbeitsbereiche** gegliedert.

Beispiele:

* Welt
* NPCs
* Quests
* Items
* Fähigkeiten
* Cutscenes
* Dialoge
* Regionen
* Events
* Übersetzung / i18n

Diese Liste ist erweiterbar und noch keine endgültige UI-Struktur.

### Entscheidende Regel

Der aktuell gewählte Arbeitsbereich bestimmt:

* welche Informationen die KI erhält,
* welche Content-Kataloge sie durchsuchen darf,
* welche Werkzeuge/Funktionen verfügbar sind,
* welchen Content sie erstellen darf,
* welchen Content sie verändern darf.

Die KI soll **NICHT automatisch außerhalb des aktuellen Arbeitsbereichs** Content erzeugen oder verändern.

Beispiele:

* Im Arbeitsbereich „Welt" soll die KI nicht nebenbei neue Questketten erstellen.
* Im Arbeitsbereich „Quests" soll die KI nicht ungefragt das Gebiet mit neuen Monstern, Spawnpunkten oder Orten erweitern.

---

## 3. Vorhandenen Content bevorzugen

Ein zentrales Prinzip lautet:

> **Vorhandenen Content zuerst suchen und wiederverwenden.**

**Beispiel Quest-Erstellung:**

Die Quest-KI darf relevante vorhandene Informationen verwenden wie:

* NPCs
* Monster
* Spawnpunkte
* Items
* Orte
* Regionen
* Weltobjekte
* Fähigkeiten
* Interaktionsmöglichkeiten
* vorhandene Quest-/Storyinformationen
* freigegebene Lua-Funktionen

Aus diesen vorhandenen Bausteinen soll sie eine Quest oder Questreihe zusammensetzen.

Sie soll **nicht automatisch** neue Monster, Items, NPCs, Orte oder Spawns erfinden, nur weil dies für eine generierte Geschichte bequem wäre.

### Fehlende Abhängigkeiten

Wenn notwendiger Content fehlt, soll dies als **fehlende Abhängigkeit bzw. Vorschlag** gemeldet werden.

Beispiel:

> „Für diese Quest wäre ein bisher nicht vorhandener Questgegenstand nötig."

Danach kann bewusst entschieden werden, ob dieser Content erstellt oder die Quest an vorhandenen Content angepasst wird.

---

## 4. READ / COMPOSE / CREATE

Konzeptionelles Modell mit drei Arten von Aktionen:

### READ

* vorhandenen Content suchen
* vorhandenen Content verstehen
* Abhängigkeiten ermitteln

### COMPOSE

* vorhandene Content-Bausteine kombinieren
* zum Beispiel aus vorhandenen NPCs, Items, Spawns und Orten eine Quest erstellen

### CREATE

* tatsächlich neue Content-Objekte erzeugen

### Grundprinzip

> **COMPOSE FIRST. CREATE ONLY WHEN REQUIRED AND ALLOWED.**

CREATE darf nur innerhalb des dafür freigegebenen Arbeitsbereichs stattfinden.

Ein Quest-Arbeitsbereich darf beispielsweise **nicht selbstständig** einen neuen Monsterbestand für ein Gebiet erzeugen.

---

## 5. Content-Katalog

Die Content-KI soll langfristig vorhandenen Content über einen **lokalen, kontrollierten Content-Katalog** ermitteln können.

Beispiele für Kataloginhalte:

* Item-Definitionen
* NPC-Definitionen
* Monster
* Spawns
* Regionen
* Orte
* Fähigkeiten
* Questdefinitionen
* Weltobjekte

### Stabile Content-IDs

Dabei werden stabile Content-IDs verwendet.

Beispiele:

```text
iron_ore
water_flask
andorer_red_cloth
borin_letter_001
```

Die KI benötigt **keine konkreten Runtime-Instanzen** und **keine Spielerinventare**.

Beispielsweise muss sie bei Items wissen können:

* Content-ID
* Kategorie
* Qualitäts-/Raritätsinformationen soweit relevant
* Levelbereich
* Klassenbeschränkungen soweit relevant
* Stack-Eigenschaften
* Region/Herkunft soweit definiert
* i18n-Keys
* Quest-Relevanz soweit definiert

> **Die genaue technische Form dieses Katalogs ist NOCH NICHT entschieden.** Es wird nicht festgelegt, ob dies JSON, Lua, Indexdateien, eine lokale Datenbank, generierte Metadaten oder etwas anderes wird.

---

## 6. Kein produktiver DB-Zugriff

Die Content-KI soll vollständig lokal arbeiten können.

Sie erhält grundsätzlich **keinen direkten Zugriff auf die produktive MariaDB**.

Insbesondere kein:

* SQL-Zugriff
* Lesen von Spielerzuständen
* Schreiben von Spielerzuständen
* Zugriff auf Inventare realer Spieler
* Zugriff auf konkrete Item-Instanzen realer Spieler
* direkte Änderung von Runtime-Daten

Neue Inhalte entstehen zunächst als **kontrollierte Content-Dateien bzw. -Änderungen** und können geprüft, validiert und versioniert werden.

---

## 7. Content-Pipeline

Langfristiges Ziel – ungefähr folgender Ablauf:

```text
Designer
   ↓
Arbeitsbereich im Content Studio
   ↓
Content-KI
   ↓
vorhandenen Content suchen
   ↓
Content zusammensetzen / erlaubte Änderungen erzeugen
   ↓
Validator / Tests
   ↓
menschliches Review
   ↓
versionierter Andora-Content
   ↓
spätere Verwendung durch Realm/Client
```

> **KI-generierter Content geht nicht ungeprüft direkt auf einen Produktiv-Realm.**

---

## 8. Mehrere Content-Designer

Das System soll langfristig ermöglichen, dass auch andere Personen Content für Andora erstellen können, **ohne** Zugriff auf die produktive Datenbank oder die komplette Serveradministration.

Beispielsweise könnte ein **Questdesigner** Zugriff auf Folgendes erhalten:

* relevante Lore
* vorhandene Questinformationen
* NPC-Katalog
* Item-Katalog
* Spawn-/Monsterinformationen
* Orte und Regionen
* freigegebene Lua-/Content-API
* Validator
* lokale Content-KI

Er benötigt dafür **keine DB-Zugangsdaten**.

Arbeitsbereiche können langfristig auch als **Berechtigungs-/Scope-Grenzen** dienen.

> **Noch KEIN konkretes Benutzer-/Rechtesystem entwerfen.**

---

## 9. Content-KI vs. Runtime-NPC-KI

Die **Content-KI** ist von der **Runtime-NPC-KI** zu unterscheiden.

### Content-KI

* Entwicklungswerkzeug
* erzeugt bzw. bearbeitet Content
* arbeitet außerhalb des eigentlichen Spielbetriebs
* Ergebnisse werden geprüft und versioniert

### Runtime-NPC-KI

* läuft im Spielbetrieb
* steuert bzw. unterstützt dynamische NPC-Interaktionen entsprechend der bestehenden Andora-NPC-/KI-Architektur
* darf nicht mit der Content-KI verwechselt werden

Die bestehende NPC-/KI-Dokumentation wird hier **nicht neu definiert**, sondern referenziert (`Ki-NPC.md`, `Coordinator.md`, `ai_system.md`).

---

## 10. Lua als Grundlage

Das geplante allgemeine Lua-Content-Scripting-System ist eine wichtige technische Grundlage des Content Studios.

Die Content-KI soll langfristig **nur dokumentierte und freigegebene Lua-/Content-Funktionen** verwenden.

Sie darf **keine Lua-API erfinden**.

Lua wiederum darf die Autorität des Rust-Realm-Servers **nicht umgehen**.

Prinzip:

```text
Content-KI
→ Content/Lua
→ kontrollierte API
→ Rust validiert
→ autoritatives System führt aus
```

> Dieses Dokument definiert **nicht** die Lua-Architektur selbst. Dafür ist das separate Dokument `Lua-Scripting-System.md` zuständig.

---

## 11. Moderne Architektur

Falls historische MMO-/Lua-Systeme als Referenz erwähnt werden:

Sie sind **ausschließlich konzeptionelle Referenzen**.

Andora übernimmt geeignete Ideen, **nicht** deren alte technische Umsetzung.

Die spätere Umsetzung soll modernen Anforderungen entsprechen an:

* Sicherheit
* Wartbarkeit
* Testbarkeit
* Performance
* Serverautorität
* Versionsverwaltung
* KI-gestützte Content-Erstellung

---

## 12. Bewusst noch offen

Folgende Dinge werden **NICHT** entschieden oder erfunden:

* konkrete GUI
* Programmiersprache des Content Studios
* konkretes KI-Modell
* konkrete Model-Server-Technik
* Content-Katalog-Dateiformat
* lokale Datenbank ja/nein
* konkretes Plugin-System
* Benutzer-/Rechtesystem
* Deployment
* konkrete Validator-Implementierung
* Git-Workflow im Detail
* genaue Lua-Runtime
* endgültige API-Namen

Diese Punkte werden entschieden, wenn das System tatsächlich entwickelt wird.

---

## 13. Beziehung zu bestehender Dokumentation

| System | Dokument | Bezug zum Content Studio |
|---|---|---|
| Lua-Content-Schicht | `Lua-Scripting-System.md` | Grundlage des Content Studios; Content-KI nutzt nur freigegebene Content-/Lua-Funktionen |
| Quest-System | `Quest-System.md`, `quests_stories.md`, `Bericht_Questsystem.md` | Quest-Erstellung im Arbeitsbereich „Quests"; verfügbare Quest-Inhalte und -Zieltypen |
| NPC / Runtime-KI | `Ki-NPC.md`, `Coordinator.md`, `ai_system.md` | NPC-/KI-Inhalte; Abgrenzung Content-KI vs. Runtime-NPC-KI |
| Cutscenes / Scenes | `cutscene_system.md`, `ai_cutscene_system.md` | Cutscene-Orchestrierung als Content im Arbeitsbereich „Cutscenes" |
| Items / Inventory | `item_properties.md`, `inventory_system.md` | Item-Definitionen als Kataloginhalte; keine Spieler-Instanzen |
| Klassen / Fähigkeiten | `Klassensystem.md`, `Ability-System.md`, `Kampfsystem.md` | Klassen-/Ability-Content; Fähigkeiten-Hierarchie; Combat-Regeln bleiben Realm-Aufgabe |
| Architektur | `architecture.md`, `project_overview.md` | Content-/Scripting-Schicht innerhalb der lokalen, versionierbaren Content-Organisation; keine produktive MariaDB als Arbeitsoberfläche |
| Persistenz | `Datenbank_Architektur.md`, `Projekt-Status.md` | Produktive DB nur für Runtime-/Spielerzustand; Content Studio ist Konzept, nicht implementiert |
| Lokalisierung | `i18n/README.md` | Spieler sichtbare Texte als i18n-Keys; Arbeitsbereich „Übersetzung / i18n" |