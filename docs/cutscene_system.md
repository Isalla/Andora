# Andora – Cutscene System / Architektur-Übergang

## Status

**Altes Konzept überarbeitet**

Dieses Dokument beschreibt die Überführung des ursprünglichen Godot-basierten Cutscene-Systems in die aktuelle Andora-Architektur.

Die ursprüngliche Idee von Cutscenes bleibt bestehen.

Die technische Verantwortung wurde jedoch vom Client auf die serverautoritative **Dynamic Scene Engine** verlagert.

Das ausführliche aktuelle System wird in der Dokumentation zum:

**AI Cutscene / Dynamic Scene System**

beschrieben.

---

# 1. Grundprinzip

Cutscenes sind in Andora keine isolierten Videos oder ausschließlich clientseitig abgespielten Sequenzen.

Sie sind inszenierte Ereignisse innerhalb der laufenden Spielwelt.

Sie können ausgelöst werden durch:

* Gebietseintritt
* Questfortschritt
* Questabschluss
* Bossbegegnungen
* Bossniederlagen
* Story-Ereignisse
* World Events
* besondere Charakterereignisse
* andere serverseitige Events

Die Scene Engine kontrolliert den Ablauf.

---

# 2. Alte Architektur

Das ursprüngliche Konzept sah eine zentrale Godot-Klasse vor:

```gdscript
extends Node
class_name Cutscene

var cutscene_name: String = ""
var duration: float = 0.0
var is_active: bool = false
var triggers = []

func play() -> void:
    pass

func stop() -> void:
    pass

func pause() -> void:
    pass

func resume() -> void:
    pass
```

Zusätzlich sollten andere Client-Systeme wie der `BossManager` Cutscenes direkt starten.

Beispiel:

```gdscript
func trigger_cutscene(scene_name: String, on_exit: bool = false) -> void:
    pass
```

Diese Architektur wird nicht weiterverwendet.

Der Godot-Client darf nicht die Autorität darüber besitzen, wann ein relevantes Story- oder Weltereignis ausgelöst wird.

---

# 3. Aktuelle Architektur

Die heutige Architektur lautet:

```text
Server Event
      ↓
TypeScript Scene Engine
      ↓
Lua Scene Definition
      ↓
AI / Ollama optional
      ↓
Godot Darstellung
```

Die Verantwortlichkeiten sind getrennt.

## TypeScript

Kontrolliert:

* Trigger
* Bedingungen
* Szenenzustand
* Teilnehmer
* Positionen
* NPC Scene Locks
* Bewegungsfreigaben
* Validierung
* Übergänge
* Konsequenzen

## Lua

Definiert:

* Szenenablauf
* Szenenphasen
* feste Texte
* i18n-Schlüssel
* NPC-Rollen
* AI-Prompt-Fragmente
* Content-spezifische Regeln

## AI / Ollama

Kann ausdrücklich freigegebene Dialogteile improvisieren und personalisieren.

## Godot

Stellt die vom Server vorgegebene Szene dar:

* Kamera
* Animation
* Figuren
* Bewegung
* Effekte
* Musik
* Sounds
* Dialogfenster
* Sprechblasen
* UI

---

# 4. Event-basierte Auslösung

Andere Gameplay-Systeme starten Cutscenes nicht direkt.

Sie erzeugen Ereignisse.

Beispiel:

```text
Boss
   ↓
BossDefeated
   ↓
Event System
   ↓
Scene Engine
   ↓
passende Lua-Szene?
   ↓
Scene starten
```

Dadurch muss ein Boss nicht wissen, welche Cutscene nach seinem Tod abgespielt werden soll.

Dasselbe Prinzip gilt für Quests, Gebiete und World Events.

---

# 5. Unterstützte Scene Trigger

Das ursprüngliche Trigger-Konzept bleibt erhalten und wird erweitert.

Mögliche Events:

```text
AreaEntered
QuestStarted
QuestProgressChanged
QuestCompleted
QuestFailed

BossEncounterStarted
BossDefeated
BossVictory
BossDefeat

StoryEventTriggered

WorldEventStarted
WorldEventChanged
WorldEventCompleted
WorldEventFailed

SpecialCharacterEvent
```

