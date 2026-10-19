# Andora – Quest-System

## 1. Status

**Gesamtstatus:** 🟡 Konzept (langfristige Architektur)

**Quest V1:** Verbindlich festgelegt (Abschnitt 27)

Das Quest-System von Andora soll klassische Aufgaben ebenso unterstützen wie dynamische, klassenabhängige, gruppenbasierte und durch die Welt ausgelöste Quests.

Quests sollen nicht nur Aufgabenlisten sein, sondern auf Charakter, Klasse, Beziehungen und Ereignisse der Welt reagieren können.

Die verbindlichen Storytelling-Grundprinzipien (Hauptstory, Nebenmissionen, Weltgeheimnisse, Bücher, verborgene Questketten, variable persönliche Rätsel, spielerausgelöste Realm-Ereignisse, Realm-Chroniken) stehen in `Storytelling_und_Weltgeheimnisse.md`.
Titel und Erfolge sind in `Erfolge_und_Titel.md` definiert.
Dieses Dokument definiert nur die Ausführung: Zustände, serverseitige Prüfung, Fortschritt und Belohnung.

> **Quest V1:** Der verbindliche Implementierungsumfang für das erste Quest-System ist in Abschnitt 27 festgelegt. Er schränkt das allgemeine Quest-System für V1 bewusst ein, ohne langfristige Questideen der Abschnitte 1 bis 26 zu verwerfen.

---

# 2. Grundarchitektur

Das Quest-System wird klar zwischen statischem Inhalt, Ausführung, Speicherung und Darstellung getrennt.

## Lua – statischer Teil

Lua beschreibt, **was eine Quest ist**.

Die übergreifende Architektur der serverseitigen Lua-Content-Schicht (Sicherheitsgrenzen, Eventmodell, Script-Domänen, Quest-Ablaufmodelle) steht in `Lua-Scripting-System.md`.

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
* kontextgebundene Kampf-Überschreibungen (Angriffserlaubnis, Aggressivität, Verhalten; an Quest-/Dialogkontext gebunden, laufen mit Phasenende automatisch aus; Details in `Kampfsystem.md`, Abschnitt 19)

Lua verändert jedoch nicht direkt den persistenten Spielzustand.

---

## Realm-Server (Rust) – ausführender Teil

Der Realm-Server (Rust) ist die Autorität des Quest-Systems.

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

**Der Realm-Server (Rust) entscheidet, ob sie tatsächlich erfüllt ist.**

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

> **Lua beschreibt die Aufgabe. Der Realm-Server (Rust) prüft die Aufgabe. MariaDB merkt sich den Fortschritt. Godot zeigt ihn dem Spieler.**

---

# 4. Questzustände

Das Quest-System kennt folgende fünf Zustände:

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

> **Quest V1:** Die verbindlichen V1-Regeln zu Ableitung, Persistenz und FAILED der Zustände stehen in Abschnitt 27.5.

---

# 5. Questziele

Questziele werden strukturiert definiert und nicht ausschließlich als Text gespeichert.

Mögliche Zieltypen des langfristigen Quest-Systems:

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

> **Quest V1:** Verbindlich unterstützt werden die Zieltypen `kill`, `talk`, `collect` und `deliver` (Abschnitt 27.1). Die übrigen Zieltypen gehören zum langfristigen Quest-System und werden dadurch nicht verworfen; sie sind lediglich nicht Teil des V1-Implementierungsumfangs. Der Unterschied zwischen `collect` und `deliver` ist für V1 verbindlich in Abschnitt 27.2 geregelt.

Beispiel:

```text
Questtext:
"Bring Borins Brief zu Elara."

Technisches Ziel:
type       = deliver
item       = borin_letter_001
target_npc = elara
```

Dadurch kann der Realm-Server (Rust) eindeutig feststellen, ob das tatsächliche Ziel erfüllt wurde.

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

Der Realm-Server (Rust) prüft anschließend den tatsächlichen Spieler- und Weltzustand.

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

> **Lua definiert, wann eine Quest verfügbar sein kann. Der Realm-Server (Rust) entscheidet anhand des aktuellen Welt- und Spielerzustands, ob sie tatsächlich verfügbar ist.**

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

Der Realm-Server (Rust) liest den tatsächlichen Beziehungszustand des Charakters.

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

## Gemeinsame Gruppenaufträge (spätere Erweiterung – vereinbarte Richtung)

Diese Richtung ist **vereinbart**, aber **nicht** Teil von Quest V1 und **nicht implementiert**
(§27.11). Sie führt keine neue Zustandsmaschine ein; Annahme, Fortschritt und Abschluss nutzen
die bestehenden Wege (§27.26–§27.28). Die einzelnen Punkte sind **Vereinbarungen**, keine
Implementierung. Die individuellen persönlichen Aufträge aus §12–§14 sind davon zu unterscheiden.

* Ein **Gildenhaus beziehungsweise eine Auftragsstelle** kann später gemeinsame Gruppenaufträge
  anbieten. Die konkrete Zugangsregel bleibt offen.
* Der **Leader** nimmt für die Gruppe an.
* Die Quest gehört der **Gruppen-ID**; **alle Gruppenmitglieder** nehmen teil.
* Gemeinsame Teilziele dürfen in **verschiedenen Gebieten** liegen.
* **Austritt** beendet die Questteilnahme und den noch offenen Belohnungsanspruch; bereits
  erzielter Gruppenfortschritt bleibt erhalten.
* **Neue Mitglieder** übernehmen den vorhandenen Fortschritt und haben Anspruch auf die noch
  ausstehende Abschlussbelohnung.
* **Leaderwechsel** erhält den Auftrag.
* **Gruppenauflösung** beendet den Auftrag.
* **Disconnect allein ist kein Austritt**, solange die Mitgliedschaft besteht
  (`Gruppensystem.md` §9).
* Beim **verbindlichen Abschluss** wird die **Empfängerliste fixiert**. Ein späterer Beitritt
  erzeugt **keinen rückwirkenden Anspruch**. Eine Wiederaufnahme darf **keine doppelte
  Auszahlung** erzeugen.
* Die tatsächliche Empfängerzuordnung und die Anbindung an den bestehenden Abschlussvertrag
  (`quest_completion`, `character_quest_commits`) sind noch zu entwerfen. Die bestehenden
  Abschlussregeln gelten für eine einzelne Charakterzeile; eine spätere gemeinsame
  Gruppenabrechnung muss daran anschließen, ohne den Doppelauszahlungsschutz zu schwächen.

Querverweise: `Gruppensystem.md` §2/§8/§9 (Mitgliedschaft, Leaderwechsel, Austritt,
Disconnect/Reconnect), `KI_Personalisierungs_Questsystem_v2.md` §4.5 (persönliche
Gruppenmitglieder-Aufträge).

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

Der Realm-Server (Rust) prüft die Bedingungen und übergibt die Kontrolle anschließend an das Scene-System.

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

Die Höhe der Quest-EXP-Belohnung wird von der jeweiligen Quest selbst festgelegt (keine globale EXP-Formel); die Grundprinzipien der Charakter-EXP stehen in `Erfahrung_und_Progressionssystem.md` (Abschnitt 10).

Lua darf beschreiben, welche Belohnung vorgesehen ist.

Der Realm-Server (Rust) führt die tatsächliche Vergabe aus.

## Statusquests und Statusbelohnungen (spätere Erweiterung – vereinbarte Richtung)

Diese Richtung ist **vereinbart**, aber **nicht implementiert** und **nicht** Teil von Quest V1.
Sie ändert die verbindliche Reward-Semantik des Abschlusses (§27.26–§27.28) nicht. Die einzelnen
Punkte sind **Vereinbarungen**, keine Implementierung.

* Statusquests sind **solo und als Gruppenaufträge** vorgesehen. Gruppenquests vergeben
  **höhere Statusbelohnungen**; konkrete Zahlen bleiben offen.
* **Persönlicher Status** und **Gildenstatus** sind **getrennte Bestände**.
* **Persönlicher Status** ist ausgebbar, beispielsweise für Prestigehäuser, die **sowohl Idia
  als auch Statuspunkte** kosten (siehe `Housing.md`).
* **Gildenstatus** dient dem **Gildenaufstieg**. Höhere **Gildenlevel** erweitern
  **Gildenbankkapazität** und **Mitgliederplätze**
  (siehe `exp2_Region_Mandalonien_Gildenstadt_Wirtschaft.md`).
* Gildenstatus soll außerdem zum **regionalen politischen Einfluss** und zum möglichen Aufstieg
  als **Hauptstadtverwalter** beitragen (siehe `Politik-Herrschaftssystem.md` §27).
