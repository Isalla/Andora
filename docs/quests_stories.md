# Andora – Quests und Geschichten

## Status

**Architektur / Planung**

Das Quest-System verbindet Spieler mit den Geschichten, NPCs und Ereignissen der persistenten Welt von Andora.

Quests werden nicht als isolierte Aufgaben betrachtet, sondern können mit NPC-Wissen, Beziehungen, World Events und dem Dynamic Scene System verbunden sein.

Die verbindlichen Grundprinzipien für Storytelling-Ebenen, Weltgeheimnisse, Bücher als Gameplay, verborgene Questketten, variable persönliche Rätsel, spielerausgelöste Realm-Ereignisse und Realm-Chroniken stehen in `Storytelling_und_Weltgeheimnisse.md`.
Das Erfolgssystem und Titel sind in `Erfolge_und_Titel.md` definiert.

---

# 1. Grundprinzip

Quests besitzen eine klare Trennung zwischen:

```text
Lua
→ Questdefinition und Inhalt

Realm-Server (Rust)
→ Questmechanik und Validierung

MariaDB
→ persistenter Questfortschritt

Godot
→ Darstellung und Spielerinteraktion

Runtime-KI (lokal: Ollama)
→ optionale Dialoge und Interpretation
```

Der Server ist jederzeit die Autorität über den tatsächlichen Questzustand.

---

# 2. Hauptgeschichte

Andora kann eine übergeordnete Hauptgeschichte besitzen.

Grundstruktur:

```text
Einführung in die Welt
         ↓
Entdeckung der Regionen und Bewohner
         ↓
zentrale Handlung entwickelt sich
         ↓
größere Konflikte und World Events
         ↓
Endgame-Inhalte
```

Die Hauptgeschichte muss nicht bedeuten, dass jeder Spieler exakt denselben Weg nimmt.

Neben der zentralen Handlung können regionale Geschichten, NPC-Geschichten und persönliche Questketten existieren.

> Die Ausgestaltung der Hauptstory über mehrere Veröffentlichungen hinweg (Geheimnisse, die zunächst nicht beantwortet werden und später gelöst oder durch größere Geheimnisse ersetzt werden können) ist verbindlich in `Storytelling_und_Weltgeheimnisse.md` (§ 4) definiert.

---

# 3. Questdefinitionen

Die Inhalte einer Quest werden in Lua definiert.

Eine Quest kann beispielsweise enthalten:

```lua
quest = {
    id = "borin_iron_ore",

    title_key = "quest.borin_iron_ore.title",
    description_key = "quest.borin_iron_ore.description",

    requirements = {
        level = 5
    },

    objectives = {
        {
            type = "collect",
            target = "iron_ore",
            required = 20
        }
    },

    rewards = {
        xp = 500,
        gold = 100
    }
}
```

Die Werte sind lediglich ein Beispiel für die Struktur.

Lua beschreibt die Quest.

Der Realm-Server (Rust) entscheidet, ob die Bedingungen tatsächlich erfüllt wurden.

---

# 4. Strukturierte Questziele

Questziele werden nicht ausschließlich als Text gespeichert.

Sie besitzen strukturierte Objective-Typen.

Geplante Grundtypen:

```text
kill
collect
talk
visit
escort
deliver
discover
craft
event
```

Dadurch kann der Server Questfortschritt zuverlässig prüfen.

> **Hinweis:** Die vollständige Zieltypen-Liste des langfristigen Quest-Systems steht in `Quest-System.md` (Abschnitt 5). Der verbindliche Quest-V1-Umfang (`kill`, `talk`, `collect`, `deliver`) ist in `Quest-System.md`, Abschnitt 27, festgelegt. Die hier genannten Grundtypen beschreiben das langfristige Gesamtsystem.

---

# 5. Kill

Beispiel:

```text
Töte 5 Schleime.
```

Strukturell:

```text
type = kill
target = slime
required = 5
```

Der Server erhöht den Fortschritt nur bei einem gültigen Kill-Ereignis.

---

# 6. Collect

Beispiel:

```text
Sammle 20 Eisenerz.
```

Strukturell:

```text
type = collect
target = iron_ore
required = 20
```

Das Quest-System verwendet dabei das serverseitige Item-/Inventory-System.

---

# 7. Talk

Beispiel:

```text
Sprich mit Borin.
```

Der Server prüft:

* richtigen NPC
* richtigen Spieler
* Questzustand
* erforderliche Bedingungen

Ein Gespräch allein bedeutet nicht automatisch, dass ein Questziel abgeschlossen wird.

Die Präsentation von Gesprächen/Portraits (VN-Darstellung, Expressions, Antwortmöglichkeiten)
ist in `Visual_Novel_Dialog_und_Szenendarstellung.md` beschrieben; sie ändert nichts an den
hier geltenden Questregeln.

---

# 8. Visit und Discover

Quests können das Erreichen oder Entdecken bestimmter Orte verlangen.

Beispiel:

```text
visit
→ erreiche den alten Tempel

discover
→ entdecke den verborgenen Eingang
```

Gebiets- und Positionsprüfung erfolgt serverseitig.

---

# 9. Escort