Weitere Events können später ergänzt werden.

---

# 6. Gebietstrigger

Szenen können weiterhin automatisch ausgelöst werden, wenn ein Spieler einen bestimmten Bereich betritt.

Die Entscheidung erfolgt jedoch serverseitig.

```text
Spieler bewegt sich
       ↓
Server aktualisiert Position
       ↓
Triggerbereich betreten?
       ↓
AreaEntered
       ↓
Event System
       ↓
Scene Engine prüft Bedingungen
       ↓
passende Szene vorhanden?
       ↓
Scene starten
```

Der Client darf nicht selbst entscheiden, dass ein Story-Trigger ausgelöst wurde.

---

# 7. Bedingungen für Gebietsszenen

Ein `AreaEntered` bedeutet nicht automatisch, dass eine Szene abgespielt werden muss.

Lua bzw. die Scene Definition kann zusätzliche Bedingungen enthalten.

Beispiele:

```text
Quest aktiv?
Story Flag vorhanden?
Szene bereits gesehen?
World Event aktiv?
bestimmter NPC verfügbar?
bestimmte Spielergruppe beteiligt?
bestimmte Tages-/Weltbedingung erfüllt?
```

Erst wenn der Server die Bedingungen bestätigt, startet die Szene.

---

# 8. Quest-Cutscenes

Quest-Szenen werden über Events mit dem Quest-System verbunden.

Beispiel:

```text
Quest Progress
      ↓
QuestProgressChanged
      ↓
Event System
      ↓
Scene Engine
```

Dadurch können Szenen beispielsweise auftreten:

* beim Start einer Quest
* bei wichtigen Zwischenschritten
* bei Entscheidungen
* bei Questabschluss
* bei Questfehlschlag

Das Quest-System selbst muss keine konkrete Cutscene kennen.

---

# 9. Boss-Cutscenes

Boss-Szenen können vor, während oder nach einer Begegnung verwendet werden.

Beispiele:

## Vor dem Kampf

```text
BossEncounterStarted
        ↓
Intro Scene
        ↓
Bosskampf
```

## Sieg

```text
BossDefeated
        ↓
Server bestimmt Ergebnis
        ↓
Victory Scene
```

## Niederlage

```text
BossDefeat
        ↓
Defeat Scene
```

Die Scene Engine kann dabei normale Figuren, NPCs, Animationen und Effekte innerhalb der Spielwelt verwenden.

---

# 10. Keine direkte BossManager-Abhängigkeit

Der BossManager soll keine Funktion benötigen wie:

```text
trigger_cutscene("boss_victory")
```

Stattdessen erzeugt das Boss-System ein Gameplay-Event.

```text
BossDefeated
```

Die Scene Engine entscheidet anschließend, ob und welche Szene darauf reagiert.

Dadurch bleiben Boss-System und Scene-System voneinander getrennt.

---

# 11. World Events

World Events können ebenfalls Szenen auslösen.

Beispiele:

```text
WorldEventStarted
WorldEventChanged
WorldEventCompleted
WorldEventFailed
```

Damit können beispielsweise inszeniert werden:

* Angriff auf eine Stadt
* Beginn einer Belagerung
* Erscheinen eines besonderen Gegners
* Sieg eines Gebietes
* Niederlage eines Gebietes
* Veränderung der Welt
* öffentliche Feste
* besondere serverweite Ereignisse

Die Weltlogik und das Ergebnis bestimmt immer der Server.

---

# 12. Scene States

Die ursprünglichen Konzepte:

```text
play
pause
resume
stop
```

werden in serverseitige Szenenzustände überführt.

Beispiel:

```text
PENDING
RUNNING
PAUSED
FINISHED
CANCELLED
```

## PENDING

Die Szene wurde vorbereitet, wartet aber noch auf notwendige Bedingungen.

## RUNNING

Die Szene läuft.

## PAUSED

Die Szene wurde kontrolliert angehalten.

Beispielsweise weil ein notwendiger Teilnehmer seine Position verlassen hat.

## FINISHED

Die Szene wurde erfolgreich abgeschlossen.

