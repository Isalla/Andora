# Andora – Quest-System

## 1. Status

**Status:** 🟡 Konzept

Das Quest-System von Andora soll klassische Aufgaben ebenso unterstützen wie dynamische, klassenabhängige, gruppenbasierte und durch die Welt ausgelöste Quests.

Quests sollen nicht nur Aufgabenlisten sein, sondern auf Charakter, Klasse, Beziehungen und Ereignisse der Welt reagieren können.

---

# 2. Grundarchitektur

Das Quest-System wird klar zwischen statischem Inhalt, Ausführung, Speicherung und Darstellung getrennt.

## Lua – statischer Teil

Lua beschreibt, **was eine Quest ist**.

Dazu gehören beispielsweise:

* Quest-ID
* Titel
* Beschreibung
* Questabschnitte
* Ziele
* Voraussetzungen
* Klassenbedingungen
* Beziehungen
* benötigte vorherige Quests
* World Events
* Regions-Events
* Story-Flags
* Belohnungen
* Folgequests
* Trigger
* i18n-Keys
* klassenspezifische Ziele

Lua verändert jedoch nicht direkt den persistenten Spielzustand.

---

## TypeScript – ausführender Teil

TypeScript ist die Autorität des Quest-Systems.

Der Server:

* prüft Voraussetzungen
* startet Quests
* verarbeitet Quest-Events
* aktualisiert Fortschritt
* prüft Questziele
* wechselt Questabschnitte
* entscheidet über Abschluss oder Fehlschlag
* vergibt Belohnungen
* setzt Story- und Weltzustände
* prüft Klassenbedingungen
* prüft Beziehungen
* verarbeitet Gruppenfortschritt
* validiert alle durch Lua beschriebenen Aktionen

Lua beschreibt eine Bedingung.

**TypeScript entscheidet, ob sie tatsächlich erfüllt ist.**

---

## MariaDB – dynamischer Zustand

MariaDB beschreibt nicht, was eine Quest ist.

Sie speichert, **wo sich ein Charakter innerhalb einer Quest befindet**.

Beispielsweise:

```text
character_id
quest_id
quest_status
current_stage
objective_progress
started_at
completed_at
```

Bei einer Quest mit mehreren Abschnitten wird gespeichert, in welchem Abschnitt sich der Spieler befindet.

`objective_progress` kann mehrere Fortschrittswerte enthalten.

Beispiel:

```json
{
  "wolves": 4,
  "soldiers_healed": 7,
  "tower_destroyed": true
}
```

Die statische Questdefinition bleibt weiterhin in Lua.

---

## Godot – Client und Darstellung

Godot entscheidet nicht über Questfortschritt oder Questabschluss.

Der Client erhält die gültigen Informationen vom Server und stellt sie für den Spieler übersichtlich dar.

Dazu können gehören:

* Questlog
* Questtitel
* Beschreibung
* aktuelle Ziele
* Questabschnitte
* Fortschrittsanzeigen
* verfügbare Quests
* Kartenhinweise
* Questmarker
* Belohnungen
* abgeschlossene Quests

---

# 3. Grundregel

> **Lua beschreibt die Aufgabe. TypeScript prüft die Aufgabe. MariaDB merkt sich den Fortschritt. Godot zeigt ihn dem Spieler.**

---

# 4. Questzustände

Eine Quest kann beispielsweise folgende Zustände besitzen:

```text
HIDDEN
AVAILABLE
ACTIVE
COMPLETED
FAILED
```

**HIDDEN**

Die Quest existiert, wird dem Spieler aber aufgrund fehlender Voraussetzungen noch nicht angezeigt.

**AVAILABLE**

Alle notwendigen Voraussetzungen sind erfüllt und die Quest kann angenommen werden.

**ACTIVE**

Der Spieler hat die Quest angenommen.

**COMPLETED**

Die Quest wurde erfolgreich abgeschlossen.

**FAILED**

Die Quest ist fehlgeschlagen, sofern dieser Zustand für die jeweilige Quest vorgesehen ist.

---

# 5. Questziele

Questziele werden strukturiert definiert und nicht ausschließlich als Text gespeichert.

Mögliche Zieltypen:

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
heal
protect
taunt
destroy
interact
```

Weitere Zieltypen können später ergänzt werden.

Beispiel:

```text
Questtext:
"Bring Borins Brief zu Elara."

Technisches Ziel:
type       = deliver
item       = borin_letter_001
target_npc = elara
```

Dadurch kann TypeScript eindeutig feststellen, ob das tatsächliche Ziel erfüllt wurde.

---

# 6. Eventbasierter Questfortschritt

Questfortschritt soll möglichst über das zentrale Eventsystem erfolgen.

Beispiel:

```text
MonsterKilled
      ↓