* **Noch nicht festgelegt:** das Verhältnis von ausgebbarem Gildenstatus, Gildenlevel und
  politischem Einfluss. Es wird **keine** getrennte Einflusswährung, **keine**
  Berechnungsformel und **keine** automatische Statusbelohnungsaufteilung erfunden.
* Bei Gruppen mit **mehreren Gilden** bleibt die Zurechnung des Gildenanteils offen.

Querverweise: `KI_Personalisierungs_Questsystem_v2.md` (persönliche/Statusquests),
`Politik-Herrschaftssystem.md` §27, `Housing.md`, `exp2_Region_Mandalonien_Gildenstadt_Wirtschaft.md`.

---

# 24. i18n

Spieler sichtbare Questtexte werden nicht fest im Realm-Server (Rust) oder in Godot eingebaut.

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
* verborgene Questketten (ohne sichtbaren Questgeber)
* Regionsquests
* World-Event-Quests
* Realm-Event-Quests
* NPC-Beziehungsquests
* Craftingquests
* Entdeckungsquests
* Rätsel-Quests
* zeitlich begrenzte Quests
* Dynamic-Scene-Quests
* besondere Storyquests

Neue Questtypen sollen möglichst durch neue Lua-Definitionen und vorhandene serverseitige Mechaniken entstehen, anstatt für jede Quest Sondercode zu benötigen.

> **Quest V1:** Abschnitt 27 legt verbindlich fest, welcher Teil dieses erweiterbaren Systems Teil des ersten Implementierungskerns ist. Die hier genannten langfristigen Typen bleiben erhalten.

---

# 26. Architektur-Grundsatz

Das Quest-System verbindet Inhalte mit der lebenden Welt, ohne Lua oder den Client zur Autorität zu machen.

> **Lua beschreibt die Quest.**

> **Der Realm-Server (Rust) führt und prüft die Quest.**

> **MariaDB speichert den individuellen dynamischen Zustand.**

> **Godot stellt die Quest für den Spieler dar.**

Quests sollen dadurch auf Welt, Region, Klasse, Gruppe, Beziehungen und Ereignisse reagieren können, ohne dass die grundlegende Serverautorität von Andora aufgegeben wird.

---

# 27. Quest V1 – verbindlicher Umfang

Dieser Abschnitt legt den verbindlichen Umfang des ersten Quest-Systems (**Quest V1**) fest.

Er präzisiert die allgemeinen Aussagen der Abschnitte 1 bis 26 für V1 und schränkt sie für den ersten Implementierungskern bewusst ein. Langfristige Questideen der übrigen Abschnitte werden dadurch nicht verworfen; sie gehören lediglich nicht zum V1-Umfang.

Die nachfolgenden Regeln gelten verbindlich für die Quest-V1-Implementierung.

---

## 27.1 V1-Zieltypen

Quest V1 unterstützt verbindlich vier Zieltypen:

```text
kill
talk
collect
deliver
```

Beispiele:

```text
kill:    "Töte 5 Wölfe."
talk:    "Sprich mit Borin."
collect: "Sammle/Besitze 8 Wolfsfelle."
deliver: "Bringe Borin 8 Wolfsfelle."
```

Andere konzeptionell dokumentierte Zieltypen (z. B. `heal`, `visit`, `discover`, `interact`, `craft`, `escort`, `protect`, `destroy`, `event`, zeitabhängige Ziele) gehören nicht zum verbindlichen V1-Implementierungsumfang. Sie bleiben Teil der langfristigen Architektur (Abschnitt 5) und werden bewusst nicht gelöscht.

---

## 27.2 collect und deliver klar getrennt

`collect` und `deliver` werden in V1 klar getrennt.

**COLLECT**

Das Ziel verlangt, dass eine bestimmte Menge eines Gegenstands gesammelt bzw. vorhanden ist.

Beispiel:

```text
8 / 8 Wolfsfelle
```

Das Erfüllen eines `collect`-Ziels entfernt die Gegenstände **NICHT** automatisch.

Rust prüft den tatsächlichen autoritativen Inventarzustand.

**DELIVER**

Das Ziel verlangt die tatsächliche Übergabe einer definierten Menge eines Gegenstands an einen vorgesehenen Empfänger (NPC).

Beim Abschluss prüft Rust erneut autoritativ:

* richtige Quest
* Quest ACTIVE
* richtiges `deliver`-Ziel
* richtiger Empfänger
* benötigte Item-ID
* benötigte Menge tatsächlich vorhanden

Erst danach dürfen die Gegenstände autoritativ entfernt werden.

Der Client darf niemals behaupten können, dass die Gegenstände vorhanden oder bereits übergeben seien.

---

## 27.3 Questannahme

Quest V1 verwendet grundsätzlich eine **bewusste Annahme** durch den Spieler.

Der Spieler interagiert/spricht mit dem Questgeber-NPC.

Ist eine Quest AVAILABLE, kann der Questdialog angeboten werden.

Der Spieler erhält mindestens die logische Möglichkeit:

```text
[Annehmen]
[Ablehnen]
```

Erst nach bestätigtem Annehmen wird die Quest ACTIVE.

Rust prüft vor ACTIVE erneut autoritativ alle für V1 relevanten Voraussetzungen.

Der Client entscheidet nicht, ob eine Quest verfügbar ist.

Die genaue visuelle Gestaltung des Dialogfensters ist noch kein Bestandteil von Quest V1.

Die Präsentationsregeln für Dialog-/VN-Darstellung (Portraits, Expressions, Antworten) als
Erweiterung desselben Dialogflusses stehen in `Visual_Novel_Dialog_und_Szenendarstellung.md`;
sie ändern nichts an Quest-Autorität und -Persistenz.

---

## 27.4 Questabgabe

Quest V1 verwendet für Quests mit vorgesehenem Questgeber/Empfänger eine **bewusste Abgabe**.

Wenn die notwendigen Ziele erfüllt sind, spricht der Spieler erneut mit dem vorgesehenen NPC.

Der Abschlussdialog bietet logisch:

```text
[Quest abschließen]
```

Erst nach dieser Bestätigung führt Rust die endgültige Abschlussvalidierung durch.

Bei `deliver`:

1. Queststatus prüfen
2. Zielstatus prüfen
3. Empfänger prüfen
4. benötigte Gegenstände prüfen
5. Gegenstände autoritativ entfernen
6. Questabschluss durchführen
7. Belohnung autoritativ vergeben
8. COMPLETED persistieren

Die endgültige Implementierung muss gegen Doppelabschluss und doppelte Belohnungsvergabe geschützt sein.

---

## 27.5 Questzustände in V1

Für V1 gelten die fünf dokumentierten Zustände (Abschnitt 4):

```text
HIDDEN
AVAILABLE
ACTIVE
COMPLETED
FAILED
```

Zusätzlich gilt verbindlich:

**HIDDEN und AVAILABLE sind abgeleitete Zustände.**

Sie müssen für einen Charakter nicht als aktiver Questdatensatz in MariaDB persistiert werden.

**HIDDEN**

Die Quest ist für den Charakter derzeit nicht verfügbar bzw. ihre Voraussetzungen sind nicht erfüllt.

**AVAILABLE**

Die Voraussetzungen sind erfüllt und die Quest kann angenommen werden.

**ACTIVE**

Die Quest wurde angenommen. ACTIVE wird persistent gespeichert.

**COMPLETED**

Die Quest wurde erfolgreich abgeschlossen. COMPLETED wird persistent gespeichert.

**FAILED**

Der Zustand bleibt Bestandteil des Quest-Datenmodells.

Quest V1 implementiert jedoch noch **KEINEN allgemeinen automatischen Failed-Mechanismus** (kein Questtimeout, kein NPC-Tod = Failed, kein Spieler-Tod = Failed, kein Logout = Failed, kein Gebietswechsel = Failed).

FAILED wird ausdrücklich nicht entfernt, da spätere Questtypen diesen Zustand benötigen können.

---

## 27.6 Persistenz

Persistiert werden mindestens:

* ACTIVE
* COMPLETED
* Fortschritt aktiver Ziele
* für den Questzustand erforderliche V1-Daten

HIDDEN und AVAILABLE werden aus der Questdefinition und dem aktuellen Charakterzustand abgeleitet.

Bestehende Tabelle:

```text
quests
```

Die implementierte Tabelle `quests` stammt aus der Migration `004_quests.sql` und besitzt:

```text
char_id
quest_id
state
data
updated_at
```

**Keine neue Migration** wird für Quest V1 erstellt.