NPCs können Bestandteil von Escort-Quests sein.

Dabei bleibt der NPC eine echte persistente Person der Welt.

Er wird nicht automatisch für die Quest dupliziert.

Das Quest-System muss deshalb mit:

* NPC-Zustand
* Position
* Reise
* Verfügbarkeit
* Tod
* Scene Locks

zusammenarbeiten können.

---

# 10. Deliver

Lieferquests verbinden Quest-, Inventory- und NPC-System.

Beispiel:

```text
Bringe Borin 20 Eisenerz.
```

Ablauf:

```text
Spieler spricht mit Borin
        ↓
QuestService prüft Quest
        ↓
InventoryService prüft 20 Eisenerz
        ↓
Spieler bestätigt Übergabe
        ↓
Server entfernt Gegenstände
        ↓
Questfortschritt
        ↓
QuestCompleted
```

Die KI darf diesen Vorgang nicht selbst durchführen.

---

# 11. Craft

Crafting kann als Questziel verwendet werden.

Beispiel:

```text
Stelle ein bestimmtes Werkzeug her.
```

Das Crafting-System meldet einen erfolgreichen serverseitig validierten Herstellungsprozess.

Das Quest-System reagiert auf dieses Event.

---

# 12. Event

Quests können mit World Events verbunden sein.

Beispiele:

```text
Nimm an der Verteidigung eines Dorfes teil.

Hilf bei einem regionalen Ereignis.

Erlebe den Abschluss eines World Events.
```

Das Quest-System kann auf entsprechende Events reagieren.

Vorbereitete, durch Spieler-Entdeckungen oder -Handlungen ausgelöste Realm-Ereignisse und die daraus entstehenden Realm-Chroniken sind in `Storytelling_und_Weltgeheimnisse.md` (§ 10–11) verbindlich definiert. Quests können auf solche Realm-Ereignisse reagieren, ohne deren Zustandsmodell zu hinterfragen.

---

# 13. Questfortschritt

Der Questfortschritt wird serverseitig verwaltet.

Beispiel:

```text
Gameplay Event
      ↓
Event System
      ↓
QuestService
      ↓
passt Event zu aktivem Objective?
      ↓
Fortschritt aktualisieren
      ↓
MariaDB
```

Der Client darf Questfortschritt niemals selbst festlegen.

---

# 14. Voraussetzungen

Quests können Voraussetzungen besitzen.

Beispiele:

```text
Level
vorherige Quest
Story Flag
NPC-Beziehung
World State
Gebiet
Fraktion
bestimmtes Ereignis
```

Die Voraussetzungen werden vom Server geprüft.

---

# 15. Questabschluss

Eine Quest wird ausschließlich durch den Server abgeschlossen.

```text
letztes Objective erfüllt
        ↓
QuestService validiert
        ↓
QuestCompleted
        ↓
Belohnungen
        ↓
Persistenz
        ↓
weitere Systeme reagieren
```

`QuestCompleted` kann anschließend beispielsweise:

* eine weitere Quest freischalten
* NPC-Wissen verändern
* Beziehungen beeinflussen
* eine Dynamic Scene auslösen
* Story Flags verändern
* World Events beeinflussen

---

# 16. Belohnungen

Mögliche Questbelohnungen:

* Erfahrung
* Gold
* Items
* Reputation
* Beziehung
* Story-Fortschritt
* Zugang zu neuen Inhalten
* besondere Dienstleistungen

Die KI vergibt keine Belohnungen.

Die tatsächliche Vergabe erfolgt durch serverseitige Services.

---

# 17. NPC-Integration

Quests können eng mit NPCs verbunden sein.

NPCs können:

* Quests anbieten
* Informationen geben
* Questziele darstellen
* Gegenstände entgegennehmen
* auf Questfortschritt reagieren
* nach Abschluss anders mit dem Spieler umgehen

Dabei gelten weiterhin die normalen NPC-Regeln.

> **Ein NPC darf nur auf Informationen reagieren, die er tatsächlich besitzt.**

Ein NPC weiß deshalb nicht automatisch, dass der Spieler irgendwo weit entfernt eine Quest abgeschlossen hat.

---

# 18. Informationsübertragung

Questinformationen können Teil des normalen Informationssystems der Welt sein.

Beispiel:

```text
Spieler rettet Dorf
      ↓
World Event / Quest abgeschlossen
      ↓
Bewohner haben es beobachtet
      ↓
Information verbreitet sich
      ↓
andere NPCs erfahren später davon
```

Dadurch muss eine Tat nicht automatisch sofort jedem NPC der Welt bekannt sein.

---

# 19. Beziehungen

Quests können Beziehungen zu NPCs beeinflussen.

Beispielsweise:

```text
Spieler hilft Borin
      ↓
Borin-Beziehung steigt

Spieler betrügt Borin
      ↓
Borin-Beziehung sinkt
```

Diese Beziehung kann spätere:

* Dialoge
* Dienstleistungen
* Preise
* Informationen
* Crafting-Aufträge
* Questmöglichkeiten

beeinflussen.

---

# 20. KI-Integration

Die Runtime-KI kann Quests erzählerisch unterstützen.