QuestService
      ↓
aktive relevante Quests suchen
      ↓
Bedingungen prüfen
      ↓
Fortschritt aktualisieren
      ↓
Questziel erfüllt?
      ↓
Abschnitt / Quest fortsetzen
```

Weitere mögliche Ereignisse:

```text
NPCInteraction
ItemCollected
ItemDelivered
AreaEntered
ItemCrafted
NPCHealed
EnemyTaunted
ObjectDestroyed
WorldEventStarted
RegionEventStarted
QuestCompleted
```

Das Quest-System soll dadurch nicht für jede Spielmechanik eine eigene parallele Logik benötigen.

---

# 7. Dynamische Questverfügbarkeit

Nicht jede Quest ist jederzeit für jeden Spieler sichtbar.

Lua kann Bedingungen definieren, unter denen eine Quest verfügbar sein kann.

TypeScript prüft anschließend den tatsächlichen Spieler- und Weltzustand.

Mögliche Voraussetzungen:

* Level
* Klasse
* Rolle
* vorherige Quest
* Questabschnitt
* Story-Flag
* Beziehung zu einem NPC
* Ruf
* Fraktion
* Region
* World Event
* Regions-Event
* Weltzustand
* besondere Entscheidung
* bestimmtes Ereignis

Grundregel:

> **Lua definiert, wann eine Quest verfügbar sein kann. TypeScript entscheidet anhand des aktuellen Welt- und Spielerzustands, ob sie tatsächlich verfügbar ist.**

---

# 8. Beziehungen und Quests

NPC-Beziehungen können Quests freischalten oder verhindern.

Beispiel:

Ein NPC bietet einem unbekannten Spieler lediglich normale Aufgaben an.

Bei höherer Beziehung kann eine persönliche Questreihe verfügbar werden.

Dadurch können Quests beispielsweise Bedingungen besitzen wie:

```text
NPC = borin
Beziehung >= 60
```

Die Beziehung selbst wird nicht von Lua erfunden.

TypeScript liest den tatsächlichen Beziehungszustand des Charakters.

---

# 9. World- und Regions-Events

Quests können an Ereignisse der Welt gekoppelt sein.

Beispiele:

```text
World Event:
Invasion beginnt

Region Event:
Dorf wird angegriffen

Quest:
Verteidige das Dorf
```

Endet das Ereignis, kann die dazugehörige Quest ebenfalls nicht mehr verfügbar sein oder entsprechend reagieren.

Damit können Quests Bestandteil einer lebenden Welt werden.

---

# 10. Klassenabhängige Quests

Die Klasse eines Charakters kann bestimmen, welche Quest oder welche Variante einer Quest verfügbar wird.

Dasselbe Ereignis kann dadurch für unterschiedliche Klassen verschiedene Aufgaben erzeugen.

Beispiel:

## Angriff auf ein Feldlager

### Tank

```text
Verteidige die Soldaten.
Provoziere 10 Angreifer.
```

### Heiler

```text
Versorge die Verwundeten.
Heile 10 Soldaten.
```

### DD

```text
Schlage den Angriff zurück.
Besiege 10 Gegner.
```

### Magier

Der Magier kann beispielsweise:

```text
Verbündete mit Mana-Regeneration unterstützen
```

oder eine besondere Aufgabe erhalten:

```text
Zerstöre 3 magisch geschützte Türme.
```

Diese Türme können beispielsweise nur durch magischen Schaden verwundbar sein.

---

# 11. Klassenmechaniken innerhalb von Quests

Klassenquests sollen nicht lediglich dieselbe Aufgabe mit anderem Text wiederholen.

Die Welt selbst kann auf unterschiedliche Klassenfähigkeiten reagieren.

Beispiele für einen Magier:

* magische Türme zerstören
* Schutzbarrieren brechen
* Runen deaktivieren
* Portale schließen
* magische Objekte beeinflussen
* Verbündete mit Mana unterstützen

Ein Tank kann dagegen:

* Gegner provozieren
* NPCs schützen
* Positionen halten
* Angriffe abfangen

Ein Heiler kann:

* Verwundete behandeln
* NPCs heilen
* Gruppenmitglieder versorgen

Dadurch bekommt jede Klasse Aufgaben, die zu ihren tatsächlichen Fähigkeiten passen.

Grundprinzip:

> **Die Klasse verändert nicht unbedingt das Weltereignis – sondern die Aufgabe, die der Charakter innerhalb dieses Ereignisses übernimmt.**

---

# 12. Gruppenquests mit individuellen Klassenzielen

Eine Gruppe kann gemeinsam dieselbe Quest erhalten, während jedes Gruppenmitglied ein zur eigenen Klasse beziehungsweise Rolle passendes Ziel bekommt.

Beispiel:

```text
Quest:
Verteidigung des Feldlagers