**Dokumentarische Vereinheitlichung:** Der derzeit implementierte Schema-Name ist `quests` (Migration `004_quests.sql`). Frühere Dokumentationsstellen verwendeten dafür den Namen `character_quests`; dieser Name wird zugunsten des implementierten Namens `quests` bereinigt (siehe `Datenbank_Architektur.md`, Abschnitt 19). Es findet keine Datenbankänderung statt.

Die statische Questdefinition wird nicht für jeden Spieler dupliziert.

---

## 27.7 Questdefinition vs. Spieler-Questzustand

**QUESTDEFINITION**

beschreibt die Quest selbst:

* Identität
* Texte/Keys
* Voraussetzungen
* Ziele
* Zielparameter
* Belohnungen
* Ablauf/Phasen soweit definiert

**SPIELER-QUESTZUSTAND**

beschreibt nur den individuellen Zustand eines Charakters:

* ACTIVE / COMPLETED usw.
* Fortschritt
* notwendige Laufzeitdaten

Questdefinitionen dürfen nicht für jeden Spieler vollständig dupliziert werden.

---

## 27.8 Serverautorität und Questfortschritt

Die bestehende Andora-Grundregel gilt unverändert: **Rust Realm ist autoritativ.**

**Client**

sendet Spieleraktionen und Entscheidungen.

**Rust**

validiert Questzustand, Voraussetzungen, Fortschritt, Inventar, Questgeber/Empfänger und Abschluss.

**MariaDB**

persistiert den Spieler-Questzustand.

**Godot**

zeigt Questinformationen und sendet Benutzeraktionen.

**Lua**

darf Quests beschreiben/orchestrieren und Requests auslösen, besitzt jedoch keine Autorität über Fortschritt, Inventar, Belohnungen oder Persistenz.

Questfortschritt darf nicht aus einem vom Client behaupteten Ergebnis entstehen:

* **KILL:** Fortschritt entsteht ausschließlich aus einem vom Realm bestätigten Gegner-Tod.
* **TALK:** Fortschritt entsteht ausschließlich aus einer vom Realm validierten NPC-Interaktion.
* **COLLECT:** Fortschritt/Zielerfüllung basiert auf dem autoritativen Inventarzustand.
* **DELIVER:** Fortschritt/Abschluss basiert auf der autoritativ validierten tatsächlichen Übergabe.

Keine Client-Nachricht wie

```text
"Ich habe Wolf X getötet"
"Ich besitze 8 Felle"
"Ich habe die Items abgegeben"
```

darf unmittelbar Questfortschritt erzeugen.

> **Bestandskorrektur (Codeprüfung, Quest V1.2b — Kill-Credit):** Die KILL-Regel ist jetzt im Code **eingebunden und getestet**: `combat_tick` (`src/realm-rs/src/combat/mod.rs`) meldet den **einmaligen Monster-Tod beim Übergang Alive→Dead** (nicht jeden Tick mit hp≤0) an `QuestService.add_progress` für die dokumentierten, anhand von Claim und Position bestimmten Empfänger (Gruppen-Claim `g:<gid>` → aktive Mitglieder im 100-m-Bereich um den Anführer einschließlich gestorbener Mitglieder; nach außerhalb Teleportierte → kein Fortschritt; Spieler-Claim → dieser Spieler; ohne Claim → ersatzweise der tödlich treffende Spieler — identisch zur EXP-Vergabe, §27.16/§27.12, keine neue Zuordnungsregel). Monster werden anhand der **stabilen Content-/Definitionskennung** (`Npc.content_id`, `monster_definitions`) zugeordnet, niemals anhand des zufälligen Spawn-Schlüssels oder des Anzeigenamens. Nur ACTIVE-Quests mit einem passenden `kill`-Ziel erhalten einen Zuwachs von 1 über die bestehende Begrenzungs-/Ziellogik. Reine RAM-Änderung — keine DB-Ein-/Ausgabe unter der World-Sperre (Zwischenfortschritt wird nur an Quest-Prüfpunkten persistiert, `Player_Persistenz.md` §8). Der QuestService wird im Produktionspfad über Arc gemeinsam genutzt (`main.rs` → `net::serve` Ctx + Tick-Schleife). **Weiterhin offen:** die Spielanbindung für TALK/COLLECT/DELIVER, die C2S-Nachrichten für Annahme/Abgabe und die S2C-Anzeige, Belohnungen sowie der Quest-Content-/Definitionsloader (§27.9 — die Laufzeit-Registry bleibt leer, Kill-Credit bleibt ohne passende Definition gesperrt). Dieser Hinweis kennzeichnet eine Bestandsabweichung; er ändert **keine** Regel.

---

## 27.9 Lua in Quest V1

Die bestehende Lua-Quest-Domäne und die vorgesehene Quest-Orchestrierung bleiben Teil der Andora-Architektur (siehe `Lua-Scripting-System.md`, Abschnitte 3.1 und 5).

**ABER:** Das endgültige öffentliche Quest-Scriptformat und die vollständige Spiellayer-Anbindung sind laut Lua-Dokumentation noch nicht abschließend festgelegt.

Deshalb gilt für Quest V1:

* KEIN neues Lua-Quest-Dateiformat wird festgelegt.
* KEINE endgültige Quest-Lua-API wird festgelegt.
* KEINE Realm-Lua-Verdrahtung wird in dieser Stufe implementiert.

Quest V1 muss mit der bestehenden Autoritätsregel kompatibel sein und darf Lua niemals autoritativ machen (Abschnitt 27.8).

Bestandskorrektur (Codeprüfung, Quest V1.2c — Definitionsloader): Der Definitionsloader ist im Produktionspfad eingebunden. QuestService::load_quest_definitions (src/realm-rs/src/quest.rs) lädt beim Realm-Start die Script-Domäne quests/ unter CONTENT_ROOT. Dabei werden die vorhandenen Funktionen scan_script_files, die Sandbox des ScriptManager und die neue Daten-Auswertung eval_data verwendet. Die Definitionen werden im gemeinsam genutzten Arc<QuestService> registriert und stehen damit dem Kill-Fortschritt und dem Handler-Pfad zur Verfügung. Lua liefert ausschließlich Definitionsdaten; sämtliche Zustandsänderungen und Berechtigungsentscheidungen bleiben beim autoritativen Realm gemäß §27.8.
Quest-Lua-Format V1: Ein Definitionsskript gibt eine reine Datentabelle zurück. Zulässig sind id, title_key, description_key, requirements = { level = N, requires = { "quest_id", … } }, objectives = { { id=, type=, target=, required= }, … } sowie optional repeatable als boolescher Wert und giver als stabile NPC-Content-ID (Erweiterung gemäß §27.27). Unbekannte Felder werden auf oberster Ebene sowie innerhalb von requirements und den Objectives mit einer präzisen Fehlermeldung abgewiesen.
Belohnungen: `rewards` darf fehlen oder leer sein. Die Abgabe-Vorbereitung (§27.28) validiert zusätzlich `exp` und `idia` als nichtnegative Ganzzahlen bis i64::MAX für die serverseitige Vorschau. Unbekannte oder nicht unterstützte Felder werden abgewiesen. Die Belohnungsvergabe bleibt bis zur Freigabe des gemeinsamen Persistenzvertrags gesperrt; ladbare Belohnungsdaten sind kein Nachweis für eine implementierte Vergabe.
Validierung und Registrierung: Content-Referenzen müssen auflösbar sein: kill und talk verweisen auf geladene Content-IDs aus monster_definitions, collect und deliver auf item_definitions. Auch Questverweise in requires müssen aufgelöst werden können. Die Registrierung erfolgt vollständig oder gar nicht. Fehlerhafte Definitionen verhindern den Realm-Start; ein teilweise registrierter Definitionsbestand wird nicht übernommen.
Verzeichnisverhalten: Ein fehlendes oder leeres Verzeichnis <CONTENT_ROOT>/quests/ ergibt Ok(0). Der Realm startet dann mit einem leeren Definitionsbestand, da Quests optionaler Content sind. Ohne passende Definition werden weder Questangebote freigegeben noch Kill-Fortschritte zugeordnet. Fehlerhafte vorhandene Skripte führen dagegen zum Startabbruch.
Dokumentierte Vertragsänderung: Die bisherige Aussage in §27.9, dass kein neues Lua-Quest-Dateiformat festgelegt werde, wird durch das hier ausdrücklich festgelegte minimale Format V1 ersetzt. Die vollständige Lua-API und Orchestrierung bleiben offen. Das Beispiel aus quests_stories.md §3 verwendet die nicht unterstützten Schlüssel `xp`/`gold` und ist nicht unverändert ladbar; die Vorschau-Erweiterung §27.28 verwendet ausdrücklich `exp`/`idia`. Diese Formatfestlegung erfindet keine Belohnungsmengen.