Beispiele:

* natürliche NPC-Dialoge
* Questhinweise
* Reaktionen auf Fortschritt
* persönliche Antworten
* Interpretation natürlicher Spielerfragen

Die KI kontrolliert jedoch niemals den Questfortschritt.

```text
Spielerfrage
      ↓
NPC / Quest Context
      ↓
AI
      ↓
Dialog

Questzustand
      ↓
bleibt vollständig serverseitig
```

---

# 21. Questhinweise

NPCs können Hinweise geben.

Dabei gelten ihre tatsächlichen Kenntnisse.

Ein NPC darf keine Lösung kennen, nur weil die Information im Questskript existiert.

Beispiel:

```text
Quest:
Finde den Eingang zur Mine.

NPC A:
kennt Mine
→ kann helfen

NPC B:
hat davon gehört
→ ungenauer Hinweis

NPC C:
kennt Mine nicht
→ kann nicht helfen
```

Dadurch bleiben NPC-Wissen und Quest-System miteinander konsistent.

---

# 22. Dynamic Scene Integration

Quests können Szenen auslösen.

Mögliche Events:

```text
QuestStarted
QuestProgressChanged
QuestCompleted
QuestFailed
```

Die Scene Engine reagiert auf diese Events.

Das Quest-System startet keine konkrete Cutscene direkt.

Beispiel:

```text
QuestCompleted
      ↓
Event System
      ↓
Scene Engine
      ↓
passende Lua-Szene vorhanden?
      ↓
Scene starten
```

---

# 23. i18n

Questtexte werden nicht direkt in einer bestimmten Sprache in die Questdefinition geschrieben.

Stattdessen werden i18n-Schlüssel verwendet.

Beispiel:

```lua
title_key = "quest.borin_iron_ore.title"
description_key = "quest.borin_iron_ore.description"
```

Dadurch können dieselben Questdefinitionen für alle unterstützten Sprachen verwendet werden.

---

# 24. Speicherung

MariaDB speichert den persistenten Questzustand.

Beispiele:

```text
character_id
quest_id
state
objective_progress
started_at
completed_at
relevant_choices
```

Die eigentlichen statischen Questdefinitionen müssen nicht für jeden Spieler erneut in der Datenbank gespeichert werden.

---

# 25. Questzustände

Ein Quest-State kann beispielsweise folgende Zustände besitzen:

```text
AVAILABLE
ACTIVE
COMPLETED
FAILED
```

Bei komplexeren Questketten können zusätzliche Zustände später ergänzt werden.

> **Hinweis:** `Quest-System.md` ist der Architektur-Anker und definiert **fünf** Zustände (HIDDEN, AVAILABLE, ACTIVE, COMPLETED, FAILED). Die hier vereinfachte Liste ist ein Teil davon. Die verbindlichen Quest-V1-Regeln zu Ableitung und Persistenz der Zustände stehen in `Quest-System.md`, Abschnitt 27.5.

---

# 26. Keine Questlogik im Client

Godot ist für Darstellung und Interaktion zuständig.

Der Client darf:

* Questtexte anzeigen
* Ziele anzeigen
* Fortschritt anzeigen
* Questmarker darstellen
* Spieleraktionen senden

Der Client darf nicht:

* Questfortschritt bestimmen
* Questbedingungen umgehen
* Belohnungen erzeugen
* Quests selbst abschließen

---

# 27. Trennung von anderen Systemen

Crafting und Hausbau besitzen eigene Systemdokumentationen.

Sie werden deshalb nicht als Teil des Quest-Systems definiert.

Quests können diese Systeme jedoch verwenden.

Beispiele:

```text
Quest
→ stelle Gegenstand her
→ CraftingService

Quest
→ errichte bestimmtes Objekt
→ BuildingService

Quest
→ bringe Gegenstand
→ InventoryService

Quest
→ sprich mit NPC
→ NPC / Interaction System
```

Das Quest-System orchestriert Ziele, dupliziert aber nicht die Mechanik anderer Systeme.

---

# 28. Architekturregel

> **Lua beschreibt die Aufgabe. Der Realm-Server (Rust) prüft die Aufgabe. MariaDB merkt sich den Fortschritt. Godot zeigt ihn dem Spieler.**

Die Runtime-KI kann die Geschichte und Gespräche lebendiger machen, besitzt aber keine Autorität über Questzustände.

---

# 29. Ziel

Quests in Andora sollen nicht nur aus:

> „Gehe zu X, töte Y und kehre zurück.“

bestehen.

Sie sollen mit der persistenten Welt verbunden werden können.

NPCs, Beziehungen, Informationen, Reisen, World Events und Dynamic Scenes können gemeinsam dafür sorgen, dass eine Quest als Teil der Welt wahrgenommen wird und nicht als isolierter Eintrag im Questlog.

Die vier Ebenen des Storytellings (Hauptstory, Nebenmissionen, Rätsel und Weltgeheimnisse, spielerausgelöste Realm-Ereignisse) sowie Bücher, verborgene Questketten und variable persönliche Rätsel sind in `Storytelling_und_Weltgeheimnisse.md` verbindlich definiert.