Tank:
10 Gegner provozieren

Heiler:
10 verwundete Soldaten heilen

DD:
10 Gegner besiegen

Magier:
3 magisch geschützte Türme zerstören
oder die Gruppe magisch unterstützen
```

Alle Charaktere erleben dieselbe Geschichte und dasselbe Ereignis.

Ihre Aufgaben unterscheiden sich jedoch.

---

# 13. Zusammenarbeit innerhalb einer Gruppenquest

Die einzelnen Klassenziele dürfen sich gegenseitig unterstützen.

Beispiel:

```text
Magier
  ↓
erhöht Mana-Regeneration

Heiler
  ↓
kann länger Spieler und NPCs heilen

Tank
  ↓
hält Gegner von den Verwundeten fern

DD
  ↓
beseitigt gebundene Gegner

Gruppe
  ↓
verteidigt gemeinsam das Feldlager
```

Dadurch erledigt nicht jeder Spieler isoliert seine eigene Checkliste.

Die unterschiedlichen Klassen arbeiten tatsächlich gemeinsam am selben Ziel.

---

# 14. Kein gegenseitiger Questfortschritts-Konflikt

Klassenspezifische Gruppenziele verhindern, dass Gruppenmitglieder sich unnötig gegenseitig behindern.

Beispiel:

Der Tank benötigt keine 10 Kills, wenn seine Aufgabe darin besteht, 10 Gegner zu provozieren.

Der Heiler konkurriert nicht um Gegner, weil seine Aufgabe darin besteht, Verwundete zu heilen.

Der DD kann Gegner töten, ohne dem Tank oder Heiler deren Questfortschritt wegzunehmen.

Dadurch muss eine gemischte Gruppe die gemeinsame Quest nur einmal durchspielen.

Grundprinzip:

> **Gruppenquests teilen ein gemeinsames Ereignis, aber der individuelle Questfortschritt richtet sich nach der Aufgabe des jeweiligen Charakters.**

---

# 15. Klassenspezifische Questreihen

Bestimmte Questreihen können ausschließlich für einzelne Klassen vorgesehen werden.

Dadurch können Klassen ihre eigene Geschichte und Entwicklung erhalten.

Solche Questreihen können beispielsweise:

* Klassenmechaniken erklären
* neue Fähigkeiten thematisieren
* besondere Klassenorte verwenden
* Klassen-NPCs einführen
* klassenspezifische Herausforderungen enthalten
* auf die Rolle des Charakters in der Welt eingehen

---

# 16. Tier-Klassenquests

Zu Beginn eines neuen Tiers kann für jede Klasse eine entsprechende Klassenquest verfügbar werden.

Diese Quest erfüllt zwei Aufgaben:

1. Sie führt den Charakter spielerisch in die Anforderungen des neuen Tiers ein.
2. Sie stellt eine grundlegende klassengerechte Ausrüstung bereit.

---

# 17. Tier-Basic-Ausrüstung

Die Belohnung einer Tier-Klassenquest besteht aus **Basics des jeweiligen Tiers**.

Beispiele:

Tank:

* grundlegende schwere Rüstung
* grundlegender Schild
* passende Basic-Waffe

Heiler:

* grundlegende Heilausrüstung
* passende Basic-Waffe oder Fokus

Magier:

* grundlegende magische Ausrüstung
* passender Stab oder Fokus

Andere Klassen erhalten entsprechend geeignete Ausrüstung.

Diese Gegenstände sollen nicht besonders wertvoll oder außergewöhnlich sein.

> **Tier-Klassenausrüstung = Basics des jeweiligen Tiers.**

---

# 18. Zweck der Basic-Ausrüstung

Die Basic-Ausrüstung verhindert, dass ein Charakter ein neues Tier mit völlig veralteter Ausrüstung beginnen muss.

Beispiel:

Ein Level-20-Tank soll nicht gezwungen sein, mit Level-10-Ausrüstung in den neuen Bereich zu gehen.

Die Klassenquest stellt deshalb eine vernünftige Ausgangsbasis bereit.

Bessere Ausrüstung kann weiterhin durch:

* normale Quests
* Crafting
* Loot
* Dungeons
* Auktionshaus
* Gruppen
* Gilden
* besondere Ereignisse

erworben werden.

---

# 19. Solo-Fortschritt

Die Basic-Ausrüstung soll keinen künstlichen Punkt erzeugen, an dem ein Solospieler ohne Gruppen- oder Gildenausrüstung nicht mehr weiterspielen kann.

Schlechtere Ausrüstung kann durch zusätzliches Leveln ausgeglichen werden.

Beispiel:

```text
Spieler mit guter Ausrüstung
Level 20
      ↓