---

## 27.10 Zeitbegrenzte Quests – später

Zeitbegrenzte Quests gehören **NICHT** zum Quest-V1-Implementierungsumfang.

Die Architektur soll sie später jedoch ermöglichen.

Beispiel:

```text
"Borin hat Hunger. Bringe Borin innerhalb von 10 Minuten etwas zu essen."
```

Späteres Grundprinzip:

* Zeitlimits müssen serverseitig/autoritativ ausgewertet werden.
* Der Client darf die verbleibende Zeit anzeigen, aber nicht bestimmen.
* Ein späteres Zeitlimit kann beispielsweise `ACTIVE → FAILED` auslösen.

Die genaue spätere Semantik von Logout während Timer, Serverneustart, absolute vs. aktive Spielzeit, Wiederholbarkeit und erneuter Annahme nach FAILED wird mit dieser Festlegung ausdrücklich **NICHT** bestimmt.

Keine Timer-Implementierung wird erstellt.

---

## 27.11 Nicht Teil von Quest V1

Ausdrücklich **nicht** Bestandteil des ersten V1-Kerns:

* zeitbegrenzte Quests
* Escort
* Crafting-Ziele
* Regions-/World-Event-Quests
* komplexe Area-/Discover-Ziele
* automatische FAILED-Trigger
* Gruppenquests als eigenes System
* Questmarker-/Kartensystem
* dynamische KI-generierte Live-Quests
* NPC-Beziehungsquests
* Quest Editor Implementierung
* Lua Watchdog/Failover
* Lua Monitoring/Telemetry

Diese Dinge werden dadurch nicht verworfen.

> **Einordnung (Auftragsdokumentation):** Die genannten Punkte bleiben weiterhin außerhalb des
> V1-Implementierungsumfangs. Einige davon sind inzwischen als **spätere, vereinbarte Richtung**
> dokumentiert: gemeinsame Gruppenaufträge (§14), Statusquests und Status (§23) sowie
> persönliche, gesprächsbasierte NPC-Aufträge (`KI_Personalisierungs_Questsystem_v2.md` §4.5).
> Diese Nennung behauptet **keine** Implementierung und ändert die V1-Abgrenzung nicht.

---

## 27.12 Gruppen-Kill-Credit – geregelt

Die zuvor offene Frage, ob bei `kill` automatisch nur der Claim-Spieler Questfortschritt erhält, ist **verbindlich entschieden** (Abschnitt 27.16). Einzelspieler, Gruppen, Gruppen-Claim, Entfernung zum Kill sowie lebend/tot innerhalb der Gruppe sind dort positionbasiert geregelt.

Ausdrücklich **keine** weiteren Beteiligungs-/Damage-Regeln: Die Coding-KI darf über die Positionsregel in Abschnitt 27.16 hinaus **keine zusätzlichen** Schadensanteil-, Assist- oder Ownership-Regeln selbst erfinden.

---

## 27.13 Inventory-Remove für deliver

Quest V1 benötigt für `deliver` eine autoritative Möglichkeit, eine exakt validierte Itemmenge aus dem Spielerinventar zu entfernen.

Eine allgemeine passende remove-/Übergabe-API ist derzeit nicht vorhanden (Ergebnis des Doku-/Code-Audits).

Diese API wird in dieser Dokumentationsstufe **nicht** implementiert; sie ist eine technische Voraussetzung für die spätere `deliver`-Implementierung.

> **Bestandskorrektur (Codeprüfung, Quest V1.2a):** Dieses Prüfergebnis ist gegenüber dem aktuellen Repository **veraltet**. Im Code vorhanden sind `InventoryState.try_remove(item_id, qty)` (`src/realm-rs/src/inventory.rs`, mit Dokumentationsverweisen auf §27.13/§27.26 — exakt validierte Mengenentnahme, keine Teilentnahme, keine negative Menge) und die atomare Übergabe-/Abschluss-Transaktion `QuestService.complete_for_character` (`src/realm-rs/src/quest.rs`) mit der DB-Schutzprüfung `db::guarded_complete_quest` (`UPDATE state=2 … WHERE state=1`, exakt 1 Zeile). Beide sind durch reine Modultests abgedeckt. **Es fehlt nur noch die Spiel-/Protokollanbindung** (C2S-Abgabenachricht, Handler, registrierte Questdefinition), die diesen vorhandenen Kern erreicht — **nicht** die Entnahme-/Übergabe-Grundoperation selbst. Dieser Hinweis kennzeichnet eine Bestandsabweichung; er ändert **keine** Regel.

Dabei muss insbesondere verhindert werden:

* negative Mengen
* Entfernen nicht vorhandener Items
* doppelte Übergabe
* Teilentfernung mit anschließendem fehlerhaften Questabschluss
* Belohnung trotz fehlgeschlagener Übergabe

Die genaue technische Transaktionsgrenze wird beim Coding-Auftrag festgelegt, ohne neue Gameplayregeln zu erfinden.

---

## 27.14 Implementierungsplan V1

Der Implementierungsplan für Quest V1 wird in drei kleine, separat testbare Blöcke unterteilt:

**QUEST V1.1 – Server-/Datenkern**

* Questmodul mit V1-Definitionsstruktur
* Zustandsmodell (ACTIVE/COMPLETED persistent; HIDDEN/AVAILABLE abgeleitet)
* QuestService: Annahme, Zielerfüllung, Abschluss
* `kill`-Zielgrundlage (Realm-bestätigter Gegner-Tod) mit offenem Gruppen-Kill-Credit (Abschnitt 27.12)
* Persistenz über die bestehende Tabelle `quests` (Migration 004)

**QUEST V1.2 – NPC-Talk + collect + deliver**

* `talk`-Ziel über validierte NPC-Interaktion
* `collect`-Ziel über autoritativen Inventarzustand
* `deliver`-Ziel über autoritative Übergabe
* notwendige Inventory-Schnittstelle für die exakte Entfernung validierter Mengen (Abschnitt 27.13)

**QUEST V1.3 – Protokoll + Godot-Client-Basis**

* Protokollnachrichten für Questanzeige, Annahme, Abgabe, Fortschritt
* Godot-Client-Basis: Anzeige und Senden von Benutzeraktionen (keine Fortschritts-/Belohnungsautorität)

Falls die dokumentierten Abhängigkeiten eine andere, kleinere Aufteilung sinnvoll machen, darf sie beim Coding-Auftrag vorgeschlagen werden. Dabei werden keine neuen Gameplayentscheidungen ergänzt.

---

## 27.15 Offene Punkte nach dieser Festlegung

Der Gruppen-Kill-Credit ist inzwischen verbindlich geregelt (Abschnitt 27.16).

Mindestens offen bleiben:

* endgültiges Quest-Lua-Dateiformat/-API
* konkrete Quest-Protokoll-IDs und Feldschemas
* endgültige Clientdarstellung des Questdialogs/Questlogs
* FAILED-Trigger späterer Quests
* FAILED → ACTIVE (Wiederannahme einer fehlgeschlagenen Quest)
* Abbruch → erneute Annahme einer abgebrochenen Quest
* Zeitquest-Semantik (Logout, Neustart, Spielzeit, Wiederholbarkeit)
* konkrete technische Reward-Umsetzung (Rust-API für Inventory Locks, Lock-/Reservierungs-Datenstruktur, MariaDB-Transaktionsimplementierung, RewardService-Struktur); die Ablauf- und Sicherheitsregel des Questabschlusses selbst ist verbindlich in Abschnitt 27.26 geregelt (siehe auch Abschnitt 27.13)
* endgültige Repeatable-Regeln (sofort wiederholbar, täglich, wöchentlich, Cooldown, Fraktionsbedingungen, Verhalten nach FAILED und nach Abbruch, Resetzeitpunkt, Fortschrittsreset)
* Area-/Discover-Modell
* spätere Questzieltypen
* endgültige Questitem-Verkaufspreise
* automatische Wiederherstellung zerstörter/verlorener Questitems
* konkrete erste Beispielquest/Content
* gemeinsame Gruppenaufträge: Zugangsregel, Empfängerzuordnung und Gruppenabschluss (spätere vereinbarte Richtung, §14)
* Statusquests und Status: Höhe, Verhältnis von Gildenstatus/Gildenlevel/politischem Einfluss, Zurechnung bei mehreren Gilden (spätere vereinbarte Richtung, §23)

Weitere echte Blocker, die erst bei der Implementierung entdeckt werden, werden in der jeweiligen Implementierungsphase dokumentiert.

---

## 27.16 Gruppen-Kill-Credit (verbindlich)