## CANCELLED

Die Szene wurde abgebrochen.

---

# 13. Pause und Resume

`PAUSED` bedeutet nicht nur, dass eine Animation angehalten wird.

Die gesamte logische Szenenphase wird gestoppt.

Beispiel:

```text
RUNNING
   ↓
notwendige Bedingung verletzt
   ↓
PAUSED
   ↓
NPC reagiert auf Situation
   ↓
Bedingung wieder erfüllt
   ↓
RUNNING
```

Währenddessen wird kein nachfolgender Szenenschritt unbeabsichtigt ausgeführt.

---

# 14. NPC-Steuerung

Benötigte NPCs können für eine laufende Szene durch die Scene Engine reserviert werden.

Während eines Scene Locks wird ihr normales autonomes Verhalten für die relevanten Bereiche temporär pausiert.

Dadurch kann verhindert werden, dass ein beteiligter NPC während einer Szene:

* weggeht
* seinem Tagesablauf folgt
* eine Reise beginnt
* zufällig herumläuft
* eine andere normale Tätigkeit startet

Die Scene Engine kann währenddessen szenenrelevante Dinge kontrollieren:

* Position
* Laufweg
* Blickrichtung
* Animation
* Haltung
* Dialog
* Szenenaktionen

Nach Ende oder Abbruch der Szene wird die Kontrolle wieder an das normale NPC-System übergeben.

Ein persistenter NPC wird für eine Cutscene niemals dupliziert.

---

# 15. AI-Integration

Nicht jede Cutscene benötigt KI.

Eine Szene kann vollständig fest definiert sein.

Alternativ können bestimmte Dialogbereiche durch AI/Ollama personalisiert werden.

```text
Scene Engine
     ↓
Lua Scene
     ↓
fester Abschnitt

     ↓
AI-Abschnitt
     ↓
AI Context
     ↓
Ollama
     ↓
personalisierter Dialog

     ↓
nächster fester Abschnitt
```

Die KI verändert keine Weltzustände und entscheidet nicht über den Ausgang der Szene.

---

# 16. Client-Aufgabe

Godot bleibt für die visuelle Präsentation verantwortlich.

Der Client darf beispielsweise:

* Kamera wechseln
* Animation abspielen
* NPC-Bewegung darstellen
* Effekte darstellen
* Musik abspielen
* Sprechblasen anzeigen
* Dialogfenster anzeigen

Der Client darf jedoch keine serverrelevanten Szenenergebnisse selbst bestimmen.

---

# 17. Serverautorität

Für alle gameplayrelevanten Szenen gilt:

> **Der Server entscheidet, ob eine Szene beginnt, welche Bedingungen erfüllt sind, wann sie fortgesetzt wird und welche Konsequenzen sie besitzt.**

Godot stellt diese Entscheidungen dar.

Lua definiert den Content.

AI/Ollama kann Dialoge innerhalb des erlaubten Rahmens improvisieren.

---

# 18. Abgrenzung zum Dynamic Scene System

Dieses Dokument hält hauptsächlich fest, wie das ursprüngliche Cutscene-Konzept in die aktuelle Architektur überführt wurde.

Die vollständigen Regeln für:

* AI Context
* personalisierte Dialoge
* Multiplayer-Szenen
* i18n
* Hochzeiten
* dynamische Szenenunterbrechungen
* NPC Scene Locks
* feste und improvisierte Dialoge
* AI-Fallbacks
* Narrative Context

werden in der Dokumentation:

**AI Cutscene / Dynamic Scene System**

geführt.

---

# 19. Architekturregel

Das ursprüngliche clientseitige Cutscene-System wird durch eine serverautoritative, eventbasierte Scene Engine ersetzt.

```text
Gameplay-System
       ↓
Game Event
       ↓
Event System
       ↓
Scene Engine
       ↓
Lua Scene Definition
       ↓
AI optional
       ↓
Godot Darstellung
```

Dadurch bleiben Gameplay-Systeme und Szeneninhalte voneinander getrennt.

> **Gameplay-Systeme melden, was passiert ist. Die Scene Engine entscheidet, wie dieses Ereignis inszeniert wird.**