kann stärkere Inhalte früher bewältigen

Solo-Spieler mit Basic-Ausrüstung
Level 20
      ↓
hat es zunächst schwerer
      ↓
levelt länger
      ↓
Level 22 / 23 / 24
      ↓
zusätzliche Charakterstärke gleicht
einen Teil des Ausrüstungsnachteils aus
      ↓
kann ebenfalls weiterkommen
```

Damit existieren unterschiedliche Progressionsgeschwindigkeiten, aber keine künstliche Gildenpflicht.

---

# 20. Ausrüstung oder Zeit

Ein Spieler mit Gilde, Gruppe oder guter Ausrüstung kann bestimmte Herausforderungen früher bewältigen.

Ein Solospieler kann stattdessen mehr Zeit investieren und zusätzliche Charakterstärke aufbauen.

Grundprinzip:

> **Bessere Ausrüstung spart Zeit – sie schaltet den Fortschritt nicht exklusiv frei.**

Und:

> **Basic-Ausrüstung ermöglicht den Solo-Fortschritt. Schlechtere Ausrüstung kann durch zusätzliches Leveln und damit durch mehr Spielzeit ausgeglichen werden.**

---

# 21. Verbindung mit dem NPC-Wissenssystem

Questfortschritt und NPC-Wissen bleiben getrennte Systeme, können aber miteinander kommunizieren.

Beispiel:

```text
Spieler erhält Brief von Borin
        ↓
Quest:
Brief zu Elara bringen
        ↓
Spieler erreicht Elara
        ↓
Brief wird tatsächlich übergeben
        ↓
QuestService bestätigt Übergabe
        ↓
Knowledge-System informiert Elara
        ↓
Elara kennt nun den Inhalt
```

Elara darf nicht allein deshalb etwas über den Brief wissen, weil die entsprechende Quest existiert.

Grundregel:

> **NPCs dürfen nur auf Informationen reagieren, die sie tatsächlich erhalten haben.**

---

# 22. Verbindung mit Dynamic Scenes

Quest-Ereignisse können Dynamic Scenes auslösen.

Beispiele:

```text
QuestStarted
QuestStageChanged
QuestCompleted
QuestFailed
```

Eine Lua-Quest kann definieren, dass unter bestimmten Bedingungen eine Szene ausgelöst werden darf.

TypeScript prüft die Bedingungen und übergibt die Kontrolle anschließend an das Scene-System.

---

# 23. Questbelohnungen

Mögliche Belohnungen können sein:

* Erfahrung
* Geld
* Gegenstände
* Ausrüstung
* Ruf
* Beziehung
* Titel
* Freischaltungen
* neue Quests
* Story-Fortschritt
* Zugang zu Bereichen
* Rezepte
* Fähigkeiten, sofern vom Klassensystem vorgesehen

Alle Belohnungen werden serverseitig validiert und vergeben.

Lua darf beschreiben, welche Belohnung vorgesehen ist.

TypeScript führt die tatsächliche Vergabe aus.

---

# 24. i18n

Spieler sichtbare Questtexte werden nicht fest in TypeScript oder Godot eingebaut.

Lua verwendet dafür i18n-Keys.

Beispielsweise:

```lua
title_key = "quest.fieldcamp_defense.title"
description_key = "quest.fieldcamp_defense.description"
```

Godot zeigt anschließend die Sprache an, die für den jeweiligen Spieler eingestellt ist.

---

# 25. Erweiterbarkeit

Das Quest-System soll nicht nur klassische lineare Quests unterstützen.

Die gleiche Architektur soll später auch verwendet werden können für:

* Soloquests
* Gruppenquests
* Klassenquests
* Tier-Klassenquests
* Questketten
* Regionsquests
* World-Event-Quests
* NPC-Beziehungsquests
* Craftingquests
* Entdeckungsquests
* zeitlich begrenzte Quests
* Dynamic-Scene-Quests
* besondere Storyquests

Neue Questtypen sollen möglichst durch neue Lua-Definitionen und vorhandene serverseitige Mechaniken entstehen, anstatt für jede Quest Sondercode zu benötigen.

---

# 26. Architektur-Grundsatz

Das Quest-System verbindet Inhalte mit der lebenden Welt, ohne Lua oder den Client zur Autorität zu machen.

> **Lua beschreibt die Quest.**

> **TypeScript führt und prüft die Quest.**

> **MariaDB speichert den individuellen dynamischen Zustand.**

> **Godot stellt die Quest für den Spieler dar.**

Quests sollen dadurch auf Welt, Region, Klasse, Gruppe, Beziehungen und Ereignisse reagieren können, ohne dass die grundlegende Serverautorität von Andora aufgegeben wird.