Die Regel für EXP- und Questfortschritt bei relevanten Gruppen-Kills ist verbindlich wie folgt geregelt:

Eine Gruppe besitzt einen **Gruppenbereich** mit einem Radius von **100 Metern** um den **Gruppenleader**.

Für EXP und Questfortschritt bei relevanten Gruppen-Kills gilt:

* Ein Gruppenmitglied erhält Credit nur, wenn es sich **innerhalb dieses 100-Meter-Gruppenbereichs** befindet.
* Ein **gestorbener Spieler**, dessen Charakter/Körper noch innerhalb dieses Bereichs liegt, erhält weiterhin EXP und Questfortschritt für entsprechende Gruppen-Kills.
* Hat sich der Spieler zu einem Spawn-/Respawnpunkt teleportiert (siehe `Kampfsystem.md`, Abschnitte 13–14) und befindet sich dadurch **außerhalb** des Gruppenbereichs, erhält er für entsprechende Kills **keinen** Credit.

Keine darüber hinausgehenden Beteiligungs-/Damage-Regeln: Es werden keine zusätzlichen Schadensanteil-, Assist- oder ähnliche Beteiligungsregeln erfunden (Abschnitt 27.12). Die Regel beschreibt ausschließlich den positionsbezogenen Credit.

---

## 27.17 Normale Items vs. Quest-Sammelitems (verbindlich)

Die Questformulierung bestimmt, welche Art von Ziel vorliegt.

**NORMALE ITEMS**

Beispiel:

```text
"Bringe mir 20 Kupfererz."
```

Hier darf **vorhandenes normales Kupfererz** aus dem Inventar berücksichtigt werden. Der Spieler muss es nicht zwingend erst nach Questannahme sammeln.

**SPEZIELLE QUESTITEMS**

Davon zu unterscheiden sind spezielle Questitems für Sammelziele:

* Spezielle Questitems für Sammelziele droppen **nur, solange die zugehörige Quest ACTIVE ist**.
* Der Spieler muss diese Questitems während der aktiven Quest **tatsächlich erlangen/farmen**.

Diese Unterscheidung ist verbindlich.

---

## 27.18 Deliver mit vorhandenen normalen Items (verbindlich)

Bei einem `deliver`-Ziel mit normalen Gegenständen dürfen Gegenstände verwendet werden, die **bereits vor der Questannahme** im Inventar vorhanden waren.

Beispiel:

```text
"Bringe Borin 20 Kupfererz."
```

Besitzt der Spieler bereits 20 geeignetes Kupfererz, darf dieses bei der autoritativen Übergabe (Abschnitte 27.2/27.4) berücksichtigt werden.

Spezielle Questitems können davon abweichende Erwerbsregeln besitzen (Abschnitt 27.17).

---

## 27.19 Handel mit Questitems (verbindlich)

Solange die Quest **ACTIVE** ist:

* Questitems können **nicht** an andere Spieler gehandelt werden.
* Questitems können **nicht** regulär verkauft werden.
* Questitems dürfen **nicht** im Auktionshaus angeboten werden.

Der Spieler darf Questitems jedoch **bewusst zerstören/wegwerfen**.

Es wird **keine automatische Wiederherstellung** verlorener oder zerstörter Questitems erfunden.

---

## 27.20 Questabbruch (verbindlich)

Spieler dürfen eine ACTIVE Quest **manuell abbrechen**.

Beim Abbruch werden Questitems nach Herkunft unterschieden:

**A) Questitems, die dem Spieler beim Start/Beginn der Quest direkt gegeben wurden:**

Diese werden beim Questabbruch **zerstört/entfernt**.

**B) Sammel-Questitems, die der Spieler während der Quest durch Drops erhalten/erfarmt hat:**

Diese bleiben nach dem Questabbruch **im Inventar**.

Nach dem Abbruch dürfen solche verbliebenen Sammel-Questitems an dafür geeignete **NPC-Händler verkauft** werden.

Sie dürfen weiterhin **NICHT**:

* zwischen Spielern gehandelt werden
* im Auktionshaus angeboten werden

Ziel dieser Regel ist ausdrücklich, dass Questitems **nicht das Auktionshaus mit Questmaterial überschwemmen**.

Es werden **keine konkreten Verkaufspreise** festgelegt. Es wird **keine allgemeine NPC-Händlerlogik** implementiert.

---

## 27.21 Wiederholbare Quests (grundsätzliche Unterstützung, Semantik offen)

Das Quest-System muss wiederholbare Quests **grundsätzlich unterstützen**. Sie werden später insbesondere für **Fraktionen** wichtig.

Die genaue Wiederholungssemantik ist **NOCH NICHT festgelegt**.

Bewusst noch nicht bestimmt:

* sofort wiederholbar
* täglich
* wöchentlich
* Cooldown
* Fraktionsbedingungen
* Verhalten nach FAILED
* Verhalten nach Abbruch
* Resetzeitpunkt
* Fortschrittsreset

Die Architektur darf Repeatable daher **nicht ausschließen**.

Die konkrete Regel wird später **separat entschieden**.

---

## 27.22 Questgeber und Abgabe-NPC (verbindlich)

Questgeber und Quest-Abgabe-NPC **müssen nicht identisch** sein.

Beispiel:

```text
Borin kann eine Quest vergeben, deren Abschluss bei Isalla erfolgt.
```

Definitionen müssen daher **logisch** zwischen Questgeber und vorgesehenem Abgabe-/Empfänger-NPC unterscheiden können.

Es werden **keine konkreten Datenstrukturen oder Lua-APIs** dafür festgelegt.

---

## 27.23 Verbindliche Quest-Designregel: Parallelität vor künstlicher Sequenzierung

Diese Regel ist **verbindlich**:

> **„Parallelität vor künstlicher Sequenzierung."**

Questziele werden **parallel aktiviert**, wenn ihre gleichzeitige Bearbeitung inhaltlich und spielerisch sinnvoll ist.

**Sequenzielle Questphasen** werden nur eingesetzt, wenn:

* Handlung,
* Entdeckung,
* oder eine echte logische Abhängigkeit

eine Reihenfolge erfordern.

Questphasen dürfen **NICHT ausschließlich** dazu dienen:

* Laufwege künstlich zu verlängern
* bereits erledigte Tätigkeiten wiederholen zu lassen
* Spielzeit künstlich zu strecken

**Beispiel parallel:**

```text
"Töte 10 Wölfe und sammle 5 Wolfsfelle."
```

Kill und Sammeln laufen gleichzeitig. Der Spieler soll nicht erst 10 Wölfe töten und anschließend noch einmal dieselben Gegner farmen müssen, nur weil die Objectives künstlich hintereinandergeschaltet wurden.

**Beispiel sequenziell:**

```text
"Sprich mit Borin."
```

Erst durch das Gespräch erfährt der Spieler von einem versteckten Lager. Danach:

```text
"Finde das versteckte Lager."
```

Hier ist die Reihenfolge durch die Handlung logisch begründet.

---

## 27.24 Quest-Stages (Gameplay-/Designregel)

Die langfristige Questarchitektur muss sowohl:

* mehrere gleichzeitig aktive Objectives
* als auch logisch aufeinanderfolgende Questphasen/Stages

ermöglichen.

Innerhalb einer Stage können mehrere Objectives **parallel** aktiv sein.

Eine folgende Stage wird nur verwendet, wenn die Questlogik tatsächlich eine Reihenfolge benötigt (Abschnitt 27.23).

**WICHTIG:**

* Kein endgültiges Lua-Stage-Format wird festgelegt.
* Keine Protokollstruktur wird festgelegt.
* Keine konkrete Persistenzstruktur wird festgelegt.

Es wird **nur die Gameplay-/Designregel** dokumentiert.

---

## 27.25 Quest-KI / Quest-Editor-Regel (verbindlich)

Die Quest-KI bzw. der Quest-Editor darf **nicht automatisch** aus mehreren Objectives eine künstliche lineare Kette erzeugen.

Bei KI-unterstütztem Questdesign gilt ebenfalls die verbindliche Parallelitätsregel (Abschnitt 27.23):

* **Parallelität bevorzugen**, wenn die Aufgaben logisch gleichzeitig erledigt werden können.
* **Sequenz nur** bei narrativer oder logischer Abhängigkeit.

Diese Regel gilt gleichermaßen für menschliche und KI-unterstützte Questerstellung und wird auch im Andora Studio verbindlich gehalten (siehe `Andora-Studio.md`, Abschnitt 7, Quest Editor).

Keine Studio-Implementierung.

---

## 27.26 Questabschluss und Reward-Transaktion (verbindliche Ablauf- und Sicherheitsregel)

Diese Regel ist **verbindlich**. Sie ergänzt die ausdrückliche Abgabe aus Abschnitt 27.4 sowie die Anforderungen an die zukünftige Inventory-Remove-API aus Abschnitt 27.13 um die Abschluss- und Belohnungsreihenfolge sowie die Reservierungs- und Fehlerabsicherung des Questabschlusses.

Sie legt ausschließlich Ablauf- und Sicherheitsregeln fest. Es werden **keine** neuen Gameplayregeln erfunden, **keine** neue technische API festgelegt und **kein** Code implementiert.

### Grundregel

Der Questabschluss folgt logisch der Reihenfolge:

```text
PRÜFEN → RESERVIEREN/LOCKEN → ABSCHLIESSEND VALIDIEREN
→ AUTORITATIV AUSFÜHREN → COMPLETED → CLIENT BESTÄTIGEN
```

`COMPLETED` steht dabei logisch **am Ende** der Abschlussoperation und wird nur erreicht, wenn alle vorherigen Schritte erfolgreich abgeschlossen sind.

### Vollständige Vorprüfung

Vor Beginn der Abschlussoperation prüft der Realm serverseitig und autoritativ:

* die Quest existiert
* der Questzustand ist `ACTIVE`
* alle erforderlichen Objectives sind erfüllt
* es ist der richtige Abgabe-/Empfänger-NPC
* benötigte Deliver-/Questitems sind vorhanden (richtige Item-IDs, ausreichende Mengen)
* die Belohnungen sind grundsätzlich vergabefähig
* die freie Inventarkapazität reicht für die vollständige Itembelohnung
* die Quest ist nicht bereits abgeschlossen

Schlägt die Vorprüfung fehl, bleibt die Quest `ACTIVE`. Es werden **keine** Items entfernt, **keine** Belohnung vergeben und **kein** `COMPLETED` gesetzt.

### Volles Inventar – keine Ausweichlösung

Die **vollständige** Itembelohnung muss in das Inventar passen. Reicht die Inventarkapazität nicht, findet **kein** Abschluss statt: Die Quest bleibt `ACTIVE`, und der Spieler muss Platz schaffen und den Abschluss später erneut versuchen.

Für Quest V1 gibt es ausdrücklich **keine** Ausweichlösung:

* kein Post-/Mail-Versand der Belohnung
* kein Fallenlassen/Platzieren am Boden
* keine Teilbelohnung
* kein automatisches Zwischenlager

### Item- und Mengen-Lock (Reservierung)

Nach erfolgreicher Vorprüfung werden die für den Abschluss benötigten Gegenstände reserviert.

Reservierte Bestandteile dürfen für die Dauer der Reservierung **nicht**:

* verschoben werden
* verkauft werden
* gehandelt werden
* im Auktionshaus angeboten werden
* weggeworfen/zerstört werden
* verbraucht werden
* anderweitig entfernt werden

Der Lock schützt ausschließlich den geprüften Zustand während der kurzen Abschlussoperation. Er ist **keine** dauerhafte Einschränkung des Inventars und keine neue, über die Reservierung hinausgehende Gameplayregel.

### Stacks – nur die benötigte Menge

Bei stapelbaren normalen Items wird **nur die tatsächlich benötigte Menge** reserviert.

Beispiel:

```text
Spieler besitzt 50 Kupfererz. Das deliver-Ziel verlangt 20.
→ Es werden 20 Kupfererz reserviert. Die übrigen 30 bleiben normal nutzbar.
```

Sofern eine sichere Mengenreservierung möglich ist, wird **nicht** unnötigerweise der vollständige Stack gesperrt.

### Inventarkapazität reservieren

Während der Abschlussoperation muss zudem sichergestellt sein, dass ausreichend freie Slots für die vollständige Itembelohnung vorhanden bleiben. Parallele Loot- oder andere Inventarveränderungen dürfen diese Kapazität während der Operation nicht belegen.

Es wird **nur diese Semantik** verbindlich festgelegt. Die konkrete technische Umsetzung (z. B. eine Slot-Reservierung) wird hier **nicht** festgelegt.

### Dauer des Locks

Der Lock beginnt erst beim tatsächlichen Abschlussauslöser:

```text
[Quest abschließen]
```

Er beginnt **nicht** bereits beim Öffnen des Abschluss-/Questdialogs. Ein geöffnetes Questfenster darf das Inventar nicht blockieren. Der Lock gilt ausschließlich für die kurze serverseitige Abschlussoperation.

### Autoritative Abschlussoperation

Die Abschlussoperation läuft serverseitig autoritativ in dieser logischen Reihenfolge:

```text
1. den finalen, geschützten Zustand abschließend validieren
2. die benötigten Deliver-/Questitems entfernen
3. die vollständigen Itembelohnungen ins Inventar übernehmen
4. weitere vorgesehene Belohnungen (EXP/Geld) vergeben
5. die Quest persistent auf COMPLETED setzen
6. die Reservierungen/Locks freigeben
7. den Erfolg an den Client bestätigen
```

Der Client darf **keinen** dieser Schritte selbst ausführen oder festlegen.

### Fehler während des Abschlusses

Es darf **keinen** teilweisen Abschluss geben:

* keine Items, die ohne versprochene Belohnung verloren gehen
* keine Belohnung ohne erfolgreich validierte Übergabe
* keine Teilbelohnung
* keine Doppelbelohnung
* kein `COMPLETED`, wenn die Übergabe fehlgeschlagen ist

Deshalb ist eine geeignete **atomare/transaktionale Sicherheitsgrenze** erforderlich, die Itementfernung, Belohnungsvergabe und Persistenz gemeinsam absichert. Die konkrete technische Transaktionsgrenze wird bei der späteren Implementierung des Abschlussablaufs festgelegt; diese Dokumentation legt **keine neue technische API** fest.

### Fehlerfall / Rollback

Schlägt die Abschlussoperation fehl:

* es wird **kein** Abschluss bestätigt
* die Quest bleibt `ACTIVE`
* es gibt **keine** Teilbelohnung, die als Erfolg gewertet wird
* alle Reservierungen/Locks werden freigegeben

Es entsteht insbesondere **kein Itemverlust** und **keine Doppelbelohnung**.

### Client-Autorität

Der Client löst den Abschlussversuch nur aus. Er bestimmt **nicht**:

* ob die Objectives erfüllt sind
* ob die benötigten Items vorhanden sind
* ob Inventarplatz vorhanden ist
* welche Items entfernt werden
* welche Belohnungen vergeben werden
* ob `COMPLETED` erreicht wird

All dies entscheidet ausschließlich der Realm (Abschnitt 27.8).

### Reward-Arten

Es werden **keine neuen Belohnungsarten** erfunden. Vorgesehen sind für Questabschlüsse:

* Itembelohnungen
* EXP/Geld, sofern vom jeweiligen System unterstützt

Es werden **keine** Fraktionsruf-, Titel-, Skill- oder Achievement-Belohnungen als Reward-Semantik eines Questabschlusses festgelegt.

### Doppelabschluss-Schutz

Die bestehende Regel bleibt verbindlich: Ein Abschluss derselben (nicht wiederholbaren) Quest kann nicht doppelt erfolgen, und eine wiederholte Abschlussanfrage kann keine zweite Belohnung erzeugen (Abschnitt 27.4; serverseitig im Quest-Kern abgesichert).

Die hier beschriebene Reservierungs-/Transaktionsregel unterstützt diesen Schutz, es wird jedoch **keine** konkrete Netzwerk-Idempotency-API festgelegt.

> **Bestandskorrektur (Codeprüfung, Quest V1.2b):** Die implementierte DB-Schutzprüfung (`db::guarded_complete_quest`, `UPDATE state=2 … WHERE state=1`) schützt den **ACTIVE→COMPLETED-Übergang der `quests`-Zeile** (Doppelabschluss, Rollback ohne COMMIT). Sie allein ist **kein Nachweis** für die Atomarität künftiger **Belohnungen**: Belohnungen sind noch nicht Bestandteil der Definition/Transaktion. Bei der späteren Implementierung der Belohnungen müssen der RAM-Zustand (Quest-/Inventar-/Progressionszustand), das Schreiben in die DB und die bestehenden Spool-/Revisionspfade (`Player_Persistenz.md` §15 Generation/Revision, §23 Stufe-B Spool) **gemeinsam** betrachtet werden — eine außerhalb der geschützten Transaktion (oder nur im RAM) geschriebene Belohnung würde deren Schutz nicht übernehmen. Dieser Hinweis kennzeichnet eine Bestandsabweichung; er ändert **keine** Regel.

### Ausdrücklich nicht implementieren

Mit dieser Festlegung wird **kein** Code erstellt:

* kein Inventory-Lock-Code
* keine Slot-Reservierung
* kein RewardService
* keine DB-Transaktion
* kein Questabschluss-Code
* keine Netzwerkframes
* kein Lua-Code
* kein Godot-Code
* keine Datenbankmigrationen

### Noch offen (technische Entscheidungen)

Bewusst weiterhin **nicht** festgelegt:

* konkrete Rust-API für Inventory Locks
* Lock-/Reservierungs-Datenstruktur
* konkrete MariaDB-Transaktionsimplementierung
* Struktur eines späteren RewardService
* endgültige Repeatable-Semantik (Abschnitt 27.21)
* FAILED → ACTIVE (Wiederannahme, Abschnitt 27.10)
* Abbruch → erneute Annahme (Abschnitt 27.20)
* Zeitquest-Semantik (Abschnitt 27.10)
* Quest-Lua-Format/-API (Abschnitt 27.9)
* Quest-Protokoll-IDs

## 27.27 NPC-Questangebot und ausdrückliche Annahme (eingebaut)

Diese Erweiterung ersetzt die früheren offenen Annahme-/Anzeigegrenzen aus
§27.8/§27.15: Der Pfad für NPC-Angebote und Annahme ist serverseitig eingebaut.
Abgabe, Belohnungen, Questlog und weitere Zielanbindungen bleiben offen.

### Questgeber und Definitionsvertrag

Das optionale Feld `giver` ist eine nichtleere Zeichenkette mit der stabilen
Content-ID aus `monster_definitions`. Der vorhandene Definitionsloader prüft
die Referenz gegen den tatsächlich geladenen NPC-/Monster-Definitionsbestand,
bevor er den gesamten Definitionssatz registriert. Ungültiger Typ, leere oder
unauflösbare Referenz verhindern den Ladevorgang ohne Teilregistrierung.
Ohne `giver` bleibt eine Definition unverändert ladbar, bietet aber keine
NPC-Annahme an. `turn_in` und eine Abgabe-NPC-Zuordnung werden nicht eingeführt.
Die konkrete NPC-Instanz wird über `npc_id` ausgewählt; die fachliche
Questgeberzuordnung erfolgt ausschließlich über deren `Npc.content_id`,
nicht über Spawn-Schlüssel oder Anzeigename.

### NPC_TALK / NPC_TEXT

Die vorhandenen IDs C2S 6 / S2C 8 bleiben erhalten. Zusätzlich zu den
unveränderten Händleraktionen gelten:

* `NPC_TALK {npc_id, action:"quest_offers"}` fragt verfügbare Angebote ab.
* `NPC_TALK {npc_id, action:"quest_accept", quest_id}` fordert die Annahme an.
* Angebotsantwort: `{ok:true, action:"quest_offers", npc_id, quests:[{quest_id,
  title_key, description_key, state:"AVAILABLE", can_accept:true}]}`.
  Eine leere Liste ist ein erfolgreiches Ergebnis. HIDDEN, ACTIVE, COMPLETED
  und FAILED erscheinen nicht als annehmbare Angebote. Keine Erweiterung der
  Wiederannahme-/Repeatable-Regeln.
* Annahmeerfolg: `{ok:true, action:"quest_accept", npc_id, quest_id,
  state:"ACTIVE"}` ausschließlich nach bestätigtem DB-COMMIT und RAM-Übernahme.
* Ablehnung: `{ok:false, action, npc_id, reason}`. Stabile Gründe sind
  `unknown_npc`, `npc_unavailable`, `character_dead`, `out_of_range`,
  `unknown_quest`, `wrong_quest_giver`, `quest_not_available`,
  `quest_already_active`, `quest_already_completed`, `quest_persistence_failed`
  und `quest_accept_pending`. Bei verlorenem Eigentum erfolgt keine Aktion
  und keine Antwort an die verdrängte Verbindung.

Der Client liefert nur Aktion und Kennungen. Eigentümer und Charakterzustand
stammen aus der serverseitigen Verbindungszuordnung; behauptete `account_id`,
Level oder Questzustände sind wirkungslos. `seq` wird nur zur Korrelation
zurückgespiegelt und verleiht keine Berechtigung oder Idempotenz.

### Prüfung, Serialisierung und Commit-Grenze

Der Handler löst den Charakter aus `by_conn` auf und prüft nach Erwerb des
bestehenden `PersistRuntime.player_gate` das aktuelle Eigentum erneut.
Unter der World-Sperre prüft er NPC-Existenz, lebendigen Charakter/NPC,
`NpcStatus::Alive`, den bestehenden Interaktionsradius
`cfg.loot.pickup_radius` (`LOOT_PICKUP_RADIUS`), `giver` und die bestehenden
QuestService-Voraussetzungen (Level und abgeschlossene Vorgängerquests).
Nur tatsächlich AVAILABLEe Quests werden zur Annahme vorbereitet.

Die bestehende Sofortpersistenz wird im gemeinsamen Kern
`QuestService::persist_acceptance` wiederverwendet. In der Tabelle `quests`
(Migration 004) wird eine fehlende Zeile eingefügt; bei vorhandener Zeile
bleiben Zustand und Fortschritt unverändert. Das anschließende `SELECT … FOR
UPDATE` liest den maßgeblichen Zustand in derselben Transaktion. Es wartet
auch auf einen gegebenenfalls noch laufenden ersten INSERT/COMMIT. So erzeugt
eine identische technische Wiederholung keinen zweiten Zustand und setzt
keinen Fortschritt zurück. Ein bereits im RAM aktiver Zustand wird vor jedem
DB-Aufruf mit `quest_already_active` abgewiesen.

DB-Ein-/Ausgabe erfolgt **ohne World-Sperre**. Das gemeinsame Charakter-Gate
bleibt geholt nur über die **jeamlig** Vorprüfung und EINE bounded Persistenz-
Versuch, nicht über een endless Retry. Konkurrierente Annahme, Logout und
Takeover desselben Charakters warten daran nur solange dieses jeamlig Versuch
active; es geniet kene detached, unbounded Task, der een geordem Shutdown
indefinitely verhindern.

**BOUND_QUEST_ACCEPTANCE_IO_AND_SHUTDOWN (explicit IO-Timeout + Deadline):**
The Annahme- und Retry-Persistenzversuch carries an **explicit Zeitgrenze**
`QUEST_ACCEPT_IO_TIMEOUT` (2 s) around the **whole** attempt — Acquire + SQL-
Execute + COMMIT — via `tokio::time::timeout` (reuses the existing per-
operation Timeout-Muster of the Logout `write_timeout`, `net.rs:190`; **NO**
global Pool-Change; the production Pool keeps its sqlx-default acquire-
Timeout of 30 s, `db.rs:162`). A **Timeout is treated like an Uncertain
COMMIT**: the acceptance stays **outstanding** (pending), NO false ACTIVE,
NO claimed safe rollback. It is distinguished from a pre-COMMIT `Persistence`
error (which drops the pending entry and melds `quest_persistence_failed`).

The acceptance is registered in `World.pending_quest_acceptances` **BEFORE**
the timed attempt, so a Cancellation between attempt, registration and RAM-
Übernahme can never lose the outstanding state. After the bounded attempt
(incl. Timeout) the per-character Gate is released; the pending entry and its
idempotent Wiederaufnahme remain. `retry_quest_acceptances` wraps EACH entry-
attempt in the same IO-Timeout and is **character-scoped** via `requested`:
Logout (`finish_owner`) and Login/Takeover (`handle_hello`) pass their own
char_id, so they never wait on a foreign character's hanging entry. The
periodic pass (`main.rs`) iterates pending chars acquiring **one** Gate at a
time (never multiple), so independent characters stay securable. **Preserved
uncertainty:** in `retry_quest_acceptances` the pending entry is removed ONLY on
a confirmed COMMIT plus correct RAM-Übernahme; a pre-COMMIT failure of a LATER
attempt (e.g. at Pool-Acquire) proves NOTHING about the first COMMIT and MUST
NOT undo the original uncertainty — it stays pending. A first attempt that
safely failed pre-COMMIT is still rejected normally (`quest_persistence_failed`,
no pending).

Shutdown caps the **entire** retry pass by a real Deadline
(`tokio::time::timeout(SHUTDOWN_LOGOUT_BUDGET, retry_quest_acceptances(...))`),
so even a hanging attempt and a blocked Gate cannot push Shutdown beyond its
budget; on Deadline-elapsed the still-outstanding entries stay pending and are
reported as a defined error state (`shutdown_open_quest_acceptances`,
PersistStatus Degraded). No SIGKILL as the regular completion path, no
weakened Save-Guarantee (character Snapshots are secured independently).

**Neustart / DB-Wiederaufnahme:** the `quests`-Table remains the authority
(§27.26). The next Login re-derives ACTIVE via the existing DB-load path
(`load_for_character` → `load_quest_rows`) **if** the COMMIT had in fact
landed; otherwise the player re-accepts. **No automatic Commit-effect on a
dropped sqlx `Transaction` is behaupted** — the COMMIT outcome after a
Timeout/Cancellation is explicitly **unknown**, which is exactly why the
acceptance is kept pending rather than resolved optimistically. No second
Spool-/Persistenzpipeline und kein Reward-Nachweis.

### Testclient und Nachweisgrenzen

`testclient/tc_merchant.gd` erweitert die vorhandene NPC-Auswahl um Questliste,
Beschreibung, „Annehmen“ und Lade-/Fehler-/Pending-/Erfolgsanzeige. `tc_main.gd`
leitet die neuen Aktionen und Antworten auf demselben Protokoll weiter.
Nachrichten werden anhand von `seq`, Aktion und NPC der laufenden Anfrage
zugeordnet; späte Antworten nach Auswahlwechsel/Logout werden verworfen.
ACTIVE wird lokal erst nach ausdrücklicher Serverbestätigung angezeigt.
Bei NPC-Klick werden zunächst Questangebote abgefragt; ohne Angebote folgt
die bisherige Händleröffnung. Die Händleraktionen selbst bleiben unverändert.
`testclient/` bleibt von Git ignoriert, Godot-Version 3.6.3.

**BOUND-Tests: Suite 925/925 offline bestanden** (920 Vorlauf + 4 neue
`npc_quest.rs`-Hang-/Cancellation-Nachweise + 1 neue `net.rs`-Shutdown-
Deadline-Test); 901 als berichteter Vorlauf anerkannt. Enthalten sind echte
Handler-/Dispatch-Aufrufe, konkurrierente Annahme, Fehler und COMMIT-
Antwortverlust, die bounded-Pending-Nachweise (Uncertain-COMMIT then
PERMANENT failure stays pending, identical resumption after DB returns,
pending blocks a second accept, offers exclude a pending quest, Logout/Takeover
of a pending character never falsely active, per-character Gate independence)
AND die **HANG-Pfad mit einer niemals-zurückkehrenden Store-Future +
gesteuerter Tokio-Zeit**: first attempt runs off and stays pending
(`npc_quest_hang_first_attempt_stays_pending_gate_free`), retry runs off then
DB returns resolves identically without double (`npc_quest_retry_hang_then_db_
returns_resolves_no_double`), Cancellation around a hang keeps the pending
entry (`npc_quest_cancellation_around_hang_pending_survives`), a character-
scoped retry does not wait on a foreign hang (`npc_quest_char_scoped_retry_
does_not_wait_on_foreign_hang`), and Shutdown's whole pass is bounded by a real
Deadline even with a hanging attempt and blocked Gate
(`npc_quest_shutdown_deadline_bounds_hang_and_blocked_gate`). Loader-Nachweise
mit/ohne/ungültigem `giver` bleiben. Der DB-Adapter ist ausdrücklich ein
Modell, keine echte SQL-/MariaDB-/Crash-Verifikation. **No bloß immediate
error is used as a Hang-Nachweis** (mode 4 never returns; the IO-Timeout/
Deadline fires). Keine Live-DB, Startdaten, Migrationen oder Dienststarts.

Der tatsächliche Quest-Panel-Aufbau und dessen Signal-/Antwortpfad bestehen
**19/19 Godot-3.6.3-Prüfungen** mit projektlokalem Prüfdisplay. Der vollständige
Client ist durch vorbestehende Einrückungs-/Parsefehler der uncommitteten
Elternkontroll-Erweiterung blockiert (`tc_main.gd:108`, `tc_parental.gd:147`);
diese fremden Arbeitsabschnitte wurden erhalten. Deshalb keine vollständige
Client-Szenen-, visuelle oder Live-E2E-Abnahme behauptet. Artefakte:
`.tmp/quest-offer-acceptance/`.

## 27.28 Abgabe und Belohnungen – eingebaut (Durable Quest-Completion)

Der Abgabe-/Belohnungs-Pfad ist **durchgängig umgesetzt** (Productions-Pfad,
offline-geprüft). Der Freigabe in `Quest-Turn-In-Persistenz.md` ist GRANTED:
die existing Spool-/Drain-Pfad is extended with a single-character
`quest_completion`-order and the additive Receipt-Table `character_quest_commits`
(Migration 025, created + registered, NOT executed against a real DB). A
deliberate NPC turn-in (`quest_turn_in`) at the bound `turn_in`-NPC completes a
fulfilled Killquest with the prepared EXP-/Idia rewards.

Embedded production path:

- Optionales `turn_in` als nichtleere, beim Laden auflösbare stabile
  NPC-Content-ID, getrennt von `giver`. Ohne explizites `turn_in` keine
  Abgabevorschau. Ein Fallback ist durch §27.22 bislang nicht festgelegt.
- `rewards = {exp = N, idia = M}`: nichtnegative Ganzzahlen bis i64::MAX,
  fehlende Felder null. Andere Angaben, auch `xp`, `gold`, `items` und
  Auswahlbelohnungen, werden ausdrücklich abgewiesen. Keine Balancewerte
  oder Content-Seeds. Die reine Reward-Vorberechnung verwendet den bestehenden
  `Progression::grant_exp`-Kern ohne Rested-Bonus, prüft Überläufe und verändert
keinen Charakter.
- `NPC_TALK {action:"quest_turn_ins", npc_id}` liefert ACTIVE-Killquests am
  expliziten Abgabe-NPC mit `objectives_met`, `rewards:{exp,idia}` und
  **`can_turn_in` = objectives_met** (server-driven).
- `NPC_TALK {action:"quest_turn_in", npc_id, quest_id}` prüft Eigentum,
  NPC-/Spielerleben, Reichweite, Definition, Zuordnung, ACTIVE und echte
  Zielerfüllung. On success it builds ONE immutable `quest_completion`-order
  (full resulting `PersistSnapshot` incl. Lifecycle + the once-computed Reward,
  the COMPLETED quest state, base/target-Revision, technical commit-ID), publishes
  it durably over the existing Spool-/Drain-Pfad and performs the RAM handoff;
  it answers `NPC_TEXT {ok:true, state:"COMPLETED", rewards:{exp,idia}}`. An
  uncertain/failed publication keeps the identical order pending
  (`quest_turn_in_pending`/`quest_turn_in_busy`), NO false COMPLETED, NO reward
  claim, NO rollback. `seq` stays correlation. The Gate acquisition is bounded
  (2-s pattern).
- The additive Receipt-Table `character_quest_commits` (Migration 025) carries the
  canonical order in the SAME transaction as the full snapshot, the ACTIVE→COMPLETED
  row and `persist_revision`. Repetition is confirmed ONLY by a matching content
  receipt; revision equality or a COMPLETED row alone never yields AlreadyApplied.
- The ignorierte Godot-3.6-Testclient shows the reward preview, enables „Abgeben“
  only on server `can_turn_in`, and displays pending/success(COMPLETED)/rejection
  reasons. It never awards EXP/Idia locally.

**Prüfungen dieses Stand:** 935 Tests als berichteter Vorlauf anerkannt;
**947/947 Realm-Tests offline bestanden** (12 zusätzliche `quest_completion`-Tests:
drain applies snapshot+COMPLETED+receipt, older snapshot never overwrites, normal
publication waits while open, DB-error keeps order pending, divergent/missing
receipt = conflict, attribution survives unreadable payload, filename routing,
sync-failure confirms identical content only, failed publication retains order +
retry reuses identical content, overflow rejects without mutation, logout resolves
open order before gate, shutdown retry deadline-bounded). Godot 3.6.3: **13/13**
Panel-/Signal-/Vorschau-/COMPLETED-Prüfungen. Clippy `--all-targets`: only the
pre-existing `health.rs:411` deny-error (`unused_io_amount`), no new error.
`git diff --check` clean. No live DB/SQL/Crash proof. Protokolle: `.tmp/quest-turn-in/`.

**Offen (not in scope):** Itembelohnungen (no real inventory/UUID re-adoption proof),
further objective bindings, Repeatable-extensions, Questlog, new balance. No live
DB access, seeds, migration execution, service starts or installation.
nicht unterstützt; Inventarvoll-/stabile UUID-/Crash-Nachweise für eine echte
Vergabe werden nicht behauptet. Prüfartefakte: `.tmp/quest-turn-in/`.
